//! File downloads shared by the tools.
use std::fmt::Write;
use topcoat::{
    Result,
    context::Cx,
    router::{
        Body,
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE},
        response::{IntoResponse, Response},
    },
};

/// A result file, saved under the uploaded file's name.
pub(crate) struct Download {
    pub file_name: String,
    pub fallback_stem: &'static str,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl IntoResponse for Download {
    fn into_response(self, _cx: &Cx) -> Result<Response> {
        Ok(Response::builder()
            .header(CONTENT_TYPE, self.content_type)
            .header(CACHE_CONTROL, "no-store")
            .header(
                CONTENT_DISPOSITION,
                attachment(&self.file_name, self.fallback_stem),
            )
            .body(Body::from(self.body))?)
    }
}

/// A `Content-Disposition` value saving the response as `file_name`. Browsers use the
/// UTF-8 form (RFC 6266); the plain one is a fallback for names that are not ASCII.
fn attachment(file_name: &str, fallback_stem: &str) -> String {
    let fallback = if file_name.is_ascii() {
        file_name.to_owned()
    } else {
        let extension = file_name.rsplit('.').next().unwrap_or("txt");
        format!("{fallback_stem}.{extension}")
    };
    let mut encoded = String::new();
    for byte in file_name.bytes() {
        if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use super::attachment;

    #[test]
    fn names_downloads_in_utf8_with_an_ascii_fallback() {
        assert_eq!(
            attachment("lesson 1.txt", "transcript"),
            "attachment; filename=\"lesson 1.txt\"; filename*=UTF-8''lesson%201.txt"
        );
        assert_eq!(
            attachment("درس.srt", "transcript"),
            "attachment; filename=\"transcript.srt\"; filename*=UTF-8''%D8%AF%D8%B1%D8%B3.srt"
        );
    }
}
