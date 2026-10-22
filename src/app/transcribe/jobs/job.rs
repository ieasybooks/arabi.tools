//! One transcription job: its upload, its live status page and its downloads.

use std::fmt::Write;
use std::time::Duration;

use tempfile::TempPath;
use tokio::io::AsyncWriteExt;
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{
        Body, StatusCode,
        content::{
            Json,
            multipart::{Field, Multipart},
        },
        error::RouterErrorExt,
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        href, module_param, page, path_param,
        request::client_ip,
        response::{IntoResponse, Response},
        route,
    },
    runtime::connected,
    view::{View, component, emit, live, view},
};

use super::{Reply, rejected};
use crate::app::transcribe;
use crate::i18n::Locale;
use crate::queue::{CancelReason, JobId, Status};
use crate::tools::transcription::{
    AudioProblem, MAX_UPLOAD_BYTES, Transcript, Transcription, check_audio, download_name,
    has_accepted_extension,
};

module_param!(pub id: JobId, error = not_found);

/// How often an idle status page is sent again, so proxies keep its connection open.
const HEARTBEAT: Duration = Duration::from_secs(30);

#[page]
async fn page(cx: &Cx) -> Result<impl View> {
    let id = path_param::<Id>(cx)?.clone();
    let locale = Locale::of(cx);
    let mut watch = app_context::<Transcription>(cx)
        .queue
        .watch(&id)
        .ok_or_not_found()?;
    Ok(view! {
        <h1>(locale.text().transcription.name)</h1>
        (live! {
            loop {
                let status = watch.status();
                let finished = status.is_finished();
                let token = emit! {
                    job_status(locale: locale, id: id.clone(), status: status)
                }?;
                if finished || !connected(cx) {
                    break Ok(token);
                }
                tokio::select! {
                    changed = watch.changed() => if !changed {
                        break Ok(token);
                    },
                    () = tokio::time::sleep(HEARTBEAT) => {}
                }
            }
        })
    })
}

#[component]
async fn job_status(locale: Locale, id: JobId, status: Status<Transcript>) -> Result<impl View> {
    let text = &locale.text().transcription;
    let finished = status.is_finished();
    Ok(view! {
        <div class="card job">
            match status {
                Status::Reserved => {
                    <p class="status">(text.reserved)</p>
                }
                Status::Waiting { position } => {
                    <p class="status">(locale.waiting(position))</p>
                    <p class="hint">(text.keep_open)</p>
                }
                Status::Running { percent } => {
                    <p class="status">(locale.running(percent))</p>
                    <progress max="100" value=(percent)></progress>
                    <p class="hint">(text.keep_open)</p>
                }
                Status::Done(transcript) => {
                    <p class="status success">(text.done)</p>
                    <div class="downloads">
                        <a
                            class="button"
                            href=(href!(download_text, Id(id.clone())))
                            download=(true)
                        >
                            (text.download_text)
                        </a>
                        <a
                            class="button secondary"
                            href=(href!(download_srt, Id(id.clone())))
                            download=(true)
                        >
                            (text.download_srt)
                        </a>
                    </div>
                    <p class="hint">(locale.kept_for())</p>
                    if transcript.text.trim().is_empty() {
                        <p>(text.no_speech)</p>
                    } else {
                        <div class="transcript" dir="auto">
                            for line in transcript.text.lines() {
                                <p>(line)</p>
                            }
                        </div>
                    }
                }
                Status::Failed => {
                    <p class="status error">(text.failed)</p>
                }
                Status::Cancelled(CancelReason::Unwatched) => {
                    <p class="status error">(text.cancelled_unwatched)</p>
                }
                Status::Cancelled(CancelReason::Shutdown) => {
                    <p class="status error">(text.cancelled_shutdown)</p>
                }
            }
        </div>
        if finished {
            <p><a href=(href!(transcribe::page))>(text.another)</a></p>
        }
    })
}

/// Receives the file for a reserved job and queues the job.
#[route(POST "./audio")]
pub(super) async fn upload(cx: &Cx, mut form: Multipart) -> Result<(StatusCode, Json<Reply>)> {
    let locale = Locale::of(cx);
    let transcription = app_context::<Transcription>(cx);
    let id = path_param::<Id>(cx)?;
    let upload = match transcription.queue.begin_upload(id, client_ip(cx)) {
        Ok(upload) => upload,
        Err(rejection) => return Ok(rejected(locale, rejection)),
    };
    let refused = |problem| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(Reply::Error(locale.audio_problem(problem))),
        )
    };
    let (audio, file_name, language) = match receive(&mut form).await? {
        Ok(received) => received,
        Err(problem) => return Ok(refused(problem)),
    };
    if let Err(problem) = check_audio(&audio).await {
        return Ok(refused(problem));
    }
    let Some(language) = transcription.language(language.as_deref()) else {
        return Err(topcoat::router::error::bad_request("unknown language").into());
    };
    Ok(
        match language.submit(upload, audio, download_name(&file_name)) {
            Ok(()) => (
                StatusCode::OK,
                Json(Reply::Next(href!(page, Id(id.clone())).resolve(cx))),
            ),
            Err(rejection) => rejected(locale, rejection),
        },
    )
}

/// Reads the upload form: the audio, saved to a temporary file, its name, and the chosen
/// language, if any.
async fn receive(
    form: &mut Multipart,
) -> Result<Result<(TempPath, String, Option<String>), AudioProblem>> {
    let mut language = None;
    let mut audio = None;
    while let Some(mut field) = form.next_field().await? {
        match field.name() {
            Some("language") => language = Some(field.text().await?),
            Some("file") => {
                let file_name = field.file_name().unwrap_or_default().to_owned();
                if !has_accepted_extension(&file_name) {
                    return Ok(Err(AudioProblem::WrongType));
                }
                let Some(path) = save(&mut field).await? else {
                    return Ok(Err(AudioProblem::TooLarge));
                };
                audio = Some((path, file_name));
            }
            _ => {}
        }
    }
    Ok(match audio {
        Some((path, file_name)) => Ok((path, file_name, language)),
        None => Err(AudioProblem::Missing),
    })
}

/// Saves `field` to a temporary file, or returns `None` once it passes the size limit.
async fn save(field: &mut Field<'_>) -> Result<Option<TempPath>> {
    let (file, path) = tempfile::Builder::new()
        .prefix("upload-")
        .tempfile()?
        .into_parts();
    let mut file = tokio::fs::File::from_std(file);
    let mut size = 0;
    while let Some(chunk) = field.chunk().await? {
        size += chunk.len() as u64;
        if size > MAX_UPLOAD_BYTES {
            return Ok(None);
        }
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    Ok(Some(path))
}

#[route(GET "./transcript.txt")]
async fn download_text(cx: &Cx) -> Result<Download> {
    let transcript = finished(cx)?;
    Ok(Download {
        file_name: format!("{}.txt", transcript.name),
        content_type: "text/plain; charset=utf-8",
        body: transcript.text.clone(),
    })
}

#[route(GET "./transcript.srt")]
async fn download_srt(cx: &Cx) -> Result<Download> {
    let transcript = finished(cx)?;
    Ok(Download {
        file_name: format!("{}.srt", transcript.name),
        content_type: "application/x-subrip; charset=utf-8",
        body: transcript.srt.clone(),
    })
}

fn finished(cx: &Cx) -> Result<std::sync::Arc<Transcript>> {
    let id = path_param::<Id>(cx)?;
    Ok(app_context::<Transcription>(cx)
        .queue
        .result(id)
        .ok_or_not_found()?)
}

/// A transcript file, saved under the uploaded file's name.
struct Download {
    file_name: String,
    content_type: &'static str,
    body: String,
}

impl IntoResponse for Download {
    fn into_response(self, _cx: &Cx) -> Result<Response> {
        Ok(Response::builder()
            .header(CONTENT_TYPE, self.content_type)
            .header(CONTENT_DISPOSITION, attachment(&self.file_name))
            .body(Body::from(self.body))?)
    }
}

/// A `Content-Disposition` value saving the response as `file_name`. Browsers use the
/// UTF-8 form (RFC 6266); the plain one is a fallback for names that are not ASCII.
fn attachment(file_name: &str) -> String {
    let fallback = if file_name.is_ascii() {
        file_name.to_owned()
    } else {
        let extension = file_name.rsplit('.').next().unwrap_or("txt");
        format!("transcript.{extension}")
    };
    let mut encoded = String::new();
    for byte in file_name.bytes() {
        if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use super::attachment;

    #[test]
    fn names_downloads_in_utf8_with_an_ascii_fallback() {
        assert_eq!(
            attachment("lesson 1.txt"),
            "attachment; filename=\"lesson 1.txt\"; filename*=UTF-8''lesson%201.txt"
        );
        assert_eq!(
            attachment("درس.srt"),
            "attachment; filename=\"transcript.srt\"; filename*=UTF-8''%D8%AF%D8%B1%D8%B3.srt"
        );
    }
}
