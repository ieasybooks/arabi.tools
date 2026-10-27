//! One transcription job: its upload, its live status page and its downloads.

use std::sync::Arc;
use std::time::Duration;

use tempfile::TempPath;
use tokio::io::AsyncWriteExt;
use topcoat::{
    Result,
    asset::{Asset, asset},
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

use super::{COOKIE, Reply, rejected};
use crate::app::download::Download;
use crate::app::transcribe;
use crate::i18n::Locale;
use crate::icons;
use crate::queue::{CancelReason, JobId, Status};
use crate::result_controls::{self, COPY_SCRIPT, SEARCH_SCRIPT};
use crate::tools::transcription::{
    AudioProblem, MAX_UPLOAD_BYTES, Transcript, Transcription, check_audio, download_name,
    has_accepted_extension,
};

module_param!(pub id: JobId, error = not_found);

/// How often an idle status page is sent again, so proxies keep its connection open.
const HEARTBEAT: Duration = Duration::from_secs(30);
const TRANSCRIPT_SCRIPT: Asset = asset!("../../transcript.js");

#[page]
pub(super) async fn page(cx: &Cx) -> Result<impl View> {
    let id = path_param::<Id>(cx)?.clone();
    let locale = Locale::of(cx);
    let mut watch = app_context::<Transcription>(cx)
        .queue
        .watch(&id)
        .ok_or_not_found()?;
    Ok(view! {
        <script src=(SEARCH_SCRIPT) defer=(true)></script>
        <script src=(COPY_SCRIPT) defer=(true)></script>
        <script src=(TRANSCRIPT_SCRIPT) defer=(true)></script>
        transcribe::tool_hero(locale: locale, lead: false)
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
async fn job_status(locale: Locale, id: JobId, status: Status<Transcript>) -> Result<impl View> {
    let text = &locale.text().transcription;
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
        transcribe::Step::Download
    } else {
        transcribe::Step::Transcribe
    };
    let percent = match status {
        Status::Running { percent, .. } => Some(percent),
        _ => None,
    };
    let keep_open = matches!(status, Status::Waiting { .. } | Status::Running { .. });
    let transcript = match status {
        Status::Done(transcript) => Some(transcript),
        _ => None,
    };
    Ok(view! {
        transcribe::steps(locale: locale, current: step, failed: state == "failed")
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
            if keep_open {
                <p class="keep-open">
                    icon(data: icons::WARNING, size: 20)
                    <span>(text.keep_open)</span>
                </p>
            }
            if let Some(transcript) = transcript {
                result(locale: locale, id: id, transcript: transcript)
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
                    href=(href!(transcribe::page))
                >
                    (text.another)
                </a>
            </p>
        }
    })
}

/// A finished job's downloads and transcript.
#[component]
async fn result(locale: Locale, id: JobId, transcript: Arc<Transcript>) -> Result<impl View> {
    let text = &locale.text().transcription;
    let review = &locale.text().review;
    // JSON escapes line endings that HTML attribute parsing would normalize.
    let copy_override = transcript_copy_override(&transcript)
        .map(serde_json::to_string)
        .transpose()?;
    Ok(view! {
        <div
            class="result transcript-result"
            data-result="transcript"
            data-job-id=(id.to_string())
            data-copy-text=(copy_override)
        >
            <div class="result-heading">
                <p class="result-name">
                    icon(data: icons::TRANSCRIPT, size: 20)
                    <span dir="auto">(transcript.name.as_str())</span>
                </p>
                <div class="downloads">
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
                        href=(href!(download_srt, Id(id.clone())))
                        download=(true)
                    >
                        icon(data: icons::DOWNLOAD, size: 16)
                        <span>(text.download_srt)</span>
                    </a>
                </div>
            </div>
            <p class="hint">(locale.kept_for())</p>
            result_controls::search_toolbar(locale: locale)
            reading_controls(locale: locale, transcript: transcript.clone())
            result_controls::copy_feedback(locale: locale)
            if transcript.text.trim().is_empty() {
                <p>(text.no_speech)</p>
            } else {
                <h2 class="transcript-title">(text.transcript_title)</h2>
                <ol
                    class="cues"
                    role="list"
                    aria-label=(text.transcript_title)
                    tabindex="0"
                >
                    for (number, cue) in (1..).zip(&transcript.cues) {
                        <li
                            class="cue"
                            data-cue=(number.to_string())
                            id=(format!("transcript-{id}-cue-{number}"))
                        >
                            <div class="cue-heading">
                                <p class="cue-time">
                                    <span class="cue-number">(number.to_string())</span>
                                    <bdi dir="ltr" data-cue-time=(true)>
                                        (cue.start.as_str())
                                        " → "
                                        (cue.end.as_str())
                                    </bdi>
                                </p>
                                <button
                                    type="button"
                                    class="button secondary copy-cue"
                                    data-copy-cue=(true)
                                    hidden=(true)
                                >
                                    (review.copy_cue)
                                </button>
                            </div>
                            <p
                                class="cue-text"
                                dir="auto"
                                lang=(transcript.language.as_str())
                            >
                                (cue.text.as_str())
                            </p>
                        </li>
                    }
                </ol>
            }
        </div>
    })
}

#[component]
async fn reading_controls(locale: Locale, transcript: Arc<Transcript>) -> Result<impl View> {
    let text = &locale.text().review;
    Ok(view! {
        <div class="transcript-actions" data-transcript-controls=(true) hidden=(true)>
            <button type="button" class="button secondary" data-copy-all=(true)>
                (text.copy_all)
            </button>
            if !transcript.cues.is_empty() {
                <label class="timestamps-choice">
                    <input type="checkbox" data-show-timestamps=(true) checked=(true)>
                    (text.show_timestamps)
                </label>
                <label class="cue-choice">
                    <span>(text.go_to_cue)</span>
                    <select data-cue-select=(true)>
                        for (number, cue) in (1..).zip(&transcript.cues) {
                            <option value=(number.to_string())>
                                (text
                                    .cue_at
                                    .replace("{number}", &number.to_string())
                                    .replace(
                                        "{time}",
                                        &format!("\u{2066}{}\u{2069}", cue.start),
                                    ))
                            </option>
                        }
                    </select>
                </label>
            }
        </div>
    })
}

/// Normally TXT is the cue text plus a newline per cue. SRT parsing can fold
/// internal newlines, so retain the exact TXT only when it cannot be rebuilt.
/// This avoids a second full transcript in ordinary result markup.
fn transcript_copy_override(transcript: &Transcript) -> Option<&str> {
    let mut remaining = transcript.text.as_str();
    for cue in &transcript.cues {
        let Some(rest) = remaining
            .strip_prefix(&cue.text)
            .and_then(|rest| rest.strip_prefix('\n'))
        else {
            return Some(&transcript.text);
        };
        remaining = rest;
    }
    (!remaining.is_empty()).then_some(transcript.text.as_str())
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::tools::transcription::Cue;

    #[test]
    fn copy_preserves_txt_even_when_srt_cues_fold_lines() {
        let mut transcript = Transcript {
            name: "lesson".into(),
            language: "ar".into(),
            text: "كِتَاب\nhello & <b>\n".into(),
            srt: String::new(),
            cues: ["كِتَاب", "hello & <b>"]
                .into_iter()
                .map(|text| Cue {
                    start: "00:00:00,000".into(),
                    end: "00:00:01,000".into(),
                    text: text.into(),
                })
                .collect(),
        };
        assert_eq!(transcript_copy_override(&transcript), None);
        transcript.text = "كِتَاب\nhello &\n<b>\n".into();
        assert_eq!(
            transcript_copy_override(&transcript),
            Some("كِتَاب\nhello &\n<b>\n")
        );
        transcript.cues.clear();
        transcript.text.clear();
        assert_eq!(transcript_copy_override(&transcript), None);
    }
}

/// Receives the file for a reserved job and queues the job.
#[route(POST "./audio")]
pub(super) async fn upload(cx: &Cx, mut form: Multipart) -> Result<(StatusCode, Json<Reply>)> {
    let locale = Locale::of(cx);
    let transcription = app_context::<Transcription>(cx);
    let id = path_param::<Id>(cx)?;
    let upload = match transcription.queue.begin_upload(id) {
        Ok(upload) => upload,
        Err(rejection) => return Ok(rejected(cx, rejection)),
    };
    let refused = |problem| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(Reply::Error {
                error: locale.audio_problem(problem),
                job: None,
            }),
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
        fallback_stem: "transcript",
        file_name: format!("{}.txt", transcript.name),
        content_type: "text/plain; charset=utf-8",
        body: transcript.text.clone().into_bytes(),
    })
}

#[route(GET "./transcript.srt")]
async fn download_srt(cx: &Cx) -> Result<Download> {
    let transcript = finished(cx)?;
    Ok(Download {
        fallback_stem: "transcript",
        file_name: format!("{}.srt", transcript.name),
        content_type: "application/x-subrip; charset=utf-8",
        body: transcript.srt.clone().into_bytes(),
    })
}

fn finished(cx: &Cx) -> Result<Arc<Transcript>> {
    let id = path_param::<Id>(cx)?;
    Ok(app_context::<Transcription>(cx)
        .queue
        .result(id)
        .ok_or_not_found()?)
}
