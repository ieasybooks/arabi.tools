//! File downloads shared by the tools.
use std::fmt::Write;
use std::{path::Path, sync::Arc};

use futures_util::TryStreamExt;
use http_body::Frame;
use http_body_util::StreamBody;
use tokio_util::io::ReaderStream;
use topcoat::{
    Result,
    context::Cx,
    router::{
        Body,
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
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

/// Streams a disk-backed result. Keeping its owner in the stream prevents queue expiry
/// from removing its temporary directory while the response is being downloaded.
pub(crate) async fn stream_file<T: Send + Sync + 'static>(
    owner: Arc<T>,
    path: &Path,
    file_name: &str,
    fallback_stem: &str,
    content_type: &str,
) -> Result<Response> {
    let file = tokio::fs::File::open(path).await?;
    let size = file.metadata().await?.len();
    let chunks = ReaderStream::new(file).map_ok(move |chunk| {
        let _keep_alive = &owner;
        Frame::data(chunk)
    });
    Ok(Response::builder()
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, size)
        .header(CACHE_CONTROL, "no-store")
        .header(CONTENT_DISPOSITION, attachment(file_name, fallback_stem))
        .body(Body::new(StreamBody::new(chunks)))?)
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
    let fallback: String = fallback
        .chars()
        .map(|c| {
            if c.is_ascii_control() || matches!(c, '"' | '\\' | '/') {
                '_'
            } else {
                c
            }
        })
        .collect();
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

    #[test]
    fn escapes_untrusted_file_names_in_headers() {
        assert_eq!(
            attachment("bad\"\r\n.txt", "document"),
            "attachment; filename=\"bad___.txt\"; filename*=UTF-8''bad%22%0D%0A.txt"
        );
    }

    #[tokio::test]
    async fn streams_files_and_keeps_their_directory_until_the_body_is_dropped() {
        use http_body_util::BodyExt;
        use std::sync::Arc;

        let owner = Arc::new(tempfile::tempdir().unwrap());
        let directory = owner.path().to_owned();
        let path = directory.join("result.txt");
        let content = vec![b'x'; 128 * 1024];
        tokio::fs::write(&path, &content).await.unwrap();
        let response = super::stream_file(owner, &path, "result.txt", "document", "text/plain")
            .await
            .unwrap();
        assert!(directory.exists());
        assert_eq!(response.headers()["cache-control"], "no-store");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body.as_ref(), content.as_slice());
        assert!(!directory.exists());
    }
}
