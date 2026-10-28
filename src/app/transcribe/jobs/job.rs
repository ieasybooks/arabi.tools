//! One transcription job: its upload, its live status page and its downloads.

use std::sync::Arc;
use std::time::Duration;

use tempfile::TempPath;
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
        error::{RouterErrorExt, not_found},
        href, module_param, page, path_param, route,
    },
    runtime::{connected, shard},
    view::{View, ViewExt, component, emit, live, view},
};

use super::{COOKIE, Reply, rejected};
use crate::app::download::Download;
use crate::app::transcribe;
use crate::i18n::Locale;
use crate::icons;
use crate::queue::{CancelReason, JobId, Status};
use crate::result_controls::{self, COPY_SCRIPT, CopyAll, DownloadLink, SEARCH_SCRIPT};
use crate::tools::transcription::{
    AudioProblem, MAX_UPLOAD_BYTES, Transcript, Transcription, check_audio, download_name,
    has_accepted_extension,
};

module_param!(pub id: JobId, error = not_found);

/// How often an idle status page is sent again, so proxies keep its connection open.
const HEARTBEAT: Duration = Duration::from_secs(30);

#[page]
pub(super) async fn page(cx: &Cx) -> Result<impl View> {
    let id = path_param::<Id>(cx)?.clone();
    let locale = Locale::of(cx);
    crate::workspace::private_response(cx);
    let exists = app_context::<Transcription>(cx).queue.watch(&id).is_some();
    Ok(view! {
        <script src=(SEARCH_SCRIPT) defer=(true)></script>
        <script src=(COPY_SCRIPT) defer=(true)></script>
        <script src=(transcribe::REVIEW_SCRIPT) defer=(true)></script>
        transcribe::tool_hero(locale: locale, lead: false)
        <section class="workspace">
            <div class="container workspace-layout">
                if exists {
                    (live! {
                        let mut watch = app_context::<Transcription>(cx)
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
                    <a class="button secondary" href=(href!(transcribe::page))>
                        (locale.text().transcription.another)
                    </a>
                }
            </div>
        </section>
        <script src=(crate::workspace::SCRIPT) defer=(true)></script>
    })
}

/// A job link is the existing read capability. The shard observes it without starting another job.
#[shard("/transcribe/observe")]
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
                let Some(mut watch) = app_context::<Transcription>(cx).queue.watch(&id) else {
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

#[component]
async fn job_status(
    locale: Locale,
    id: JobId,
    status: Status<Transcript>,
    embedded: bool,
) -> Result<impl View> {
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
            if keep_open {
                <p class="keep-open">
                    icon(data: icons::WARNING, size: 20)
                    <span>(text.keep_open)</span>
                </p>
            }
            if let Some(transcript) = transcript {
                result(locale: locale, id: id, transcript: transcript)
            } else if !embedded {
                // The job's own page has no input to carry the line before a result.
                crate::workspace::privacy_note(text: locale.privacy_note())
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

/// A finished job's header, its toolbar (reading controls, then search) and transcript, with the
/// page's privacy line at its foot.
#[component]
async fn result(
    cx: &Cx,
    locale: Locale,
    id: JobId,
    transcript: Arc<Transcript>,
) -> Result<impl View> {
    let text = &locale.text().transcription;
    let review = &locale.text().review;
    // JSON escapes line endings that HTML attribute parsing would normalize.
    let copy_override = transcript_copy_override(&transcript)
        .map(serde_json::to_string)
        .transpose()?;
    // The time column is as wide as the longest start time, so every cue's text lines up.
    let time_chars = transcript
        .cues
        .iter()
        .map(|cue| short_time(&cue.start).len())
        .max()
        .unwrap_or(4);
    let downloads = vec![
        DownloadLink {
            label: text.download_text,
            href: href!(download_text, Id(id.clone())).resolve(cx),
        },
        DownloadLink {
            label: text.download_srt,
            href: href!(download_srt, Id(id.clone())).resolve(cx),
        },
    ];
    Ok(view! {
        <div
            class="result transcript-result"
            data-result="transcript"
            data-job-id=(id.to_string())
            data-copy-text=(copy_override)
        >
            result_controls::result_header(
                locale: locale,
                name: transcript.name.clone(),
                copy: Some(CopyAll::Transcript),
                downloads: downloads
            )
            <div class="result-toolbar">
                reading_controls(locale: locale, transcript: transcript.clone())
                result_controls::search_toolbar(
                    locale: locale,
                    label: review.search_transcript
                )
            </div>
            result_controls::copy_feedback(locale: locale)
            if transcript.text.trim().is_empty() {
                <p>(text.no_speech)</p>
            } else {
                <h2 class="visually-hidden">(text.transcript_title)</h2>
                // Rows run in the transcript's direction, so each time sits beside its text.
                <ol
                    class="cues"
                    role="list"
                    aria-label=(text.transcript_title)
                    tabindex="0"
                    dir=(text_direction(&transcript.language))
                    style=(format!("--time-chars: {time_chars}"))
                >
                    for (number, cue) in (1..).zip(&transcript.cues) {
                        <li
                            class="cue"
                            data-cue=(number.to_string())
                            id=(format!("transcript-{id}-cue-{number}"))
                        >
                            <p class="cue-time">
                                <span class="cue-number visually-hidden">
                                    (number.to_string())
                                </span>
                                <bdi dir="ltr" data-cue-time=(true)>
                                    (short_time(&cue.start))
                                </bdi>
                            </p>
                            <p
                                class="cue-text"
                                dir="auto"
                                lang=(transcript.language.as_str())
                            >
                                (cue.text.as_str())
                            </p>
                            <button
                                type="button"
                                class="button ghost compact copy-cue"
                                data-copy-cue=(true)
                                aria-label=(review.copy_cue)
                                hidden=(true)
                            >
                                icon(data: icons::COPY, size: 16)
                                <span>(review.copy)</span>
                            </button>
                        </li>
                    }
                </ol>
            }
            crate::workspace::privacy_note(text: locale.privacy_note())
        </div>
    })
}

/// The start of the toolbar: the timestamps switch, then the cue picker, each cue named by its
/// short start time.
#[component]
async fn reading_controls(locale: Locale, transcript: Arc<Transcript>) -> Result<impl View> {
    let text = &locale.text().review;
    Ok(view! {
        <div class="transcript-actions" data-transcript-controls=(true) hidden=(true)>
            if !transcript.cues.is_empty() {
                <label class="timestamps-choice">
                    <input type="checkbox" data-show-timestamps=(true) checked=(true)>
                    (text.show_timestamps)
                </label>
                <label class="cue-choice">
                    <span class="visually-hidden">(text.go_to_cue)</span>
                    <select data-cue-select=(true)>
                        for (number, cue) in (1..).zip(&transcript.cues) {
                            <option value=(number.to_string())>
                                (text
                                    .cue_at
                                    .replace("{number}", &number.to_string())
                                    .replace(
                                        "{time}",
                                        &format!("\u{2066}{}\u{2069}", short_time(&cue.start)),
                                    ))
                            </option>
                        }
                    </select>
                </label>
            }
        </div>
    })
}

/// A cue's start time as a reader expects it, from the SRT form `HH:MM:SS,mmm`: `0:03`,
/// `12:45` or `1:02:03`. The rows, the cue picker and search show this; the precise time stays in
/// the SRT file.
fn short_time(srt: &str) -> String {
    let clock = srt.split_once(',').map_or(srt, |(clock, _)| clock);
    let mut parts = clock
        .split(':')
        .map(|part| part.parse::<u64>().unwrap_or(0));
    let (hours, minutes, seconds) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// The direction of text in a transcript's language.
fn text_direction(language: &str) -> &'static str {
    if matches!(language, "ar" | "fa" | "he" | "ps" | "ur" | "yi") {
        "rtl"
    } else {
        "ltr"
    }
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
    fn cue_times_read_like_a_clock() {
        assert_eq!(short_time("00:00:03,046"), "0:03");
        assert_eq!(short_time("00:12:45,999"), "12:45");
        assert_eq!(short_time("01:02:03,500"), "1:02:03");
        assert_eq!(short_time("02:59:59,000"), "2:59:59");
    }

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
                        job_id: id.to_string(),
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
