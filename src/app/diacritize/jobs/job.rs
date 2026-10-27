//! One diacritization job: submission, live status, cancellation, and streamed files.

use std::{sync::Arc, time::Duration};

use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use topcoat::{
    Result,
    context::{Cx, app_context},
    cookie::{Cookies, signed_cookies},
    icon::icon,
    router::{
        StatusCode,
        content::{
            Form, Json,
            multipart::{Field, Multipart},
        },
        error::{RouterErrorExt, not_found, see_other},
        href, module_param, page, path_param,
        request::headers,
        response::{IntoResponse, Response},
        route,
    },
    runtime::{connected, shard},
    view::{View, ViewExt, component, emit, live, view},
};

use super::{COOKIE, Reply, error, rejected, remember, unavailable};
use crate::tools::diacritization::{
    Diacritization, Document, DocumentKind, MAX_UPLOAD_BYTES, Model, Problem, SubmitError,
    UploadedDocument, download_name,
};
use crate::{
    app::{diacritize, download::stream_file},
    i18n::Locale,
    icons,
    queue::{CancelReason, JobId, Status},
};

module_param!(pub id: JobId, error = not_found);

const HEARTBEAT: Duration = Duration::from_secs(30);

#[page]
pub(in crate::app::diacritize) async fn page(cx: &Cx) -> Result<impl View> {
    crate::app::workspace::private_response(cx);
    let id = path_param::<Id>(cx)?.clone();
    let locale = Locale::of(cx);
    let exists = app_context::<Diacritization>(cx).queue.watch(&id).is_some();
    Ok(view! {
        diacritize::tool_hero(locale: locale, lead: false)
        <section class="workspace">
            <div class="container workspace-layout workspace-wide">
                if exists {
                    diacritize::editor(locale: locale, initial: Some(id))
                } else {
                    (StatusCode::NOT_FOUND)
                    <p class="notice">(locale.text().workspace.missing)</p>
                    <a class="button secondary" href=(href!(diacritize::page))>
                        (locale.text().diacritization.another)
                    </a>
                }
            </div>
        </section>
        <script src=(crate::app::workspace::SCRIPT) defer=(true)></script>
        <script src=(diacritize::SCRIPT) defer=(true)></script>
    })
}

/// A job link is the existing read capability. Cancellation stays on its HTTP route.
#[shard("/diacritize/observe")]
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
                let Some(mut watch) = app_context::<Diacritization>(cx).queue.watch(&id) else {
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
                        job_status(locale: locale, id: id.clone(), status: status)
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

#[component]
async fn job_status(
    cx: &Cx,
    locale: Locale,
    id: JobId,
    status: Status<Document>,
) -> Result<impl View> {
    let text = &locale.text().diacritization;
    let finished = status.is_finished();
    let message = match &status {
        Status::Reserved => text.reserved.into(),
        Status::Waiting { position } => locale.diacritization_waiting(*position),
        Status::Running { .. } => text.running.into(),
        Status::Done(_) => text.done.into(),
        Status::Failed => app_context::<Diacritization>(cx)
            .queue
            .failure(&id)
            .and_then(|error| error.downcast_ref::<Problem>().copied())
            .map_or_else(
                || text.failed.into(),
                |problem| locale.diacritization_problem(problem),
            ),
        Status::Cancelled(CancelReason::Unwatched) => text.cancelled_unwatched.into(),
        Status::Cancelled(CancelReason::Shutdown) => text.cancelled_shutdown.into(),
        Status::Cancelled(CancelReason::User) => locale.text().cancelled_user.into(),
    };
    let (state, state_icon) = match &status {
        Status::Reserved | Status::Waiting { .. } => ("waiting", Some(icons::WAITING)),
        Status::Running { .. } => ("running", None),
        Status::Done(_) => ("done", Some(icons::DONE)),
        Status::Cancelled(CancelReason::User) => ("cancelled", None),
        Status::Failed | Status::Cancelled(_) => ("failed", Some(icons::FAILED)),
    };
    let phase = match &status {
        Status::Reserved => "reserved",
        Status::Waiting { .. } => "queued",
        Status::Running { .. } => "processing",
        Status::Done(_) => "completed",
        Status::Cancelled(CancelReason::User) => "cancelled",
        Status::Failed | Status::Cancelled(_) => "failed",
    };
    let percent = match &status {
        Status::Running { percent, .. } => Some(*percent),
        _ => None,
    };
    let document = match status {
        Status::Done(document) => Some(document),
        _ => None,
    };
    Ok(view! {
        diacritize::steps(
            locale: locale,
            current: if state == "done" {
                diacritize::Step::Download
            } else {
                diacritize::Step::Diacritize
            },
            failed: state == "failed"
        )
        <div
            class="job"
            data-state=(state)
            data-update=(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis())
            data-job-id=(id.to_string())
            data-phase=(phase)
        >
            <div class="job-heading">
                if let Some(state_icon) = state_icon {
                    <span class="job-icon">icon(data: state_icon, size: 20)</span>
                }
                <p class="status">(message)</p>
                if let Some(percent) = percent {
                    <p class="percent">(locale.percent(percent))</p>
                }
            </div>
            if let Some(percent) = percent {
                <progress max="100" value=(percent) aria-labelledby="job-status"></progress>
            }
            if !finished {
                <p class="keep-open">
                    icon(data: icons::WARNING, size: 20)
                    <span>(text.keep_open)</span>
                </p>
            }
            if let Some(document) = document {
                result(locale: locale, id: id.clone(), document: document)
            }
        </div>
        if finished {
            <p class="another">
                <a class="button secondary" href=(href!(diacritize::page))>
                    (text.another)
                </a>
            </p>
        }
    })
}

#[component]
async fn result(locale: Locale, id: JobId, document: Arc<Document>) -> Result<impl View> {
    let text = &locale.text().diacritization;
    let name = if document.kind == DocumentKind::Text {
        text.result_label.to_owned()
    } else {
        document.name.clone()
    };
    Ok(view! {
        <div
            class="result diacritize-result"
            data-result="diacritization"
            data-job-id=(id.to_string())
            data-model=(locale.diacritization_model(document.model))
            data-model-value=(document.model.as_str())
            data-source=(document.text.as_ref().map(|text| text.source.as_str()))
            data-mode=(if document.text.is_some() { "text" } else { "document" })
        >
            <div class="result-heading">
                <p class="result-name">
                    icon(data: icons::TRANSCRIPT, size: 20)
                    <span dir="auto">(name)</span>
                </p>
                <p class="hint">
                    (text.model)
                    ": "
                    <bdi dir="auto">(locale.diacritization_model(document.model))</bdi>
                </p>
            </div>
            if let Some(text_result) = document.text.as_ref() {
                <label class="visually-hidden" for="diacritized-text">
                    (text.result_label)
                </label>
                <textarea
                    id="diacritized-text"
                    class="diacritize-text"
                    name="result"
                    rows="7"
                    dir="auto"
                    readonly=(true)
                >
                    "\n"
                    (text_result.output.as_str())
                </textarea>
            }
            <div class="downloads">
                if document.text.is_some() {
                    <button
                        type="button"
                        class="button secondary"
                        data-copy="diacritized-text"
                        data-copied=(text.copied)
                        data-copy-failed=(text.copy_failed)
                    >
                        (text.copy)
                    </button>
                }
                <a
                    class="button secondary"
                    href=(href!(download_result, Id(id.clone())))
                    download=(true)
                >
                    icon(data: icons::DOWNLOAD, size: 16)
                    (if document.kind == DocumentKind::Docx {
                        text.download_docx
                    } else {
                        text.download_text
                    })
                </a>
                if document.text.is_none() {
                    <a
                        class="button secondary"
                        href=(href!(download_original, Id(id)))
                        download=(true)
                    >
                        (text.download_original)
                    </a>
                }
            </div>
            <p class="copy-status hint" role="status"></p>
            <p class="hint">(text.replaces_marks)</p>
            <p class="hint">(locale.diacritization_kept_for())</p>
        </div>
    })
}

#[derive(Deserialize)]
struct TextSubmission {
    model: String,
    text: String,
}

#[route(POST "./text")]
pub(super) async fn submit_text(
    cx: &Cx,
    Form(input): Form<TextSubmission>,
) -> Result<(StatusCode, Json<Reply>)> {
    let tool = app_context::<Diacritization>(cx);
    if !tool.is_available() {
        return Ok(unavailable(cx));
    }
    let id = path_param::<Id>(cx)?;
    let reservation = match tool.queue.begin_upload(id) {
        Ok(reservation) => reservation,
        Err(rejection) => return Ok(rejected(cx, rejection)),
    };
    let model = match chosen_model(cx, &input.model) {
        Ok(model) => model,
        Err((status, message)) => return Ok(error(status, message)),
    };
    if input.text.trim().is_empty() {
        return Ok(error(
            StatusCode::UNPROCESSABLE_ENTITY,
            Locale::of(cx).text().diacritization.text_missing,
        ));
    }
    Ok(match tool.submit_text(reservation, model, input.text) {
        Ok(()) => accepted(cx, id),
        Err(SubmitError::Problem(problem)) => problem_reply(cx, problem),
        Err(SubmitError::Rejected(rejection)) => rejected(cx, rejection),
    })
}

#[route(POST "./document")]
pub(super) async fn upload(cx: &Cx, mut form: Multipart) -> Result<(StatusCode, Json<Reply>)> {
    let tool = app_context::<Diacritization>(cx);
    if !tool.is_available() {
        return Ok(unavailable(cx));
    }
    let id = path_param::<Id>(cx)?;
    let reservation = match tool.queue.begin_upload(id) {
        Ok(reservation) => reservation,
        Err(rejection) => return Ok(rejected(cx, rejection)),
    };
    let (upload, file_name, model) = match receive(&mut form).await? {
        Ok(received) => received,
        Err(problem) => return Ok(problem_reply(cx, problem)),
    };
    let model = match chosen_model(cx, &model) {
        Ok(model) => model,
        Err((status, message)) => return Ok(error(status, message)),
    };
    let upload = match tool.validate(upload).await {
        Ok(upload) => upload,
        Err(problem) => return Ok(problem_reply(cx, problem)),
    };
    Ok(
        match tool.submit_document(reservation, model, upload, download_name(&file_name)) {
            Ok(()) => accepted(cx, id),
            Err(rejection) => rejected(cx, rejection),
        },
    )
}

fn chosen_model(cx: &Cx, value: &str) -> Result<Model, (StatusCode, &'static str)> {
    let text = &Locale::of(cx).text().diacritization;
    let model = value
        .parse()
        .map_err(|_| (StatusCode::UNPROCESSABLE_ENTITY, text.choose_model))?;
    if !app_context::<Diacritization>(cx).model_available(model) {
        return Err((StatusCode::SERVICE_UNAVAILABLE, text.model_unavailable));
    }
    Ok(model)
}

fn problem_reply(cx: &Cx, problem: Problem) -> (StatusCode, Json<Reply>) {
    error(
        StatusCode::UNPROCESSABLE_ENTITY,
        Locale::of(cx).diacritization_problem(problem),
    )
}

fn accepted(cx: &Cx, id: &JobId) -> (StatusCode, Json<Reply>) {
    remember(cx, id);
    (
        StatusCode::OK,
        Json(Reply::Next {
            job_id: id.to_string(),
            next: href!(page, Id(id.clone())).resolve(cx),
        }),
    )
}

async fn receive(
    form: &mut Multipart,
) -> Result<Result<(UploadedDocument, String, String), Problem>> {
    let mut received = None;
    let mut model = None;
    while let Some(mut field) = form.next_field().await? {
        match field.name() {
            Some("model") => {
                if model.is_some() {
                    return Ok(Err(Problem::WrongType));
                }
                // Model identifiers are short; never collect an arbitrary multipart field.
                let mut bytes = Vec::new();
                while let Some(chunk) = field.chunk().await? {
                    if bytes.len() + chunk.len() > 32 {
                        return Ok(Err(Problem::WrongType));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                model = Some(String::from_utf8(bytes).unwrap_or_default());
            }
            Some("file") => {
                if received.is_some() {
                    return Ok(Err(Problem::WrongType));
                }
                let name = field.file_name().unwrap_or_default().to_owned();
                let document = match UploadedDocument::new(&name).await {
                    Ok(document) => document,
                    Err(problem) => return Ok(Err(problem)),
                };
                if !save(&mut field, &document.path).await? {
                    return Ok(Err(Problem::TooLarge));
                }
                received = Some((document, name));
            }
            _ => {}
        }
    }
    Ok(received
        .map(|(document, name)| (document, name, model.unwrap_or_default()))
        .ok_or(Problem::Missing))
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

pub(in crate::app::diacritize) fn owns_job(cx: &Cx, id: &JobId) -> bool {
    signed_cookies(cx)
        .get(COOKIE)
        .and_then(|cookie| cookie.value().parse::<JobId>().ok())
        .as_ref()
        == Some(id)
}

#[route(POST "./cancel")]
pub(in crate::app::diacritize) async fn cancel(cx: &Cx) -> Result<Response> {
    let id = path_param::<Id>(cx)?;
    if !owns_job(cx, id) {
        return Err(not_found().into());
    }
    let cancelled = app_context::<Diacritization>(cx).queue.cancel(id);
    let next = href!(page, Id(id.clone())).resolve(cx);
    if headers(cx)
        .get("accept")
        .is_some_and(|value| value == "application/json")
    {
        Json(serde_json::json!({ "cancelled": cancelled, "next": next })).into_response(cx)
    } else {
        see_other(next).into_response(cx)
    }
}

#[route(GET "./result")]
async fn download_result(cx: &Cx) -> Result<Response> {
    let document = finished(cx)?;
    stream_file(
        Arc::clone(&document),
        &document.path,
        &format!("{}.{}", document.name, document.kind.extension()),
        "diacritized",
        document.kind.content_type(),
    )
    .await
}

#[route(GET "./original")]
async fn download_original(cx: &Cx) -> Result<Response> {
    let document = finished(cx)?;
    stream_file(
        Arc::clone(&document),
        &document.original_path,
        &format!("{}.{}", document.name, document.kind.extension()),
        "original",
        document.kind.content_type(),
    )
    .await
}

fn finished(cx: &Cx) -> Result<Arc<Document>> {
    let id = path_param::<Id>(cx)?;
    Ok(app_context::<Diacritization>(cx)
        .queue
        .result(id)
        .ok_or_not_found()?)
}
