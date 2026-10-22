//! Reserves a transcription job before its file is uploaded, so a full queue or an unfinished
//! job is reported before the visitor spends time uploading.

mod job;

use serde::{Deserialize, Serialize};
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{
        StatusCode,
        content::{Form, Json},
        href,
        request::client_ip,
        route,
    },
};

use crate::i18n::Locale;
use crate::queue::{Attempt, Rejection};
use crate::tools::transcription::Transcription;

/// What the upload page's script receives from the job routes.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Reply {
    /// Where to upload the file for a reserved job.
    Upload(String),
    /// The page to show once the file is uploaded.
    Next(String),
    /// Why the request was refused, ready to show.
    Error(String),
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
            let upload = href!(job::upload, job::Id(id)).resolve(cx);
            (StatusCode::OK, Json(Reply::Upload(upload)))
        }
        Err(rejection) => rejected(Locale::of(cx), rejection),
    })
}

fn rejected(locale: Locale, rejection: Rejection) -> (StatusCode, Json<Reply>) {
    let status = match rejection {
        Rejection::Closed | Rejection::Full => StatusCode::SERVICE_UNAVAILABLE,
        Rejection::ActiveJob => StatusCode::TOO_MANY_REQUESTS,
        Rejection::NoReservation => StatusCode::CONFLICT,
    };
    (
        status,
        Json(Reply::Error(locale.rejection(rejection).to_owned())),
    )
}
