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
use crate::result_controls::{self, COPY_SCRIPT, CopyAll, DownloadLink, SEARCH_SCRIPT};
use crate::tools::ocr::{
    Document, DocumentProblem, MAX_UPLOAD_BYTES, Ocr, UploadedDocument, download_name,
};

const PDF_MODULE: Asset = asset!("../../vendor/pdfjs/pdf.min.mjs");
const PDF_WORKER: Asset = asset!("../../vendor/pdfjs/pdf.worker.min.mjs");
/// The image decoders PDF.js requests by filename for JBIG2 and JPEG 2000 scans.
const PDF_RESOURCES: &[(&str, Asset)] = &[
    ("jbig2.wasm", asset!("../../vendor/pdfjs/wasm/jbig2.wasm")),
    (
        "openjpeg.wasm",
        asset!("../../vendor/pdfjs/wasm/openjpeg.wasm"),
    ),
];

module_param!(pub id: JobId, error = not_found);

/// How often an idle status page is sent again, so proxies keep its connection open.
const HEARTBEAT: Duration = Duration::from_secs(30);

#[page]
pub(super) async fn page(cx: &Cx) -> Result<impl View> {
    let id = path_param::<Id>(cx)?.clone();
    let locale = Locale::of(cx);
    crate::workspace::private_response(cx);
    let exists = app_context::<Ocr>(cx).queue.watch(&id).is_some();
    Ok(view! {
        <script src=(COPY_SCRIPT) defer=(true)></script>
        <script src=(SEARCH_SCRIPT) defer=(true)></script>
        <script src=(ocr::REVIEW_SCRIPT) defer=(true)></script>
        ocr::tool_hero(locale: locale, lead: false)
        <section class="workspace">
            <div class="container workspace-layout">
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
        <script src=(crate::workspace::SCRIPT) defer=(true)></script>
    })
}

/// A job link is the existing read capability. The shard observes it without starting another job.
#[shard("/ocr/observe")]
pub(crate) async fn observe(cx: &Cx, job: String) -> Result<impl View> {
    crate::workspace::private_response(cx);
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
            } else if !embedded {
                // The job's own page has no input to carry the line before a result.
                crate::workspace::privacy_note(text: text.privacy.to_owned())
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

/// A finished job's downloads and locally enhanced document review, with the page's privacy line
/// at its foot.
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
    let downloads = vec![
        DownloadLink {
            label: text.download_text,
            href: href!(download_text, Id(id.clone())).resolve(cx),
        },
        DownloadLink {
            label: text.download_docx,
            href: href!(download_docx, Id(id.clone())).resolve(cx),
        },
        DownloadLink {
            label: text.download_json,
            href: href!(download_json, Id(id.clone())).resolve(cx),
        },
    ];
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
            result_controls::result_header(
                locale: locale,
                name: document.name.clone(),
                copy: Some(CopyAll::Ocr),
                downloads: downloads
            )
            if all_blank {
                <p class="ocr-empty-document">(review.all_blank)</p>
            }
            review_controls(locale: locale, id: id.clone(), pages: document.pages.len())
            result_controls::copy_feedback(locale: locale)
            <div class="ocr-panels">
                source_panel(locale: locale, id: id.clone())
                text_panel(locale: locale, document: document.clone())
            </div>
            crate::workspace::privacy_note(text: text.privacy.to_owned())
        </div>
    })
}

#[component]
async fn text_panel(locale: Locale, document: Arc<Document>) -> Result<impl View> {
    let text = &locale.text().ocr;
    let review = &text.review;
    Ok(view! {
        <section class="ocr-text-panel">
            <h2 class="visually-hidden">(review.text_title)</h2>
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
                            <h3 class="visually-hidden">
                                (locale.page_number(number))
                            </h3>
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

/// The viewer's one toolbar, over both columns: above the source, the Page/Text switch on narrow
/// screens, the page navigator, then fit and zoom; above the text, Copy this page and search.
#[component]
async fn review_controls(locale: Locale, id: JobId, pages: usize) -> Result<impl View> {
    let review = &locale.text().ocr.review;
    let page_error_id = format!("ocr-page-error-{id}");
    Ok(view! {
        <div class="result-toolbar ocr-toolbar" data-ocr-controls="" hidden=(true)>
            <div class="ocr-page-tools">
                page_controls(locale: locale, id: id.clone(), pages: pages)
                zoom_controls(locale: locale)
            </div>
            <div class="ocr-text-tools">
                <button
                    class="button ghost compact"
                    type="button"
                    data-ocr-copy-page=""
                >
                    icon(data: icons::COPY, size: 16)
                    <span>(review.copy_page)</span>
                </button>
                <div class="ocr-search" data-ocr-search-slot="">
                    result_controls::search_toolbar(
                        locale: locale,
                        label: locale.text().review.search_text
                    )
                </div>
            </div>
            <p
                class="error"
                id=(page_error_id.as_str())
                data-ocr-page-error=""
                aria-live="polite"
                hidden=(true)
            ></p>
        </div>
    })
}

/// The Page/Text switch on narrow screens, then the page navigator with its editable number.
#[component]
async fn page_controls(locale: Locale, id: JobId, pages: usize) -> Result<impl View> {
    let review = &locale.text().ocr.review;
    let page_error_id = format!("ocr-page-error-{id}");
    Ok(view! {
        <fieldset class="ocr-view-choice">
            <legend class="visually-hidden">(review.view_label)</legend>
            <label>
                <input type="radio" name=(format!("ocr-view-{id}")) value="source" />
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
        <form class="ocr-page-navigation" data-ocr-navigation="" novalidate=(true)>
            <button
                class="icon-button"
                type="button"
                data-ocr-previous=""
                aria-label=(review.previous_page)
                title=(review.previous_page)
            >
                <span class="forward">icon(data: icons::BACK, size: 16)</span>
            </button>
            <span class="ocr-page-count" dir="ltr">
                <label class="ocr-page-field">
                    <span class="visually-hidden">(review.page_label)</span>
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
                <span class="ocr-page-total" aria-hidden="true">
                    <bdi dir="ltr">(format!("/ {pages}"))</bdi>
                </span>
            </span>
            <button
                class="icon-button"
                type="button"
                data-ocr-next=""
                aria-label=(review.next_page)
                title=(review.next_page)
            >
                <span class="forward">icon(data: icons::NEXT, size: 16)</span>
            </button>
            <span
                class="visually-hidden"
                data-ocr-position=""
                aria-live="polite"
                aria-atomic="true"
            ></span>
        </form>
    })
}

/// Zoom and the two fit choices for the source, shown once a preview is attached.
#[component]
async fn zoom_controls(locale: Locale) -> Result<impl View> {
    let review = &locale.text().ocr.review;
    Ok(view! {
        <div class="ocr-zoom" data-ocr-zoom="" hidden=(true)>
            <div class="ocr-fit">
                <button
                    class="button ghost compact"
                    type="button"
                    data-ocr-fit="width"
                    aria-pressed="true"
                >
                    (review.fit_width)
                </button>
                <button
                    class="button ghost compact"
                    type="button"
                    data-ocr-fit="page"
                    aria-pressed="false"
                >
                    (review.fit_page)
                </button>
            </div>
            <div class="zoom-steps">
                <button
                    class="icon-button"
                    type="button"
                    data-ocr-zoom-out=""
                    aria-label=(review.zoom_out)
                    title=(review.zoom_out)
                >
                    icon(data: icons::MINUS, size: 16)
                </button>
                <span data-ocr-zoom-value="" aria-label=(review.zoom_label) dir="ltr"></span>
                <button
                    class="icon-button"
                    type="button"
                    data-ocr-zoom-in=""
                    aria-label=(review.zoom_in)
                    title=(review.zoom_in)
                >
                    icon(data: icons::PLUS, size: 16)
                </button>
            </div>
        </div>
    })
}

#[component]
async fn source_panel(locale: Locale, id: JobId) -> Result<impl View> {
    let review = &locale.text().ocr.review;
    Ok(view! {
        <section class="ocr-source-panel">
            <h2 class="visually-hidden">(review.source_title)</h2>
            <p class="ocr-source-status" data-ocr-source-status="" aria-live="polite">
                (review.source_missing)
            </p>
            source_picker(locale: locale, id: id)
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
