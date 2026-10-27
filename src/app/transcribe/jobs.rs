//! Reserves a transcription job before its file is uploaded, so a full queue or an unfinished
//! job is reported before the visitor spends time uploading.

pub(super) mod job;

use serde::{Deserialize, Serialize};
use topcoat::{
    Result,
    context::{Cx, app_context},
    cookie::{Cookies, cookies},
    router::{
        StatusCode,
        content::{Form, Json},
        href,
        request::client_ip,
        route,
    },
};

use crate::i18n::Locale;
use crate::queue::{Attempt, JobId, Rejection};
use crate::tools::transcription::Transcription;

/// Remembers only the job accepted in this browser, never one found by client address.
const COOKIE: &str = "transcription_job";

/// What the upload page's script receives from the job routes.
#[derive(Serialize)]
#[serde(untagged)]
enum Reply {
    /// Where to upload the file for a reserved job.
    Upload {
        job_id: String,
        next: String,
        upload: String,
    },
    /// The page to show once the file is uploaded.
    Next { job_id: String, next: String },
    /// Why the request was refused, ready to show.
    Error {
        error: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        job: Option<String>,
    },
}

/// A reservation request from the upload page.
#[derive(Deserialize)]
struct Reservation {
    /// The browser tab's upload attempt, which lets it replace a reservation it lost.
    attempt: Option<String>,
}

#[route(POST)]
pub(super) async fn reserve(
    cx: &Cx,
    reservation: Option<Form<Reservation>>,
) -> Result<(StatusCode, Json<Reply>)> {
    let transcription = app_context::<Transcription>(cx);
    let attempt = reservation
        .and_then(|Form(reservation)| reservation.attempt)
        .and_then(|attempt| Attempt::parse(&attempt));
    Ok(match transcription.queue.reserve(client_ip(cx), attempt) {
        Ok(id) => {
            let upload = href!(job::upload, job::Id(id.clone())).resolve(cx);
            let next = href!(job::page, job::Id(id.clone())).resolve(cx);
            (
                StatusCode::OK,
                Json(Reply::Upload {
                    job_id: id.to_string(),
                    next,
                    upload,
                }),
            )
        }
        Err(rejection) => rejected(cx, rejection),
    })
}

/// The cookie's job page, only while that job is unfinished. Reading it must not watch it
/// or extend its lifetime.
pub(super) fn unfinished_job(cx: &Cx) -> Option<String> {
    let id: JobId = cookies(cx).get(COOKIE)?.value().parse().ok()?;
    app_context::<Transcription>(cx)
        .queue
        .is_unfinished(&id)
        .then(|| href!(job::page, job::Id(id)).resolve(cx))
}

fn rejected(cx: &Cx, rejection: Rejection) -> (StatusCode, Json<Reply>) {
    let status = match rejection {
        Rejection::Closed | Rejection::Full => StatusCode::SERVICE_UNAVAILABLE,
        Rejection::ActiveJob => StatusCode::TOO_MANY_REQUESTS,
        Rejection::NoReservation => StatusCode::CONFLICT,
    };
    (
        status,
        Json(Reply::Error {
            error: Locale::of(cx).rejection(rejection).to_owned(),
            job: if rejection == Rejection::ActiveJob {
                unfinished_job(cx)
            } else {
                None
            },
        }),
    )
}
