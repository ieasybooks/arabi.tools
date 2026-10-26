// Expected mark ordering is copied from the released model reference fixtures.
#![allow(clippy::unicode_not_nfc)]

#[allow(dead_code)]
mod support;

use std::{
    io::{Cursor, Read, Write},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, bail};
use arlang_tools::{
    queue::{CancelReason, JobId, Slots, Status},
    tools::{
        RUNNING_JOBS,
        diacritization::{Diacritization, Document, Model},
    },
};
use reqwest::{Client, StatusCode, multipart};
use serde_json::Value;
use support::{TestApp, json_reply};
use tashkeel::text::strip_tashkeel;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const UNKNOWN: &str = "/diacritize/jobs/00000000000000000000000000000000";
const MIXED: &str =
    "\u{feff}  اللغة العربية جميلة، وفي عام 2026 نكتب Rust.\r\nهذا اختبار للتشكيل.\t ";
// Python ONNX Runtime CPU reference text/labels from tashkeel-rs v0.1.0
// crates/tashkeel/tests/host_environment.rs (MIT). These catch a model-routing
// or reconstruction regression which a marks-present smoke test would miss.
const REFERENCES: [(Model, &str, &str); 2] = [
    (
        Model::Shakkelha,
        "اللغة العربية لغة جميلة وغنية، وهي وسيلة للتواصل بين الناس ونقل العلوم والمعارف. يقرأ الطالب كتابا م",
        "اللُّغَةُ الْعَرَبِيَّةُ لُغَةٌ جَمِيلَةٌ وَغَنِيَّةٌ، وَهِيَ وَسِيلَةٌ لِلتَّوَاصُلِ بَيْنَ النَّاسِ وَنَقْلِ الْعُلُومِ وَالْمَعَارِفِ. يَقْرَأُ الطَّالِبُ كِتَابًا م",
    ),
    (
        Model::Catt,
        "سطا لصوص على منزل وزيرة الدفاع التشيلية مايا فرنانديز بعد ضرب ابنها وتهديد زوجها وفق ما أعلنته السلط",
        "سَطَا لُصُوصٌ عَلَى مَنْزِلِ وَزِيرَةِ الدِّفَاعِ التَّشَيُّلِيَّةِ مَايا فرنَانْدِيز بَعْدَ ضَرْبِ ابْنِهَا وَتَهْدِيدِ زَوْجِهَا وَفْقَ مَا أَعْلَنَتْهُ السَّلْطُ",
    ),
];

#[tokio::test]
async fn the_tool_is_discoverable_in_both_languages_without_models() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    for (lang, name) in [("ar", "تشكيل النص العربي"), ("en", "Arabic diacritization")]
    {
        let home = app
            .client
            .get(app.url("/"))
            .header("cookie", format!("lang={lang}"))
            .send()
            .await?
            .text()
            .await?;
        assert!(home.contains("/diacritize"));
        assert!(home.contains(name), "{home}");
        let page = app
            .client
            .get(app.url("/diacritize"))
            .header("cookie", format!("lang={lang}"))
            .send()
            .await?;
        assert_eq!(page.status(), StatusCode::OK);
        let page = page.text().await?;
        assert!(page.contains("شكِّلها"));
        assert!(page.contains("CATT"));
        assert!(page.contains("WebApplication"));
        assert!(page.contains(&format!("lang=\"{lang}\"")));
    }
    let sitemap = app
        .client
        .get(app.url("/sitemap.xml"))
        .send()
        .await?
        .text()
        .await?;
    assert!(sitemap.contains(&format!(
        "<loc>{}/diacritize</loc>",
        app.url("/").trim_end_matches('/')
    )));
    let reply = json_reply(
        app.client.post(app.url("/diacritize/jobs")).send().await?,
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await?;
    assert!(reply["error"].is_string());
    app.stop().await
}

#[tokio::test]
async fn unknown_results_are_private_and_text_requests_have_a_small_body_limit()
-> anyhow::Result<()> {
    let app = TestApp::start().await?;
    for suffix in ["", "/result", "/original"] {
        let response = app
            .client
            .get(app.url(&format!("{UNKNOWN}{suffix}")))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    assert_eq!(
        app.client
            .post(app.url(&format!("{UNKNOWN}/cancel")))
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    let response = app
        .client
        .post(app.url(&format!("{UNKNOWN}/text")))
        .form(&[("model", "shakkelha"), ("text", &"x".repeat(100_000))])
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    app.stop().await
}

#[tokio::test]
async fn a_job_url_cannot_be_forged_into_a_cancellation_cookie() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    let id = app.diacritization.queue.reserve(None, None).unwrap();
    let outsider = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = outsider
        .post(app.url(&format!("/diacritize/jobs/{id}/cancel")))
        .header("cookie", format!("diacritize_job={id}"))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(app.diacritization.queue.is_unfinished(&id));
    app.stop().await
}

async fn real_app() -> anyhow::Result<TestApp> {
    let cache = PathBuf::from(
        std::env::var_os("TASHKEEL_MODEL_CACHE")
            .context("set TASHKEEL_MODEL_CACHE for ignored tests")?,
    );
    let runtime = std::env::var_os("ORT_DYLIB_PATH").map(PathBuf::from);
    let tool = Diacritization::unavailable(&Slots::new(RUNNING_JOBS));
    tool.initialize(&cache, runtime).await?;
    TestApp::start_with_diacritization(tool).await
}

async fn reserve(app: &TestApp) -> anyhow::Result<Value> {
    json_reply(
        app.client
            .post(app.url("/diacritize/jobs"))
            .form(&[("attempt", format!("{:032x}", rand::random::<u128>()))])
            .send()
            .await?,
        StatusCode::OK,
    )
    .await
}

async fn finished(app: &TestApp, job: &str) -> anyhow::Result<Arc<Document>> {
    let id: JobId = job.rsplit('/').next().context("missing id")?.parse()?;
    let mut watch = app.diacritization.queue.watch(&id).context("missing job")?;
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            match watch.status() {
                Status::Done(document) => return Ok(document),
                Status::Failed | Status::Cancelled(_) => {
                    bail!("diacritization failed or cancelled")
                }
                _ => {}
            }
            if !watch.changed().await {
                bail!("job disappeared");
            }
        }
    })
    .await
    .context("diacritization timed out")?
}

async fn submit_text(app: &TestApp, model: &str, text: &str) -> anyhow::Result<String> {
    let reservation = reserve(app).await?;
    let endpoint = reservation["text"].as_str().context("text endpoint")?;
    let reply = json_reply(
        app.client
            .post(app.url(endpoint))
            .form(&[("model", model), ("text", text)])
            .send()
            .await?,
        StatusCode::OK,
    )
    .await?;
    Ok(reply["next"].as_str().context("job page")?.into())
}

async fn submit_file(
    app: &TestApp,
    model: &str,
    name: &str,
    bytes: Vec<u8>,
) -> anyhow::Result<String> {
    let reservation = reserve(app).await?;
    let form = multipart::Form::new().text("model", model.to_owned()).part(
        "file",
        multipart::Part::bytes(bytes).file_name(name.to_owned()),
    );
    let reply = json_reply(
        app.client
            .post(app.url(reservation["upload"].as_str().context("upload endpoint")?))
            .multipart(form)
            .send()
            .await?,
        StatusCode::OK,
    )
    .await?;
    Ok(reply["next"].as_str().context("job page")?.into())
}

#[tokio::test]
#[ignore = "requires the verified model cache and native ONNX Runtime"]
async fn real_http_models_match_reference_outputs_and_retain_the_original_text()
-> anyhow::Result<()> {
    let app = real_app().await?;
    for (model, source, expected) in REFERENCES {
        let job = submit_text(&app, model.as_str(), source).await?;
        let document = finished(&app, &job).await?;
        assert_eq!(document.model, model);
        assert_eq!(document.text.as_ref().unwrap().source, source);
        assert_eq!(document.text.as_ref().unwrap().output, expected);
        let response = app
            .client
            .get(app.url(&format!("{job}/result")))
            .send()
            .await?;
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(
            response.headers()["content-type"],
            "text/plain; charset=utf-8"
        );
        assert_eq!(response.text().await?, expected);
        let original = app
            .client
            .get(app.url(&format!("{job}/original")))
            .send()
            .await?
            .text()
            .await?;
        assert_eq!(original, source);
        let page = app.client.get(app.url(&job)).send().await?.text().await?;
        assert!(page.contains("noindex"));
        assert!(page.contains(source));
    }
    // The limit is Unicode scalars, not UTF-8 bytes or UTF-16 code units.
    let boundary = format!("{}ا", "🙂".repeat(999));
    let job = submit_text(&app, "catt", &boundary).await?;
    let document = finished(&app, &job).await?;
    assert_eq!(
        strip_tashkeel(&document.text.as_ref().unwrap().output),
        boundary
    );
    app.stop().await
}

#[tokio::test]
#[ignore = "requires the verified model cache and native ONNX Runtime"]
async fn real_http_uploads_preserve_txt_docx_and_original_downloads() -> anyhow::Result<()> {
    let app = real_app().await?;
    for model in ["shakkelha", "catt"] {
        let job = submit_file(&app, model, "نص.TXT", MIXED.as_bytes().to_vec()).await?;
        let document = finished(&app, &job).await?;
        assert!(
            document.text.is_none(),
            "file results should remain disk-backed"
        );
        let output = app
            .client
            .get(app.url(&format!("{job}/result")))
            .send()
            .await?
            .bytes()
            .await?;
        // Response::text strips a UTF-8 BOM; compare the actual downloaded text.
        let output = String::from_utf8(output.to_vec())?;
        assert_eq!(strip_tashkeel(&output), strip_tashkeel(MIXED));
        assert_ne!(output, strip_tashkeel(MIXED));
        assert_eq!(
            app.client
                .get(app.url(&format!("{job}/original")))
                .send()
                .await?
                .bytes()
                .await?
                .as_ref(),
            MIXED.as_bytes()
        );
        let original = docx()?;
        let job = submit_file(&app, model, "منسق.docx", original.clone()).await?;
        finished(&app, &job).await?;
        let response = app
            .client
            .get(app.url(&format!("{job}/result")))
            .send()
            .await?;
        assert_eq!(
            response.headers()["content-type"],
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        let output = response.bytes().await?;
        assert_eq!(
            part(&output, "word/styles.xml")?,
            part(&original, "word/styles.xml")?
        );
        let before = String::from_utf8(part(&original, "word/document.xml")?)?;
        let after = String::from_utf8(part(&output, "word/document.xml")?)?;
        assert_ne!(before, after);
        assert_eq!(strip_tashkeel(&before), strip_tashkeel(&after));
        assert_eq!(
            app.client
                .get(app.url(&format!("{job}/original")))
                .send()
                .await?
                .bytes()
                .await?
                .as_ref(),
            original
        );
    }
    app.stop().await
}

#[tokio::test]
#[ignore = "requires the verified model cache and native ONNX Runtime"]
async fn real_http_rejections_release_reservations_and_cancel_requires_the_job_cookie()
-> anyhow::Result<()> {
    let app = real_app().await?;
    for (model, text) in [
        ("other", "مرحبا".into()),
        ("shakkelha", String::new()),
        ("catt", "🙂".repeat(1001)),
    ] {
        let reservation = reserve(&app).await?;
        json_reply(
            app.client
                .post(app.url(reservation["text"].as_str().unwrap()))
                .form(&[("model", model), ("text", &text)])
                .send()
                .await?,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await?;
    }
    for (name, bytes) in [
        ("legacy.doc", vec![0]),
        ("invalid.txt", vec![0xff]),
        ("invalid.docx", b"not a zip".to_vec()),
    ] {
        let reservation = reserve(&app).await?;
        let form = multipart::Form::new()
            .text("model", "shakkelha")
            .part("file", multipart::Part::bytes(bytes).file_name(name));
        json_reply(
            app.client
                .post(app.url(reservation["upload"].as_str().unwrap()))
                .multipart(form)
                .send()
                .await?,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await?;
    }
    let reservation = reserve(&app).await?;
    let endpoint = reservation["cancel"].as_str().unwrap();
    let outsider = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    assert_eq!(
        outsider.post(app.url(endpoint)).send().await?.status(),
        StatusCode::NOT_FOUND
    );
    let id: JobId = endpoint.split('/').nth(3).unwrap().parse()?;
    assert!(app.diacritization.queue.is_unfinished(&id));
    let response = app.client.post(app.url(endpoint)).send().await?;
    assert_eq!(response.status(), StatusCode::OK); // follows the 303 job-page redirect
    assert!(matches!(
        app.diacritization.queue.watch(&id).unwrap().status(),
        Status::Cancelled(CancelReason::User)
    ));
    json_reply(
        app.client
            .post(app.url(reservation["text"].as_str().unwrap()))
            .form(&[("model", "shakkelha"), ("text", "مرحبا")])
            .send()
            .await?,
        StatusCode::CONFLICT,
    )
    .await?;
    app.stop().await
}

fn docx() -> anyhow::Result<Vec<u8>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let files = [
        (
            "[Content_Types].xml",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">  اللغة العربية </w:t></w:r><w:r><w:t>جميلة، API 2026.</w:t></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Normal"/></w:styles>"#,
        ),
    ];
    for (name, content) in files {
        zip.start_file(name, SimpleFileOptions::default())?;
        zip.write_all(content.as_bytes())?;
    }
    Ok(zip.finish()?.into_inner())
}

fn part(bytes: &[u8], name: &str) -> anyhow::Result<Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut content = Vec::new();
    archive.by_name(name)?.read_to_end(&mut content)?;
    Ok(content)
}
