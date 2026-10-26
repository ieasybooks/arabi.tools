//! One OCR job: its upload, its live status page and its downloads.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use topcoat::{
    Result,
    context::{Cx, app_context},
    cookie::{Cookies, cookie, cookies, time},
    icon::icon,
    router::{
        StatusCode,
        content::{
            Json,
            multipart::{Field, Multipart},
        },
        error::RouterErrorExt,
        href, module_param, page, path_param, route,
    },
    runtime::connected,
    view::{View, component, emit, live, view},
};

use super::{COOKIE, Reply, rejected, unavailable};
use crate::app::download::Download;
use crate::app::ocr;
use crate::i18n::Locale;
use crate::icons;
use crate::queue::{CancelReason, JobId, Status};
use crate::tools::ocr::{
    Document, DocumentProblem, MAX_UPLOAD_BYTES, Ocr, UploadedDocument, download_name,
};

module_param!(pub id: JobId, error = not_found);

/// How often an idle status page is sent again, so proxies keep its connection open.
const HEARTBEAT: Duration = Duration::from_secs(30);

#[page]
pub(super) async fn page(cx: &Cx) -> Result<impl View> {
    let id = path_param::<Id>(cx)?.clone();
    let locale = Locale::of(cx);
    let mut watch = app_context::<Ocr>(cx).queue.watch(&id).ok_or_not_found()?;
    Ok(view! {
        ocr::tool_hero(locale: locale, lead: false)
        <section class="workspace">
            <div class="container workspace-layout">
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
            </div>
        </section>
    })
}

#[component]
async fn job_status(locale: Locale, id: JobId, status: Status<Document>) -> Result<impl View> {
    let text = &locale.text().ocr;
    let finished = status.is_finished();
    let message = match &status {
        Status::Reserved => text.reserved.to_owned(),
        Status::Waiting { position } => locale.waiting(*position),
        Status::Running { .. } => text.running.to_owned(),
        Status::Done(_) => text.done.to_owned(),
        Status::Failed => text.failed.to_owned(),
        Status::Cancelled(CancelReason::User) => locale.text().cancelled_user.to_owned(),
        Status::Cancelled(CancelReason::Unwatched) => text.cancelled_unwatched.to_owned(),
        Status::Cancelled(CancelReason::Shutdown) => text.cancelled_shutdown.to_owned(),
    };
    let (state, state_icon) = match &status {
        Status::Reserved | Status::Waiting { .. } => ("waiting", Some(icons::WAITING)),
        Status::Running { .. } => ("running", None),
        Status::Done(_) => ("done", Some(icons::DONE)),
        Status::Failed | Status::Cancelled(_) => ("failed", Some(icons::FAILED)),
    };
    let step = if state == "done" {
        ocr::Step::Download
    } else {
        ocr::Step::Recognize
    };
    let pages = match &status {
        Status::Running { pages, .. } => *pages,
        _ => None,
    };
    let percent = match status {
        Status::Running { percent, .. } => Some(percent),
        _ => None,
    };
    let keep_open = matches!(status, Status::Waiting { .. } | Status::Running { .. });
    let document = match status {
        Status::Done(document) => Some(document),
        _ => None,
    };
    Ok(view! {
        ocr::steps(locale: locale, current: step, failed: state == "failed")
        <div class="job" data-state=(state)>
            <div class="job-heading">
                if let Some(state_icon) = state_icon {
                    <span class="job-icon">icon(data: state_icon, size: 20)</span>
                }
                // Topcoat preserves this node across live updates. Only status text belongs
                // here, so percentage updates do not repeat the polite announcement.
                <p id="job-status" class="status" role="status">(message)</p>
                if let Some(percent) = percent {
                    <p class="percent">(locale.percent(percent))</p>
                }
            </div>
            if let Some(percent) = percent {
                <progress max="100" value=(percent) aria-labelledby="job-status"></progress>
            }
            if let Some((done, total)) = pages {
                <p class="hint">(locale.pages_done(done, total))</p>
            }
            if keep_open {
                <p class="keep-open">
                    icon(data: icons::WARNING, size: 20)
                    <span>(text.keep_open)</span>
                </p>
            }
            if let Some(document) = document {
                result(locale: locale, id: id, document: document)
            }
        </div>
        if finished {
            <p class="another">
                <a
                    class=(if state == "done" {
                        "button secondary"
                    } else {
                        "button primary"
                    })
                    href=(href!(ocr::page))
                >
                    (text.another)
                </a>
            </p>
        }
    })
}

/// A finished job's downloads and document.
#[component]
async fn result(locale: Locale, id: JobId, document: Arc<Document>) -> Result<impl View> {
    let text = &locale.text().ocr;
    Ok(view! {
        <div class="result">
            <div class="result-heading">
                <p class="result-name">
                    icon(data: icons::TRANSCRIPT, size: 20)
                    <span dir="auto">(document.name.as_str())</span>
                </p>
                <div class="downloads ocr-downloads">
                    <a
                        class="button primary"
                        href=(href!(download_text, Id(id.clone())))
                        download=(true)
                    >
                        icon(data: icons::DOWNLOAD, size: 16)
                        <span>(text.download_text)</span>
                    </a>
                    <a
                        class="button secondary"
                        href=(href!(download_docx, Id(id.clone())))
                        download=(true)
                    >
                        icon(data: icons::DOWNLOAD, size: 16)
                        <span>(text.download_docx)</span>
                    </a>
                    <a
                        class="button secondary"
                        href=(href!(download_json, Id(id.clone())))
                        download=(true)
                    >
                        icon(data: icons::DOWNLOAD, size: 16)
                        <span>(text.download_json)</span>
                    </a>
                </div>
            </div>
            <p class="hint">(text.kept_for)</p>
            <h2 class="transcript-title">(text.preview_title)</h2>
            <ol class="cues ocr-pages" role="list">
                for (number, content) in (1..).zip(&document.pages) {
                    <li class="cue">
                        <h3 class="cue-time">(locale.page_number(number))</h3>
                        <p class="cue-text" dir="auto">
                            if content.trim().is_empty() {
                                (text.no_text)
                            } else {
                                (content.as_str())
                            }
                        </p>
                    </li>
                }
            </ol>
        </div>
    })
}

/// Receives one file, validates its contents and page count, then queues the job.
#[route(POST "./document")]
pub(super) async fn upload(cx: &Cx, mut form: Multipart) -> Result<(StatusCode, Json<Reply>)> {
    let locale = Locale::of(cx);
    let ocr = app_context::<Ocr>(cx);
    if !ocr.is_available() {
        return Ok(unavailable(cx));
    }
    let id = path_param::<Id>(cx)?;
    let reservation = match ocr.queue.begin_upload(id) {
        Ok(upload) => upload,
        Err(rejection) => return Ok(rejected(cx, rejection)),
    };
    let refused = |problem| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(Reply::Error {
                error: locale.document_problem(problem),
                job: None,
            }),
        )
    };
    let (upload, file_name) = match receive(&mut form).await? {
        Ok(received) => received,
        Err(problem) => return Ok(refused(problem)),
    };
    let upload = match ocr.validate(upload).await {
        Ok(upload) => upload,
        Err(problem) => return Ok(refused(problem)),
    };
    Ok(
        match ocr.submit(reservation, upload, download_name(&file_name)) {
            Ok(()) => {
                cookies(cx).add(cookie! {
                    COOKIE = id.to_string();
                    HttpOnly;
                    SameSite = Lax;
                    Path = "/";
                    MaxAge = time::Duration::hours(4)
                });
                (
                    StatusCode::OK,
                    Json(Reply::Next {
                        next: href!(page, Id(id.clone())).resolve(cx),
                    }),
                )
            }
            Err(rejection) => rejected(cx, rejection),
        },
    )
}

async fn receive(
    form: &mut Multipart,
) -> Result<Result<(UploadedDocument, String), DocumentProblem>> {
    let mut received = None;
    while let Some(mut field) = form.next_field().await? {
        if field.name() != Some("file") {
            continue;
        }
        if received.is_some() {
            return Ok(Err(DocumentProblem::WrongType));
        }
        let name = field.file_name().unwrap_or_default().to_owned();
        let document = match UploadedDocument::new(&name) {
            Ok(document) => document,
            Err(problem) => return Ok(Err(problem)),
        };
        if !save(&mut field, &document.path).await? {
            return Ok(Err(DocumentProblem::TooLarge));
        }
        received = Some((document, name));
    }
    Ok(received.ok_or(DocumentProblem::Missing))
}

async fn save(field: &mut Field<'_>, path: &std::path::Path) -> Result<bool> {
    let mut file = tokio::fs::File::create(path).await?;
    let mut size = 0;
    while let Some(chunk) = field.chunk().await? {
        size += chunk.len() as u64;
        if size > MAX_UPLOAD_BYTES {
            return Ok(false);
        }
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    Ok(true)
}

#[route(GET "./document.txt")]
async fn download_text(cx: &Cx) -> Result<Download> {
    let document = finished(cx)?;
    Ok(Download {
        fallback_stem: "document",
        file_name: format!("{}.txt", document.name),
        content_type: "text/plain; charset=utf-8",
        body: document.text.clone(),
    })
}

#[route(GET "./document.docx")]
async fn download_docx(cx: &Cx) -> Result<Download> {
    let document = finished(cx)?;
    Ok(Download {
        fallback_stem: "document",
        file_name: format!("{}.docx", document.name),
        content_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        body: document.docx.clone(),
    })
}

#[route(GET "./document.json")]
async fn download_json(cx: &Cx) -> Result<Download> {
    let document = finished(cx)?;
    Ok(Download {
        fallback_stem: "document",
        file_name: format!("{}.json", document.name),
        content_type: "application/json; charset=utf-8",
        body: document.json.clone(),
    })
}

fn finished(cx: &Cx) -> Result<Arc<Document>> {
    let id = path_param::<Id>(cx)?;
    Ok(app_context::<Ocr>(cx).queue.result(id).ok_or_not_found()?)
}
