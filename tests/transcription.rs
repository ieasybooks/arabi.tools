mod support;

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use anyhow::Context;
use arlang_tools::queue::{JobId, Limits, Status};
use arlang_tools::tools::transcription::limits;
use reqwest::{StatusCode, multipart};
use serde_json::json;
use support::{ARABIC_TEXT, M4A, MP3, TestApp, UNKNOWN_JOB, assert_error, json_reply};

#[tokio::test]
async fn mp3_upload_finishes_with_arabic_text_and_named_downloads() -> anyhow::Result<()> {
    transcribe_fixture(
        MP3,
        "درس.mp3",
        Some("ar"),
        "transcript",
        "%D8%AF%D8%B1%D8%B3",
    )
    .await
}

#[tokio::test]
async fn m4a_upload_uses_default_language_and_has_named_downloads() -> anyhow::Result<()> {
    transcribe_fixture(M4A, "lesson 1.m4a", None, "lesson 1", "lesson%201").await
}

async fn transcribe_fixture(
    audio: &[u8],
    name: &str,
    language: Option<&str>,
    fallback: &str,
    encoded_name: &str,
) -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    let upload = app.reserve().await?;
    let reply = json_reply(
        app.upload(&upload, name, audio, language).await?,
        StatusCode::OK,
    )
    .await?;
    let job = reply["next"].as_str().context("missing job page URL")?;
    assert_eq!(reply, json!({"next": job}));
    assert_eq!(Some(job), upload.strip_suffix("/audio"));

    let response = app.client.get(app.url(job)).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    let page = response.text().await?;
    assert!(page.contains("<html lang=\"ar\" dir=\"rtl\">"), "{page}");
    assert!(page.contains("تفريغ الصوت"), "{page}");
    app.wait_until_done(job).await?;
    let page = app
        .client
        .get(app.url(job))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    assert!(page.contains("اكتمل التفريغ."), "{page}");
    assert!(
        page.contains("class=\"status success\" role=\"status\""),
        "{page}"
    );
    assert!(page.contains(ARABIC_TEXT), "{page}");
    for (extension, content_type) in [
        ("txt", "text/plain; charset=utf-8"),
        ("srt", "application/x-subrip; charset=utf-8"),
    ] {
        let path = format!("{job}/transcript.{extension}");
        assert!(page.contains(&path), "missing download link: {page}");
        let response = app.client.get(app.url(&path)).send().await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], content_type);
        assert_eq!(
            response.headers()["content-disposition"],
            format!(
                "attachment; filename=\"{fallback}.{extension}\"; filename*=UTF-8''{encoded_name}.{extension}"
            )
        );
        let body = response.text().await?;
        assert_ne!(body.trim(), "");
        if extension == "txt" {
            assert!(body.lines().all(|line| line == ARABIC_TEXT), "{body}");
        } else {
            assert_subtitles(&body)?;
        }
    }
    let requests = app
        .wit
        .received_requests()
        .await
        .context("Wit requests not recorded")?;
    let chunks: Vec<_> = requests
        .iter()
        .filter(|request| request.url.path() == "/dictation")
        .collect();
    assert!(
        chunks.len() > 1,
        "the fixture must exercise multiple speech chunks"
    );
    for chunk in chunks {
        assert!(
            chunk.body.starts_with(b"RIFF"),
            "Wit should receive WAV audio"
        );
        assert!(chunk.body.len() > 44, "Wit should receive actual samples");
    }
    // A completed job's cookie must not point to a different job on the same connection.
    let another = app.reserve().await?;
    let page = app
        .client
        .get(app.url("/transcribe"))
        .send()
        .await?
        .text()
        .await?;
    assert!(!page.contains(&format!("href=\"{job}\"")), "{page}");
    assert!(
        !page.contains(another.strip_suffix("/audio").unwrap()),
        "{page}"
    );
    app.stop().await
}

fn assert_subtitles(body: &str) -> anyhow::Result<()> {
    let normalized = body.replace("\r\n", "\n");
    let mut previous_end = 0;
    for (index, cue) in normalized.trim().split("\n\n").enumerate() {
        let mut lines = cue.lines();
        assert_eq!(lines.next(), Some((index + 1).to_string().as_str()));
        let timing = lines.next().context("missing SRT timing")?;
        let (start, end) = timing.split_once(" --> ").context("invalid SRT timing")?;
        let start = timestamp_millis(start)?;
        let end = timestamp_millis(end)?;
        assert!(start >= previous_end && end > start, "{timing}");
        assert_eq!(lines.collect::<Vec<_>>().join("\n"), ARABIC_TEXT);
        previous_end = end;
    }
    Ok(())
}

fn timestamp_millis(timestamp: &str) -> anyhow::Result<u64> {
    let parts: Vec<u64> = timestamp
        .split([':', ','])
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    let [hours, minutes, seconds, millis] = parts.as_slice() else {
        anyhow::bail!("invalid SRT timestamp: {timestamp}");
    };
    assert!(*minutes < 60 && *seconds < 60 && *millis < 1000);
    Ok(((hours * 60 + minutes) * 60 + seconds) * 1000 + millis)
}

#[tokio::test]
async fn refuses_a_wrong_extension_and_allows_retry() -> anyhow::Result<()> {
    refused_upload("speech.wav", MP3, "نقبل ملفات MP3 وM4A فقط.").await
}

#[tokio::test]
async fn refuses_non_audio_with_an_accepted_extension_and_allows_retry() -> anyhow::Result<()> {
    refused_upload(
        "speech.mp3",
        b"This is not audio.",
        "لم نتمكن من قراءة الملف تسجيلًا صوتيًا بصيغة MP3 أو M4A.",
    )
    .await
}

#[tokio::test]
async fn refuses_a_recording_over_three_hours_and_allows_retry() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("too-long.mp3");
    let output = tokio::time::timeout(
        Duration::from_secs(60),
        tokio::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=8000:cl=mono",
                "-t",
                "10801",
                "-c:a",
                "libmp3lame",
                "-b:a",
                "8k",
            ])
            .arg(&path)
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let audio = tokio::fs::read(path).await?;
    assert!(audio.len() < 12_000_000, "keep the generated fixture small");
    refused_upload("too-long.mp3", &audio, "مدة التسجيل أطول من 3 ساعات.").await
}

async fn refused_upload(name: &str, audio: &[u8], message: &str) -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    let upload = app.reserve().await?;
    let response = app.upload(&upload, name, audio, None).await?;
    assert!(!response.headers().contains_key("set-cookie"));
    assert_error(response, StatusCode::UNPROCESSABLE_ENTITY, message).await?;
    let retry = app.reserve().await?;
    assert_ne!(
        upload, retry,
        "a refused upload must release its reservation"
    );
    assert_eq!(
        app.wit.received_requests().await.unwrap().len(),
        1,
        "invalid files must not reach dictation"
    );
    app.stop().await
}

#[tokio::test]
async fn refuses_a_form_without_a_file_and_allows_retry() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    let upload = app.reserve().await?;
    let response = app
        .client
        .post(app.url(&upload))
        .multipart(multipart::Form::new().text("language", "ar"))
        .send()
        .await?;
    assert_error(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "اختر ملفًا صوتيًا أولًا.",
    )
    .await?;
    app.reserve().await?;
    app.stop().await
}

#[tokio::test]
async fn uploading_to_an_unknown_job_returns_conflict_with_a_json_error() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    let response = app
        .upload(&format!("{UNKNOWN_JOB}/audio"), "speech.mp3", MP3, None)
        .await?;
    assert_error(
        response,
        StatusCode::CONFLICT,
        "انتهت مهلة هذا الرفع. حاول مرة أخرى.",
    )
    .await?;
    app.stop().await
}

#[tokio::test]
async fn reserving_again_from_the_same_attempt_replaces_an_unused_reservation() -> anyhow::Result<()>
{
    let app = TestApp::start().await?;
    let lost = app.reserve().await?;
    let retry = app.reserve().await?;
    assert_ne!(lost, retry);
    assert_error(
        app.upload(&lost, "درس.mp3", MP3, None).await?,
        StatusCode::CONFLICT,
        "انتهت مهلة هذا الرفع. حاول مرة أخرى.",
    )
    .await?;
    json_reply(
        app.upload(&retry, "درس.mp3", MP3, None).await?,
        StatusCode::OK,
    )
    .await?;
    app.stop().await
}

// Another tab or browser on the same connection must not take over a reservation whose
// upload is still on its way, e.g. held by a proxy until it is complete.
#[tokio::test]
async fn another_attempt_from_the_same_ip_cannot_replace_a_reservation() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    let upload = app.reserve().await?;
    for form in [
        vec![("attempt", "0123456789abcdef0123456789abcdef")],
        vec![],
    ] {
        let response = app
            .client
            .post(app.url("/transcribe/jobs"))
            .form(&form)
            .send()
            .await?;
        assert_error(
            response,
            StatusCode::TOO_MANY_REQUESTS,
            "يوجد تفريغ قيد التنفيذ من اتصالك بالإنترنت. انتظر حتى ينتهي، ثم ارفع ملفًا آخر.",
        )
        .await?;
    }
    json_reply(
        app.upload(&upload, "درس.mp3", MP3, None).await?,
        StatusCode::OK,
    )
    .await?;
    app.stop().await
}

#[tokio::test]
async fn reserving_while_the_same_ip_uploads_returns_too_many_requests() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    let (_id, upload) = app.hold_upload();
    // A separate connection with no cookies must still count as the same client IP.
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()?;
    let response = client.post(app.url("/transcribe/jobs")).send().await?;
    assert_error(
        response,
        StatusCode::TOO_MANY_REQUESTS,
        "يوجد تفريغ قيد التنفيذ من اتصالك بالإنترنت. انتظر حتى ينتهي، ثم ارفع ملفًا آخر.",
    )
    .await?;
    drop(upload);
    app.stop().await
}

#[tokio::test]
async fn accepted_upload_links_only_to_the_browsers_unfinished_job() -> anyhow::Result<()> {
    let app = TestApp::start_with_limits(Limits {
        running: 0,
        unwatched: Duration::ZERO,
        ..limits()
    })
    .await?;
    let response = app.client.post(app.url("/transcribe/jobs")).send().await?;
    assert!(!response.headers().contains_key("set-cookie"));
    let reply = json_reply(response, StatusCode::OK).await?;
    let upload = reply["upload"].as_str().context("missing upload URL")?;
    let response = app.upload(upload, "speech.mp3", MP3, None).await?;
    let cookie = response.headers()["set-cookie"].to_str()?.to_owned();
    let reply = json_reply(response, StatusCode::OK).await?;
    let job = reply["next"].as_str().context("missing job URL")?;
    let id = job.rsplit('/').next().unwrap();
    assert!(
        cookie.starts_with(&format!("transcription_job={id};")),
        "{cookie}"
    );
    for attribute in ["HttpOnly", "SameSite=Lax", "Path=/", "Max-Age=14400"] {
        assert!(cookie.split("; ").any(|part| part == attribute), "{cookie}");
    }

    assert_private_return_links(&app, job).await?;
    let page = app.client.get(app.url(job)).send().await?.text().await?;
    assert!(
        page.contains("id=\"job-status\" class=\"status\" role=\"status\""),
        "{page}"
    );
    assert!(page.contains("Your file is in the queue"), "{page}");

    // Once the cookie's job is cancelled, another visitor can reserve on this IP.
    // The old cookie must not reveal that visitor's job.
    app.transcription.queue.sweep();
    let page = app.client.get(app.url(job)).send().await?.text().await?;
    assert!(
        page.contains("class=\"status error\" role=\"status\""),
        "{page}"
    );
    assert!(page.contains("The transcription was cancelled"), "{page}");
    let _busy = app.hold_upload();
    let page = app
        .client
        .get(app.url("/transcribe"))
        .send()
        .await?
        .text()
        .await?;
    assert!(
        page.contains("A transcription from your connection"),
        "{page}"
    );
    assert!(!page.contains("Back to your transcription"), "{page}");
    let reply = json_reply(
        app.client.post(app.url("/transcribe/jobs")).send().await?,
        StatusCode::TOO_MANY_REQUESTS,
    )
    .await?;
    assert!(reply.get("job").is_none(), "{reply}");
    app.stop().await
}

async fn assert_private_return_links(app: &TestApp, job: &str) -> anyhow::Result<()> {
    let visitor = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()?;
    for (language, message, label) in [
        (
            "ar",
            "يوجد تفريغ قيد التنفيذ من اتصالك بالإنترنت. انتظر حتى ينتهي، ثم ارفع ملفًا آخر.",
            "العودة إلى تفريغك",
        ),
        (
            "en",
            "A transcription from your connection is already in progress. Wait for it to finish, then upload another file.",
            "Back to your transcription",
        ),
    ] {
        let page = app
            .client
            .post(app.url("/language"))
            .form(&[("lang", language), ("back", "/transcribe")])
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        assert!(page.contains(message), "{page}");
        assert!(page.contains(&format!("href=\"{job}\"")), "{page}");
        assert!(page.contains(label), "{page}");
        let reply = json_reply(
            app.client.post(app.url("/transcribe/jobs")).send().await?,
            StatusCode::TOO_MANY_REQUESTS,
        )
        .await?;
        assert_eq!(reply, json!({"error": message, "job": job}));

        // Sharing the IP gives no access to the cookie owner's link. Bad and stale
        // cookie values must not fall back to a job found by IP either.
        for job_cookie in [
            "",
            "; transcription_job=invalid",
            "; transcription_job=00000000000000000000000000000000",
        ] {
            let cookie = format!("lang={language}{job_cookie}");
            let page = visitor
                .get(app.url("/transcribe"))
                .header("cookie", &cookie)
                .send()
                .await?
                .text()
                .await?;
            assert!(page.contains(message), "{page}");
            assert!(!page.contains(job), "{page}");
            assert!(!page.contains(label), "{page}");
            assert_error(
                visitor
                    .post(app.url("/transcribe/jobs"))
                    .header("cookie", &cookie)
                    .send()
                    .await?,
                StatusCode::TOO_MANY_REQUESTS,
                message,
            )
            .await?;
        }
    }

    Ok(())
}

#[tokio::test]
async fn return_link_follows_the_cookie_across_connections_only_when_refused() -> anyhow::Result<()>
{
    let app = TestApp::start().await?;
    let id = app
        .transcription
        .queue
        .reserve(Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1))), None)
        .expect("queue should accept another connection");
    let cookie = format!("transcription_job={id}");
    let page = app
        .client
        .get(app.url("/transcribe"))
        .header("cookie", &cookie)
        .send()
        .await?
        .text()
        .await?;
    assert!(page.contains("id=\"upload-form\""), "{page}");
    assert!(!page.contains(&id.to_string()), "{page}");

    let (other, _busy) = app.hold_upload();
    let page = app
        .client
        .get(app.url("/transcribe"))
        .header("cookie", &cookie)
        .send()
        .await?
        .text()
        .await?;
    assert!(
        page.contains(&format!("href=\"/transcribe/jobs/{id}\"")),
        "{page}"
    );
    assert!(!page.contains(&other.to_string()), "{page}");
    app.stop().await
}

#[tokio::test]
async fn running_status_separates_progress_and_failed_jobs_have_no_return_link()
-> anyhow::Result<()> {
    let app = TestApp::start().await?;
    let upload = app.reserve().await?;
    let job = upload.strip_suffix("/audio").unwrap();
    let id: JobId = job.rsplit('/').next().unwrap().parse()?;
    let (finish, finished) = tokio::sync::oneshot::channel::<()>();
    app.transcription
        .queue
        .begin_upload(&id)
        .expect("reservation should accept an upload")
        .submit(move |progress| async move {
            progress.set(37);
            let _ = finished.await;
            anyhow::bail!("test failure");
        })
        .expect("queue should accept the job");
    let mut watch = app.transcription.queue.watch(&id).context("missing job")?;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !matches!(watch.status(), Status::Running { percent: 37 }) {
            assert!(watch.changed().await);
        }
    })
    .await?;
    for (language, message, percent) in [
        ("ar", "جارٍ التفريغ…", "37٪"),
        ("en", "Transcribing…", "37%"),
    ] {
        let page = app
            .client
            .get(app.url(job))
            .header("cookie", format!("lang={language}"))
            .send()
            .await?
            .text()
            .await?;
        assert!(
            page.contains(&format!(
                "<p id=\"job-status\" class=\"status\" role=\"status\">{message}</p>"
            )),
            "{page}"
        );
        assert!(
            page.contains(&format!("<p class=\"percent\">{percent}</p>")),
            "{page}"
        );
        assert!(page.contains("aria-labelledby=\"job-status\""), "{page}");
    }
    finish.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !watch.status().is_finished() {
            assert!(watch.changed().await);
        }
    })
    .await?;
    let page = app.client.get(app.url(job)).send().await?.text().await?;
    assert!(
        page.contains("class=\"status error\" role=\"status\""),
        "{page}"
    );
    assert!(page.contains("تعذّر إكمال التفريغ"), "{page}");
    let _busy = app.hold_upload();
    let page = app
        .client
        .get(app.url("/transcribe"))
        .header("cookie", format!("transcription_job={id}"))
        .send()
        .await?
        .text()
        .await?;
    assert!(!page.contains("العودة إلى تفريغك"), "{page}");
    app.stop().await
}

#[tokio::test]
async fn a_full_queue_returns_service_unavailable_with_a_json_error() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    for client in 1..=100 {
        app.transcription
            .queue
            .reserve(Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, client))), None)
            .expect("queue should have space");
    }
    let response = app.client.post(app.url("/transcribe/jobs")).send().await?;
    assert_error(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "الطابور ممتلئ الآن. حاول مرة أخرى بعد بضع دقائق.",
    )
    .await?;
    app.stop().await
}

#[tokio::test]
async fn a_closed_queue_returns_service_unavailable_with_a_json_error() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    app.transcription.queue.shutdown(Duration::ZERO).await;
    let response = app.client.post(app.url("/transcribe/jobs")).send().await?;
    assert_error(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "نحدّث الموقع الآن. حاول مرة أخرى بعد بضع دقائق.",
    )
    .await?;
    app.stop().await
}

#[tokio::test]
async fn home_and_upload_pages_default_to_arabic() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    for (path, expected) in [
        ("/", "أدوات مجانية للغة العربية"),
        ("/transcribe", "ابدأ التفريغ"),
    ] {
        let response = app.client.get(app.url(path)).send().await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().await?;
        assert!(body.contains("<html lang=\"ar\" dir=\"rtl\">"), "{body}");
        assert!(body.contains(expected), "{body}");
        if path == "/transcribe" {
            assert!(body.contains("aria-label=\"جارٍ رفع الملف…\""), "{body}");
        }
    }
    app.stop().await
}

#[tokio::test]
async fn switching_language_sets_a_cookie_and_renders_pages_in_english() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    let response = app
        .client
        .post(app.url("/language"))
        .form(&[("lang", "en"), ("back", "/transcribe")])
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.url().path(), "/transcribe");
    assert!(
        response
            .text()
            .await?
            .contains("<html lang=\"en\" dir=\"ltr\">")
    );
    for (path, expected) in [
        ("/", "Free tools for the Arabic language"),
        ("/transcribe", "Start transcription"),
    ] {
        let body = app
            .client
            .get(app.url(path))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        assert!(body.contains("<html lang=\"en\" dir=\"ltr\">"), "{body}");
        assert!(body.contains(expected), "{body}");
        if path == "/transcribe" {
            assert!(body.contains("aria-label=\"Uploading…\""), "{body}");
        }
    }
    app.stop().await
}

#[tokio::test]
async fn unknown_pages_return_an_arabic_not_found_message() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    assert_missing_page(
        &app,
        "ar",
        "rtl",
        "هذه الصفحة غير موجودة، أو انتهت صلاحيتها.",
    )
    .await?;
    app.stop().await
}

#[tokio::test]
async fn unknown_pages_return_an_english_not_found_message_after_switching() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    app.client
        .post(app.url("/language"))
        .form(&[("lang", "en"), ("back", "/")])
        .send()
        .await?
        .error_for_status()?;
    assert_missing_page(
        &app,
        "en",
        "ltr",
        "This page doesn't exist, or it has expired.",
    )
    .await?;
    app.stop().await
}

async fn assert_missing_page(
    app: &TestApp,
    language: &str,
    direction: &str,
    message: &str,
) -> anyhow::Result<()> {
    let response = app.client.get(app.url("/does-not-exist")).send().await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.text().await?;
    assert!(
        body.contains(&format!("<html lang=\"{language}\" dir=\"{direction}\">")),
        "{body}"
    );
    assert!(body.contains(message), "{body}");
    Ok(())
}

#[tokio::test]
async fn downloads_of_unknown_jobs_return_not_found() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    for job in [UNKNOWN_JOB, "/transcribe/jobs/not-a-job-id"] {
        assert_missing_downloads(&app, job).await?;
    }
    app.stop().await
}

#[tokio::test]
async fn downloads_of_an_unfinished_job_return_not_found() -> anyhow::Result<()> {
    let app = TestApp::start().await?;
    let upload = app.reserve().await?;
    assert_missing_downloads(&app, upload.strip_suffix("/audio").unwrap()).await?;
    app.stop().await
}

async fn assert_missing_downloads(app: &TestApp, job: &str) -> anyhow::Result<()> {
    for extension in ["txt", "srt"] {
        let response = app
            .client
            .get(app.url(&format!("{job}/transcript.{extension}")))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(!response.headers().contains_key("content-disposition"));
    }
    Ok(())
}
