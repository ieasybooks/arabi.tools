//! Local Drive with fresh generated keys; never needs real credentials.
use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use arlang_tools::tools::ocr::drive_config;
use aws_lc_rs::{
    encoding::AsDer,
    rsa::{KeyPair, KeySize},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;
use tahweel_drive::DriveConfig;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, path_regex},
};

pub const TEXT: &str = "مرحبا بالعالم\nهذا نص من صفحة كتاب.";
pub const PAGE_ONE: &str = "الصفحة الأولى — كِتَاب 1\nFirst page: café 😀 <b> & نص مشترك";
pub const PAGE_THREE: &str = "الصفحة الثالثة — مـدرسة 3\nThird page: آخر نص مشترك";

// The three-page fixture has a red first page, blank middle and blue last page.
// Existing small white validation fixtures keep their original mock output.
fn review_text(body: &[u8]) -> &'static str {
    let start = body
        .windows(8)
        .position(|bytes| bytes == b"\x89PNG\r\n\x1a\n")
        .or_else(|| body.windows(3).position(|bytes| bytes == b"\xff\xd8\xff"));
    let Some(image) = start.and_then(|start| image::load_from_memory(&body[start..]).ok()) else {
        return TEXT;
    };
    let image = image.to_luma8();
    if image.width() <= 500 || image.height() <= 500 {
        return TEXT;
    }
    // The OCR pipeline uses grayscale. The backgrounds are deliberately far apart in luma.
    match image.get_pixel(image.width() / 10, image.height() / 2).0[0] {
        240..=255 => "",
        160..=239 => PAGE_ONE,
        _ => PAGE_THREE,
    }
}

pub fn keys() -> String {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| {
        let key = KeyPair::generate(KeySize::Rsa2048).unwrap();
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            STANDARD.encode(key.as_der().unwrap().as_ref())
        );
        json!([{"type":"service_account", "client_email":"ocr@example.test", "private_key":pem}])
            .to_string()
    })
    .clone()
}

pub struct Drive {
    pub server: MockServer,
    pub uploads: Arc<AtomicUsize>,
    pub deletes: Arc<AtomicUsize>,
}

impl Drive {
    pub async fn start(delay: Duration) -> Self {
        Self::start_with_review(delay, false).await
    }

    /// Browser review fixtures identify their pages by their rendered shade, not request order.
    pub async fn start_with_review(delay: Duration, review: bool) -> Self {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"access_token":"test-access", "expires_in":3600, "token_type":"Bearer"}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files":[]})))
            .mount(&server)
            .await;
        let uploads = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&uploads);
        let contents = Arc::new(Mutex::new(HashMap::new()));
        let page_contents = Arc::clone(&contents);
        Mock::given(method("POST"))
            .and(path("/upload/drive/v3/files"))
            .respond_with(move |request: &wiremock::Request| {
                assert!(
                    !request
                        .url
                        .query_pairs()
                        .any(|(name, _)| name == "ocrLanguage")
                );
                let id = count.fetch_add(1, Ordering::SeqCst);
                let text = if review {
                    review_text(&request.body)
                } else {
                    TEXT
                };
                page_contents.lock().unwrap().insert(id, text);
                ResponseTemplate::new(200).set_body_json(json!({"id":format!("page-{id}")}))
            })
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/drive/v3/files/page-\d+/export$"))
            .respond_with(move |request: &wiremock::Request| {
                let id: usize = request
                    .url
                    .path()
                    .split('/')
                    .nth(4)
                    .unwrap()
                    .strip_prefix("page-")
                    .unwrap()
                    .parse()
                    .unwrap();
                let contents = contents.lock().unwrap();
                let text = contents[&id];
                ResponseTemplate::new(200)
                    .set_body_string(format!(
                        "\u{feff}________________\r\n\r\n{}",
                        text.replace('\n', "\r\n")
                    ))
                    .set_delay(delay)
            })
            .mount(&server)
            .await;
        let deletes = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&deletes);
        Mock::given(method("DELETE"))
            .and(path_regex(r"^/drive/v3/files/page-\d+$"))
            .respond_with(move |_: &wiremock::Request| {
                count.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(204)
            })
            .mount(&server)
            .await;
        Self {
            server,
            uploads,
            deletes,
        }
    }

    pub fn config(&self) -> DriveConfig {
        let mut config = drive_config(&keys()).unwrap();
        config.api_base = self.server.uri();
        config.upload_base = format!("{}/upload", self.server.uri());
        config.token_url = format!("{}/token", self.server.uri());
        config.max_attempts = 1;
        config.request_timeout = Duration::from_secs(10);
        config.concurrency_per_account = 2;
        config
    }

    pub fn assert_clean(&self) {
        assert_eq!(
            self.uploads.load(Ordering::SeqCst),
            self.deletes.load(Ordering::SeqCst)
        );
    }
}

pub fn png() -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    image::GrayImage::from_pixel(32, 32, image::Luma([255]))
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

/// Small valid PDFs with any page count, independent of PDF authoring dependencies.
pub fn pdf(pages: usize) -> Vec<u8> {
    let mut objects = vec!["<< /Type /Catalog /Pages 2 0 R >>".to_owned()];
    let kids = (0..pages)
        .map(|i| format!("{} 0 R", i + 3))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!("<< /Type /Pages /Kids [{kids}] /Count {pages} >>"));
    for _ in 0..pages {
        objects
            .push("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 72 72] /Resources << >> >>".into());
    }
    let mut output = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(output.len());
        writeln!(output, "{} 0 obj\n{object}\nendobj", index + 1).unwrap();
    }
    let xref = output.len();
    writeln!(output, "xref\n0 {}\n0000000000 65535 f ", objects.len() + 1).unwrap();
    for offset in offsets {
        writeln!(output, "{offset:010} 00000 n ").unwrap();
    }
    writeln!(
        output,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF",
        objects.len() + 1
    )
    .unwrap();
    output
}
