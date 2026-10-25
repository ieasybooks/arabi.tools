#[allow(dead_code)]
#[path = "support/drive.rs"]
mod drive;
#[allow(dead_code)]
mod support;

use std::io::{Cursor, Read};
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Context;
use arlang_tools::queue::{JobId, Queue, Rejection, Status};
use arlang_tools::tools::ocr::{
    Document, DocumentProblem, MAX_UPLOAD_BYTES, Ocr, UploadedDocument, drive_config,
};
use arlang_tools::tools::transcription;
use reqwest::{StatusCode, multipart};
use serde_json::{Value, json};
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path},
};

fn pdfium() -> Option<PathBuf> {
    std::env::var_os("TAHWEEL_TEST_PDFIUM").map(PathBuf::from)
}

async fn ocr(drive: &drive::Drive) -> anyhow::Result<Ocr> {
    Ocr::new(drive.config(), pdfium()).await
}

fn upload(name: &str, bytes: &[u8]) -> UploadedDocument {
    let upload = UploadedDocument::new(name).unwrap();
    std::fs::write(&upload.path, bytes).unwrap();
    upload
}

async fn finished(ocr: &Ocr, id: &JobId) -> Status<Document> {
    let mut watch = ocr.queue.watch(id).unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let status = watch.status();
            if status.is_finished() {
                return status;
            }
            assert!(watch.changed().await);
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn uploads_preview_and_all_downloads_in_both_languages() -> anyhow::Result<()> {
    let drive = drive::Drive::start(Duration::from_millis(150)).await;
    let ocr = ocr(&drive).await?;
    let app = support::TestApp::start_with_ocr(support::Setup::default(), ocr).await?;
    let mut cases = vec![("page.png", drive::png(), 1)];
    let mut jpeg = Cursor::new(Vec::new());
    image::GrayImage::from_pixel(32, 32, image::Luma([255]))
        .write_to(&mut jpeg, image::ImageFormat::Jpeg)?;
    cases.push(("page.JPG", jpeg.into_inner(), 1));
    if pdfium().is_some() {
        cases.push(("كتاب.pdf", drive::pdf(3), 3));
    }
    for (name, bytes, pages) in cases {
        let reservation: Value = app
            .client
            .post(app.url("/ocr/jobs"))
            .send()
            .await?
            .json()
            .await?;
        let path = reservation["upload"]
            .as_str()
            .context("missing reservation")?;
        let reply: Value = app
            .upload(path, name, &bytes, None)
            .await?
            .error_for_status()?
            .json()
            .await?;
        let job = reply["next"].as_str().unwrap();
        let id: JobId = job.rsplit('/').next().unwrap().parse()?;
        assert!(matches!(finished(&app.ocr, &id).await, Status::Done(_)));
        for (code, done) in [
            ("ar", "اكتمل استخراج النص."),
            ("en", "Text recognition complete."),
        ] {
            let page = app
                .client
                .get(app.url(job))
                .header("Cookie", format!("lang={code}"))
                .send()
                .await?
                .text()
                .await?;
            assert!(page.contains(done));
            assert!(page.contains(&format!("lang=\"{code}\"")));
            assert!(page.contains(drive::TEXT));
            assert!(page.contains("noindex"));
            assert_eq!(page.matches("class=\"cue\"").count(), pages);
        }
        for extension in ["txt", "docx", "json"] {
            let response = app
                .client
                .get(app.url(&format!("{job}/document.{extension}")))
                .send()
                .await?;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["cache-control"], "no-store");
            assert!(
                response.headers()["content-disposition"]
                    .to_str()?
                    .contains("filename*=UTF-8''")
            );
            let bytes = response.bytes().await?;
            match extension {
                "txt" => assert_eq!(
                    String::from_utf8(bytes.to_vec())?
                        .matches(drive::TEXT)
                        .count(),
                    pages
                ),
                "json" => {
                    let value: Value = serde_json::from_slice(&bytes)?;
                    assert_eq!(value.as_array().unwrap().len(), pages);
                    assert_eq!(value[0], json!({"page":1,"content":drive::TEXT}));
                }
                _ => {
                    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
                    let mut xml = String::new();
                    archive
                        .by_name("word/document.xml")?
                        .read_to_string(&mut xml)?;
                    assert!(xml.contains("مرحبا بالعالم"));
                }
            }
        }
    }
    drive.assert_clean();
    app.stop().await
}

#[tokio::test]
async fn invalid_uploads_are_refused_before_queueing() -> anyhow::Result<()> {
    let drive = drive::Drive::start(Duration::ZERO).await;
    let app =
        support::TestApp::start_with_ocr(support::Setup::default(), ocr(&drive).await?).await?;
    let mut cases = vec![
        ("file.gif", drive::png()),
        ("file.png", b"broken".to_vec()),
        ("file.jpg", drive::png()),
    ];
    if pdfium().is_some() {
        cases.push(("too-many.pdf", drive::pdf(1001)));
    }
    for (name, bytes) in cases {
        let reservation: Value = app
            .client
            .post(app.url("/ocr/jobs"))
            .send()
            .await?
            .json()
            .await?;
        let path = reservation["upload"].as_str().unwrap();
        let response = app.upload(path, name, &bytes, None).await?;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(response.json::<Value>().await?["error"].is_string());
    }
    for count in [0, 2] {
        let reservation: Value = app
            .client
            .post(app.url("/ocr/jobs"))
            .send()
            .await?
            .json()
            .await?;
        let mut form = multipart::Form::new().text("ignored", "value");
        for _ in 0..count {
            form = form.part(
                "file",
                multipart::Part::bytes(drive::png()).file_name("page.png"),
            );
        }
        let response = app
            .client
            .post(app.url(reservation["upload"].as_str().unwrap()))
            .multipart(form)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    // Exercise the streaming upload limit as well as the content validation above.
    let reservation: Value = app
        .client
        .post(app.url("/ocr/jobs"))
        .send()
        .await?
        .json()
        .await?;
    let response = app
        .upload(
            reservation["upload"].as_str().unwrap(),
            "large.png",
            &vec![0; usize::try_from(MAX_UPLOAD_BYTES + 1)?],
            None,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        response.json::<Value>().await?["error"]
            .as_str()
            .unwrap()
            .contains("99")
    );
    assert_eq!(
        drive.uploads.load(Ordering::SeqCst),
        1,
        "only the startup probe reached Drive"
    );
    app.stop().await
}

#[tokio::test]
async fn validates_size_boundary_and_page_cap_and_removes_rejected_uploads() -> anyhow::Result<()> {
    let drive = drive::Drive::start(Duration::ZERO).await;
    let ocr = ocr(&drive).await?;
    for (size, accepted) in [(MAX_UPLOAD_BYTES, true), (MAX_UPLOAD_BYTES + 1, false)] {
        let file = upload("page.png", &drive::png());
        std::fs::OpenOptions::new()
            .write(true)
            .open(&file.path)?
            .set_len(size)?;
        let path = file.path.clone();
        let result = ocr.validate(file).await;
        if accepted {
            assert!(result.is_ok());
        } else {
            assert!(matches!(result, Err(DocumentProblem::TooLarge)));
        }
        drop(result);
        assert!(!path.parent().unwrap().exists());
    }
    if pdfium().is_some() {
        assert!(
            ocr.validate(upload("book.pdf", &drive::pdf(1000)))
                .await
                .is_ok()
        );
        assert!(matches!(
            ocr.validate(upload("book.pdf", &drive::pdf(1001))).await,
            Err(DocumentProblem::TooManyPages)
        ));
    }
    Ok(())
}

#[tokio::test]
async fn processing_rechecks_page_cap() -> anyhow::Result<()> {
    if pdfium().is_none() {
        eprintln!("skipping PDF cap recheck: set TAHWEEL_TEST_PDFIUM");
        return Ok(());
    }
    let drive = drive::Drive::start(Duration::ZERO).await;
    let ocr = ocr(&drive).await?;
    let upload = ocr
        .validate(upload("book.pdf", &drive::pdf(1)))
        .await
        .unwrap();
    std::fs::write(&upload.path, drive::pdf(1001))?;
    let path = upload.path.clone();
    let id = ocr.queue.reserve(None, None).unwrap();
    ocr.submit(ocr.queue.begin_upload(&id).unwrap(), upload, "book".into())
        .unwrap();
    assert!(matches!(finished(&ocr, &id).await, Status::Failed));
    assert!(!path.exists());
    assert_eq!(drive.uploads.load(Ordering::SeqCst), 1);
    drive.assert_clean();
    Ok(())
}

#[tokio::test]
async fn progress_and_cancellation_finish_active_pages_and_delete_upload() -> anyhow::Result<()> {
    if pdfium().is_none() {
        eprintln!("skipping PDF cancellation: set TAHWEEL_TEST_PDFIUM");
        return Ok(());
    }
    let drive = drive::Drive::start(Duration::from_millis(300)).await;
    let mut ocr = ocr(&drive).await?;
    let mut limits = transcription::limits();
    limits.unwatched = Duration::from_millis(20);
    ocr.queue = Queue::new(limits);
    let upload = upload("book.pdf", &drive::pdf(20));
    let path = upload.path.clone();
    let id = ocr.queue.reserve(None, None).unwrap();
    ocr.submit(ocr.queue.begin_upload(&id).unwrap(), upload, "book".into())
        .unwrap();
    let mut watch = ocr.queue.watch(&id).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !matches!(watch.status(), Status::Running { pages: Some((done, 20)), .. } if done > 0)
        {
            assert!(watch.changed().await);
        }
    })
    .await?;
    drop(watch);
    tokio::time::sleep(Duration::from_millis(30)).await;
    ocr.queue.sweep();
    assert!(matches!(finished(&ocr, &id).await, Status::Cancelled(_)));
    assert_eq!(ocr.queue.reserve(None, None), Err(Rejection::ActiveJob));
    tokio::time::timeout(Duration::from_secs(10), async {
        while path.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    ocr.queue.shutdown(Duration::ZERO).await;
    assert!(drive.uploads.load(Ordering::SeqCst) < 21);
    drive.assert_clean();
    assert!(ocr.queue.result(&id).is_none());
    Ok(())
}

#[tokio::test]
async fn unavailable_ocr_does_not_disable_transcription() -> anyhow::Result<()> {
    for json in [
        "",
        "[]",
        "not-json",
        r#"[{"type":"service_account","private_key":"do-not-log"}]"#,
    ] {
        let error = drive_config(json).unwrap_err();
        assert!(!format!("{error:?}").contains("do-not-log"));
    }
    let drive = drive::Drive::start(Duration::ZERO).await;
    Mock::given(method("POST"))
        .and(path("/upload/drive/v3/files"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"error":{"errors":[{"reason":"storageQuotaExceeded"}]}})),
        )
        .with_priority(1)
        .mount(&drive.server)
        .await;
    let error = ocr(&drive)
        .await
        .err()
        .context("storage-less accounts should fail startup")?;
    assert!(error.to_string().contains("checking Drive OCR storage"));
    let app = support::TestApp::start().await?;
    for (code, message) in [
        ("ar", "أداة استخراج النص غير متاحة مؤقتًا"),
        ("en", "Text recognition is temporarily unavailable"),
    ] {
        let html = app
            .client
            .get(app.url("/ocr"))
            .header("Cookie", format!("lang={code}"))
            .send()
            .await?
            .text()
            .await?;
        assert!(html.contains(message));
        assert!(!html.contains("id=\"upload-form\""));
    }
    assert_eq!(
        app.client.post(app.url("/ocr/jobs")).send().await?.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let reservation = app.reserve().await?;
    let reply: Value = app
        .upload(&reservation, "speech.mp3", support::MP3, None)
        .await?
        .json()
        .await?;
    app.wait_until_done(reply["next"].as_str().unwrap()).await?;
    app.stop().await
}

#[tokio::test]
async fn ocr_queue_enforces_limits() -> anyhow::Result<()> {
    let drive = drive::Drive::start(Duration::ZERO).await;
    let mut ocr = ocr(&drive).await?;
    for index in 1..=5 {
        let client = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, index)));
        let id = ocr.queue.reserve(client, None).unwrap();
        ocr.queue
            .begin_upload(&id)
            .unwrap()
            .submit(|_| std::future::pending::<anyhow::Result<Document>>())
            .unwrap();
        let status = ocr.queue.watch(&id).unwrap().status();
        assert_eq!(matches!(status, Status::Running { .. }), index <= 4);
        if index == 5 {
            assert!(matches!(status, Status::Waiting { position: 1 }));
        }
    }
    ocr.queue.shutdown(Duration::ZERO).await;
    ocr.queue = Queue::new(transcription::limits());
    for index in 1..=100 {
        let client = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, index)));
        ocr.queue.reserve(client, None).unwrap();
        assert_eq!(ocr.queue.reserve(client, None), Err(Rejection::ActiveJob));
    }
    assert_eq!(ocr.queue.reserve(None, None), Err(Rejection::Full));
    ocr.queue.shutdown(Duration::ZERO).await;
    Ok(())
}

#[tokio::test]
async fn drive_startup_does_not_block_transcription() -> anyhow::Result<()> {
    let drive = drive::Drive::start(Duration::from_secs(1)).await;
    let ocr = Ocr::unavailable().with_pdfium_library(pdfium());
    let app = support::TestApp::start_with_ocr(support::Setup::default(), ocr.clone()).await?;
    let config = drive.config();
    let starting = tokio::spawn(async move { ocr.initialize(config).await });
    assert!(!app.ocr.is_available());
    let reservation = app.reserve().await?;
    assert!(reservation.ends_with("/audio"));
    assert_eq!(
        app.client.get(app.url("/health")).send().await?.status(),
        StatusCode::OK
    );
    starting.await??;
    assert!(app.ocr.is_available());
    let html = app.client.get(app.url("/ocr")).send().await?.text().await?;
    assert!(html.contains("id=\"upload-form\""));
    drive.assert_clean();
    app.stop().await
}
