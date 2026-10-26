//! Reserves a slot before accepting text or uploading a document.

mod job;

use serde::{Deserialize, Serialize};
use topcoat::{
    Result,
    context::{Cx, app_context},
    cookie::{Cookies, cookie, signed_cookies, time},
    router::{
        StatusCode,
        content::{Form, Json},
        href,
        request::client_ip,
        route,
    },
};

use crate::{
    i18n::Locale,
    queue::{Attempt, JobId, Rejection},
    tools::diacritization::Diacritization,
};

const COOKIE: &str = "diacritize_job";

#[derive(Serialize)]
#[serde(untagged)]
enum Reply {
    Upload {
        upload: String,
        text: String,
        cancel: String,
    },
    Next {
        next: String,
    },
    Error {
        error: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        job: Option<String>,
    },
}

#[derive(Deserialize)]
struct Reservation {
    attempt: Option<String>,
}

#[route(POST)]
pub(super) async fn reserve(
    cx: &Cx,
    reservation: Option<Form<Reservation>>,
) -> Result<(StatusCode, Json<Reply>)> {
    let tool = app_context::<Diacritization>(cx);
    if !tool.is_available() {
        return Ok(unavailable(cx));
    }
    let attempt = reservation
        .and_then(|Form(r)| r.attempt)
        .and_then(|attempt| Attempt::parse(&attempt));
    Ok(match tool.queue.reserve(client_ip(cx), attempt) {
        Ok(id) => {
            remember(cx, &id);
            (
                StatusCode::OK,
                Json(Reply::Upload {
                    upload: href!(job::upload, job::Id(id.clone())).resolve(cx),
                    text: href!(job::submit_text, job::Id(id.clone())).resolve(cx),
                    cancel: href!(job::cancel, job::Id(id)).resolve(cx),
                }),
            )
        }
        Err(rejection) => rejected(cx, rejection),
    })
}

/// Remembers only a job accepted in this browser, never one found by client address.
fn remember(cx: &Cx, id: &JobId) {
    signed_cookies(cx).add(cookie! {
        COOKIE = id.to_string();
        HttpOnly;
        SameSite = Lax;
        Path = "/";
        MaxAge = time::Duration::hours(4)
    });
}

pub(super) fn unfinished_job(cx: &Cx) -> Option<String> {
    let id: JobId = signed_cookies(cx).get(COOKIE)?.value().parse().ok()?;
    app_context::<Diacritization>(cx)
        .queue
        .is_unfinished(&id)
        .then(|| href!(job::page, job::Id(id)).resolve(cx))
}

fn error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<Reply>) {
    (
        status,
        Json(Reply::Error {
            error: message.into(),
            job: None,
        }),
    )
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
            error: Locale::of(cx).diacritization_rejection(rejection).into(),
            job: if rejection == Rejection::ActiveJob {
                unfinished_job(cx)
            } else {
                None
            },
        }),
    )
}

fn unavailable(cx: &Cx) -> (StatusCode, Json<Reply>) {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        Locale::of(cx).text().diacritization.unavailable,
    )
}
