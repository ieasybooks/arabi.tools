//! One OCR job: its upload, its live status page and its downloads.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use topcoat::{
    Result,
    asset::{Asset, asset, asset_config},
    context::{Cx, app_context},
    cookie::{Cookies, cookie, cookies, time},
    icon::icon,
    router::{
        StatusCode,
        content::{
            Json,
            multipart::{Field, Multipart},
        },
        error::{RouterErrorExt, not_found},
        href, module_param, page, path_param, route,
    },
    runtime::{connected, shard},
    view::{View, ViewExt, component, emit, live, view},
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

const REVIEW_SCRIPT: Asset = asset!("../../ocr-review.js");
const PDF_MODULE: Asset = asset!("../../vendor/pdfjs/pdf.mjs");
const PDF_WORKER: Asset = asset!("../../vendor/pdfjs/pdf.worker.mjs");
include!("../../vendor/pdfjs/assets.rs");

module_param!(pub id: JobId, error = not_found);

/// How often an idle status page is sent again, so proxies keep its connection open.
const HEARTBEAT: Duration = Duration::from_secs(30);

#[page]
pub(super) async fn page(cx: &Cx) -> Result<impl View> {
    let id = path_param::<Id>(cx)?.clone();
    let locale = Locale::of(cx);
    crate::app::workspace::private_response(cx);
    let exists = app_context::<Ocr>(cx).queue.watch(&id).is_some();
    Ok(view! {
        <script src=(REVIEW_SCRIPT) defer=(true)></script>
        ocr::tool_hero(locale: locale, lead: false)
        <section class="workspace">
            <div class="container workspace-layout workspace-wide">
                if exists {
                    (live! {
                        let mut watch = app_context::<Ocr>(cx)
                            .queue
                            .watch(&id)
                            .ok_or_not_found()?;
                        loop {
                            let status = watch.status();
                            let finished = status.is_finished();
                            let token = emit! {
                                job_status(
                                    locale: locale,
                                    id: id.clone(),
                                    status: status,
                                    embedded: false
                                )
                            }?;
                            if finished || !connected(cx) {
                                drop(watch);
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
                } else {
                    (StatusCode::NOT_FOUND)
                    <p class="notice">(locale.text().workspace.missing)</p>
                    <a class="button secondary" href=(href!(ocr::page))>
                        (locale.text().ocr.another)
                    </a>
                }
            </div>
        </section>
        <script src=(crate::app::workspace::SCRIPT) defer=(true)></script>
        // Integration: load the ocr viewer scripts here, on the direct job page too.
    })
}

/// A job link is the existing read capability. The shard observes it without starting another job.
#[shard("/ocr/observe")]
pub(crate) async fn observe(cx: &Cx, job: String) -> Result<impl View> {
    crate::app::workspace::private_response(cx);
    let locale = Locale::of(cx);
    let id = if job.is_empty() {
        None
    } else {
        Some(job.parse::<JobId>().map_err(|_| not_found())?)
    };
    Ok(view! {
        cx =>
        if let Some(id) = id {
            (live! {
                let Some(mut watch) = app_context::<Ocr>(cx).queue.watch(&id) else {
                    return emit! {
                        <div data-job-id=(id.to_string()) data-phase="missing">
                            <p class="status">(locale.text().workspace.missing)</p>
                        </div>
                    };
                };
                loop {
                    let status = watch.status();
                    let finished = status.is_finished();
                    let token = emit! {
                        job_status(
                            locale: locale,
                            id: id.clone(),
                            status: status,
                            embedded: true
                        )
                    }?;
                    if finished || !connected(cx) {
                        drop(watch);
                        break Ok(token);
                    }
                    tokio::select! {
                        changed = watch.changed() => if !changed { break Ok(token); },
                        () = tokio::time::sleep(HEARTBEAT) => {}
                    }
                }
            })
        }
    }
    .boxed())
}

fn status_message(locale: Locale, status: &Status<Document>) -> String {
    let text = &locale.text().ocr;
    match status {
        Status::Reserved => text.reserved.to_owned(),
        Status::Waiting { position } => locale.waiting(*position),
        Status::Running { .. } => text.running.to_owned(),
        Status::Done(_) => text.done.to_owned(),
        Status::Failed => text.failed.to_owned(),
        Status::Cancelled(CancelReason::User) => locale.text().cancelled_user.to_owned(),
        Status::Cancelled(CancelReason::Unwatched) => text.cancelled_unwatched.to_owned(),
        Status::Cancelled(CancelReason::Shutdown) => text.cancelled_shutdown.to_owned(),
    }
}

#[component]
async fn job_status(
    locale: Locale,
    id: JobId,
    status: Status<Document>,
    embedded: bool,
) -> Result<impl View> {
    let text = &locale.text().ocr;
    let finished = status.is_finished();
    let message = status_message(locale, &status);
    let phase = match &status {
        Status::Reserved => "reserved",
        Status::Waiting { .. } => "queued",
        Status::Running { .. } => "processing",
        Status::Done(_) => "completed",
        Status::Failed | Status::Cancelled(_) => "failed",
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
        <div
            class="job"
            data-state=(state)
            data-job-id=(id.to_string())
            data-phase=(phase)
            data-update=(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis())
        >
            <div class="job-heading">
                if let Some(state_icon) = state_icon {
                    <span class="job-icon">icon(data: state_icon, size: 20)</span>
                }
                // Topcoat preserves this node across live updates. Only status text belongs
                // here, so percentage updates do not repeat the polite announcement.
                <p
                    id=((!embedded).then_some("job-status"))
                    class="status"
                    role=((!embedded).then_some("status"))
                >
                    (message)
                </p>
                if let Some(percent) = percent {
                    <p class="percent">(locale.percent(percent))</p>
                }
            </div>
            if let Some(percent) = percent {
                <progress max="100" value=(percent) aria-labelledby="job-status"></progress>
            }
            if let Some((done, total)) = pages {
                <p class="hint page-progress">(locale.pages_done(done, total))</p>
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

/// A finished job's downloads and locally enhanced document review.
#[component]
async fn result(cx: &Cx, locale: Locale, id: JobId, document: Arc<Document>) -> Result<impl View> {
    let text = &locale.text().ocr;
    let review = &text.review;
    let labels = serde_json::to_string(review)?;
    let resources = PDF_RESOURCES
        .iter()
        .map(|(name, asset)| (*name, asset_config(cx).resolve(*asset)))
        .collect::<std::collections::BTreeMap<_, _>>();
    let resources = serde_json::to_string(&resources)?;
    let all_blank = document
        .pages
        .iter()
        .all(|content| content.trim().is_empty());
    Ok(view! {
        <div
            class="result ocr-review"
            data-result="ocr"
            data-job-id=(id.to_string())
            data-labels=(labels)
            data-pdf-module=(PDF_MODULE)
            data-pdf-worker=(PDF_WORKER)
            data-pdf-resources=(resources)
            data-wrong-type=(text.wrong_type)
            data-no-text=(text.no_text)
        >
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
            if all_blank {
                <p class="ocr-empty-document">(review.all_blank)</p>
            }
            // Integration slot for the shared search_toolbar(locale) component.
            <div data-ocr-search-slot=""></div>
            review_controls(locale: locale, id: id.clone())
            <div class="ocr-panels">
                source_panel(locale: locale, id: id.clone())
                text_panel(locale: locale, document: document.clone())
            </div>
        </div>
    })
}

#[component]
async fn text_panel(locale: Locale, document: Arc<Document>) -> Result<impl View> {
    let text = &locale.text().ocr;
    let review = &text.review;
    Ok(view! {
        <section class="ocr-text-panel">
            <h2>(review.text_title)</h2>
            <div
                class="ocr-text-viewport"
                data-ocr-text-viewport=""
                role="region"
                aria-label=(review.text_title)
                tabindex="0"
            >
                <ol
                    class="ocr-pages"
                    data-ocr-pages=""
                    role="list"
                    aria-label=(text.preview_title)
                >
                    for (number, content) in (1..).zip(&document.pages) {
                        <li class="ocr-page">
                            <h3>(locale.page_number(number))</h3>
                            <p class="ocr-page-text" data-ocr-page-text="" dir="auto">
                                (content.as_str())
                            </p>
                            if content.trim().is_empty() {
                                <p class="ocr-no-text">(text.no_text)</p>
                            }
                        </li>
                    }
                </ol>
            </div>
        </section>
    })
}

#[component]
async fn review_controls(locale: Locale, id: JobId) -> Result<impl View> {
    let review = &locale.text().ocr.review;
    let page_error_id = format!("ocr-page-error-{id}");
    Ok(view! {
        <div class="ocr-toolbar" data-ocr-controls="" hidden=(true)>
            <form class="ocr-page-navigation" data-ocr-navigation="" novalidate=(true)>
                <button class="button secondary" type="button" data-ocr-previous="">
                    (review.previous_page)
                </button>
                <label class="ocr-page-field">
                    <span>(review.page_label)</span>
                    <input
                        type="text"
                        inputmode="numeric"
                        dir="ltr"
                        value="1"
                        data-ocr-page-number=""
                        aria-describedby=(page_error_id.as_str())
                        autocomplete="off"
                    />
                </label>
                <button class="button secondary" type="button" data-ocr-next="">
                    (review.next_page)
                </button>
                <span
                    class="ocr-page-position"
                    data-ocr-position=""
                    aria-live="polite"
                    aria-atomic="true"
                ></span>
            </form>
            <p
                class="error"
                id=(page_error_id.as_str())
                data-ocr-page-error=""
                aria-live="polite"
                hidden=(true)
            ></p>
            <div class="ocr-copy-actions">
                <button class="button secondary" type="button" data-ocr-copy-page="">
                    (review.copy_page)
                </button>
                <button class="button secondary" type="button" data-ocr-copy-all="">
                    (review.copy_all)
                </button>
            </div>
            <p class="ocr-copy-status" data-ocr-copy-status="" aria-live="polite"></p>
            <div data-ocr-copy-fallback=""></div>
            <fieldset class="ocr-view-choice">
                <legend>(review.view_label)</legend>
                <label>
                    <input
                        type="radio"
                        name=(format!("ocr-view-{id}"))
                        value="source"
                    />
                    <span>(review.source_view)</span>
                </label>
                <label>
                    <input
                        type="radio"
                        name=(format!("ocr-view-{id}"))
                        value="text"
                        checked=(true)
                    />
                    <span>(review.text_view)</span>
                </label>
            </fieldset>
        </div>
    })
}

#[component]
async fn source_panel(locale: Locale, id: JobId) -> Result<impl View> {
    let review = &locale.text().ocr.review;
    Ok(view! {
        <section class="ocr-source-panel">
            <h2>(review.source_title)</h2>
            <p class="ocr-source-status" data-ocr-source-status="" aria-live="polite">
                (review.source_missing)
            </p>
            source_picker(locale: locale, id: id)
            <div class="ocr-zoom" data-ocr-zoom="" hidden=(true)>
                <button
                    class="button secondary"
                    type="button"
                    data-ocr-fit="width"
                    aria-pressed="true"
                >
                    (review.fit_width)
                </button>
                <button
                    class="button secondary"
                    type="button"
                    data-ocr-fit="page"
                    aria-pressed="false"
                >
                    (review.fit_page)
                </button>
                <button class="button secondary" type="button" data-ocr-zoom-out="">
                    (review.zoom_out)
                </button>
                <span data-ocr-zoom-value="" aria-label=(review.zoom_label) dir="ltr"></span>
                <button class="button secondary" type="button" data-ocr-zoom-in="">
                    (review.zoom_in)
                </button>
            </div>
            <div
                class="ocr-source-viewport"
                data-ocr-source-viewport=""
                role="region"
                aria-label=(review.source_title)
                tabindex="0"
                hidden=(true)
            ></div>
        </section>
    })
}

#[component]
async fn source_picker(locale: Locale, id: JobId) -> Result<impl View> {
    let review = &locale.text().ocr.review;
    let source_error_id = format!("ocr-source-error-{id}");
    Ok(view! {
        <form
            class="ocr-source-picker"
            data-ocr-source-picker=""
            hidden=(true)
            novalidate=(true)
        >
            <label>
                <span>(review.source_file)</span>
                <input
                    type="file"
                    accept=".pdf,.jpg,.jpeg,.png"
                    data-ocr-source-file=""
                    aria-describedby=(source_error_id.as_str())
                />
            </label>
            <label class="ocr-confirm">
                <input
                    type="checkbox"
                    data-ocr-confirm=""
                    aria-describedby=(source_error_id.as_str())
                />
                <span>(review.source_confirm)</span>
            </label>
            <button class="button secondary" type="submit">
                (review.source_attach)
            </button>
            <p
                class="error"
                id=(source_error_id.as_str())
                data-ocr-source-error=""
                aria-live="polite"
                hidden=(true)
            ></p>
        </form>
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
                        job_id: id.to_string(),
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
