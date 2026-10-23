//! What pages tell search engines and link previews.

use serde_json::Value;
use topcoat::{
    context::Cx,
    router::{
        header::HOST,
        request::{headers, uri},
    },
};

/// A page's description for the `<head>`.
pub struct PageMeta {
    /// The `<title>`, also used for link previews.
    pub title: String,
    pub description: String,
    /// Whether search engines may index the page, under its canonical URL.
    pub index: bool,
    /// Structured data about the page, as JSON-LD.
    pub structured_data: Option<Value>,
}

impl PageMeta {
    /// A page that search engines should leave out, such as a job's page.
    pub fn private(title: String, description: String) -> Self {
        Self {
            title,
            description,
            index: false,
            structured_data: None,
        }
    }
}

/// The site's address as the visitor sees it, such as `https://arabi.tools`: the host they
/// asked for, with the scheme the proxy in front of the app reports.
pub fn origin(cx: &Cx) -> String {
    let headers = headers(cx);
    let host = uri(cx)
        .authority()
        .map(AsRef::<str>::as_ref)
        .or_else(|| headers.get(HOST)?.to_str().ok())
        .filter(|host| is_host(host))
        .unwrap_or("localhost");
    let https = headers
        .get("x-forwarded-proto")
        .is_some_and(|proto| proto.as_bytes().eq_ignore_ascii_case(b"https"));
    let scheme = if https { "https" } else { "http" };
    format!("{scheme}://{host}")
}

/// Whether `host` looks like a host name or address with an optional port, so it is safe to
/// put in a URL.
fn is_host(host: &str) -> bool {
    !host.is_empty()
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-.:[]".contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::is_host;

    #[test]
    fn accepts_only_plain_hosts() {
        for host in [
            "arabi.tools",
            "127.0.0.1:3000",
            "[::1]:3000",
            "tools.ieasybooks.com",
        ] {
            assert!(is_host(host), "{host}");
        }
        for host in ["", "evil.com/path", "a b", "x\"y", "a<b"] {
            assert!(!is_host(host), "{host}");
        }
    }
}
