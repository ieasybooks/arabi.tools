//! Switches the site's language and returns to the page the visitor was on.

use serde::Deserialize;
use topcoat::{
    Result,
    context::Cx,
    cookie::{Cookies, cookie, cookies, time::Duration},
    router::{
        content::Form,
        error::{SeeOther, see_other},
        route,
    },
};

use crate::i18n::{COOKIE, Locale};

#[derive(Deserialize)]
struct Choice {
    lang: String,
    back: String,
}

#[route(POST)]
pub(super) async fn switch(cx: &Cx, Form(choice): Form<Choice>) -> Result<SeeOther> {
    if let Some(locale) = Locale::from_code(&choice.lang) {
        cookies(cx).add(cookie! {
            COOKIE = locale.code();
            Path = "/";
            SameSite = Lax;
            MaxAge = Duration::days(365)
        });
    }
    Ok(see_other(local_path(&choice.back)))
}

/// `back` when it is a path on this site, otherwise the home page. Browsers read `//` and
/// `/\` as the start of another site's address.
fn local_path(back: &str) -> &str {
    let mut chars = back.chars();
    match (chars.next(), chars.next()) {
        (Some('/'), Some('/' | '\\')) => "/",
        (Some('/'), _) => back,
        _ => "/",
    }
}

#[cfg(test)]
mod tests {
    use super::local_path;

    #[test]
    fn only_returns_to_local_paths() {
        assert_eq!(local_path("/transcribe"), "/transcribe");
        assert_eq!(local_path("/"), "/");
        assert_eq!(local_path("//evil.example"), "/");
        assert_eq!(local_path("/\\evil.example"), "/");
        assert_eq!(local_path("https://evil.example"), "/");
        assert_eq!(local_path(""), "/");
    }
}
