use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use anyhow::{Context, bail};
use arlang_tools::app;
use arlang_tools::config::Config;
use arlang_tools::queue::{JobId, Limits, Queue, Slots, Status, Upload};
use arlang_tools::tools::RUNNING_JOBS;
use arlang_tools::tools::ocr::Ocr;
use arlang_tools::tools::transcription::{self, MAX_DURATION, Transcript, Transcription};
use reqwest::{Client, Response, StatusCode, multipart};
use serde_json::{Value, json};
use tafrigh_wit::WitConfig;
use tempfile::TempDir;
use test_assets::{Bundler, BundlerConfig};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use topcoat::asset::AssetBundle;
use topcoat::router::RouterService;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The default Arabic app of [`TestApp::start`].
const ARABIC: (&str, &str, &str) = ("ar", "dev", ARABIC_TEXT);
pub const ARABIC_TEXT: &str = "مرحبا بالعالم";
pub const ENGLISH_TEXT: &str = "Hello world";
pub const MP3: &[u8] = include_bytes!("../fixtures/speech.mp3");
pub const M4A: &[u8] = include_bytes!("../fixtures/speech.m4a");
pub const UNKNOWN_JOB: &str = "/transcribe/jobs/00000000000000000000000000000000";
/// The upload attempt of [`TestApp::reserve`], as one browser tab would send it.
pub const ATTEMPT: &str = "a77e3b7a1e5d4c0f9b2a6c8d0e1f2a3b";

struct Assets {
    bundle: AssetBundle,
    _directory: TempDir,
}

impl Assets {
    fn load() -> anyhow::Result<Arc<Self>> {
        // Scan the large test executable once while tests overlap. The last test
        // drops the bundle directory; the cache itself does not keep it alive.
        static CACHE: Mutex<Weak<Assets>> = Mutex::new(Weak::new());
        let mut cache = CACHE.lock().expect("asset cache lock poisoned");
        if let Some(assets) = cache.upgrade() {
            return Ok(assets);
        }
        let directory = tempfile::tempdir()?;
        let config = BundlerConfig::new().cache_dir(directory.path().join("cache"));
        let binary = std::fs::read(std::env::current_exe()?)?;
        Bundler::new(&config).bundle(&binary, directory.path())?;
        let assets = Arc::new(Self {
            bundle: AssetBundle::load_dir(directory.path())?,
            _directory: directory,
        });
        *cache = Arc::downgrade(&assets);
        Ok(assets)
    }
}

/// How a [`TestApp`] differs from the site's own setup; `Setup::default()` is the same.
pub struct Setup<'a> {
    /// `(language, token, text)` per Wit app, where `text` is what every dictation request
    /// with that token returns.
    pub languages: &'a [(&'a str, &'a str, &'a str)],
    pub limits: Limits,
    /// Longest decoded audio a job may have.
    pub max_duration: Duration,
}

impl Default for Setup<'_> {
    fn default() -> Self {
        Self {
            languages: &[ARABIC],
            limits: transcription::limits(),
            max_duration: MAX_DURATION,
        }
    }
}

pub struct TestApp {
    pub client: Client,
    pub transcription: Transcription,
    pub wit: MockServer,
    pub ocr: Ocr,
    base_url: String,
    shutdown: Option<oneshot::Sender<()>>,
    server: JoinHandle<std::io::Result<()>>,
    _assets: Arc<Assets>,
}

impl TestApp {
    pub async fn start() -> anyhow::Result<Self> {
        Self::start_with(Setup::default()).await
    }

    /// Starts the site against a Wit mock with one app per configured language.
    pub async fn start_with(setup: Setup<'_>) -> anyhow::Result<Self> {
        Self::start_with_ocr(setup, Ocr::unavailable(&Slots::new(RUNNING_JOBS))).await
    }

    pub async fn start_with_ocr(setup: Setup<'_>, ocr: Ocr) -> anyhow::Result<Self> {
        let wit = MockServer::start().await;
        let mut configured = Vec::new();
        for &(code, token, text) in setup.languages {
            let authorization = format!("Bearer {token}");
            Mock::given(method("GET"))
                .and(path("/apps"))
                .and(header("authorization", authorization.as_str()))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                    {"name": format!("{code} test app"), "lang": code, "is_app_for_token": true}
                ])))
                .expect(1)
                .mount(&wit)
                .await;
            successful_dictation(token, text).mount(&wit).await;

            let mut wit_config = WitConfig::new([token]);
            wit_config.api_url = wit.uri();
            wit_config.requests_per_second = 1000.0;
            wit_config.max_attempts = 2;
            wit_config.retry_base_delay = Duration::from_millis(5);
            wit_config.retry_max_delay = Duration::from_millis(20);
            wit_config.request_timeout = Duration::from_secs(5);
            configured.push((code.to_owned(), wit_config));
        }
        let slots = Slots::new(RUNNING_JOBS);
        let mut transcription =
            Transcription::new_with_max_duration(configured, setup.max_duration, &slots).await?;
        transcription.queue = Queue::new(setup.limits, &slots);
        let config = Config {
            host: "127.0.0.1".into(),
            port: 0,
            wit_tokens: vec![],
            wit_api_url: Some(wit.uri()),
            client_ip_header: None,
            drive: None,
            pdfium_library: None,
        };
        let assets = tokio::task::spawn_blocking(Assets::load).await??;
        let router = app::router(
            &config,
            transcription.clone(),
            ocr.clone(),
            assets.bundle.clone(),
        )?;
        let listener = TcpListener::bind((config.host.as_str(), config.port)).await?;
        let base_url = format!("http://{}", listener.local_addr()?);
        let (shutdown, stop) = oneshot::channel();
        let service = RouterService::new(router).shutdown_timeout(Duration::from_secs(1));
        let server = tokio::spawn(topcoat::serve_until(listener, service, async {
            let _ = stop.await;
        }));
        let client = Client::builder()
            .no_proxy()
            .cookie_store(true)
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            client,
            transcription,
            ocr,
            wit,
            base_url,
            shutdown: Some(shutdown),
            server,
            _assets: assets,
        })
    }

    pub fn url(&self, path: &str) -> String {
        assert!(path.starts_with('/') && !path.starts_with("//"), "{path}");
        format!("{}{path}", self.base_url)
    }

    /// Starts an upload from the test client's address and holds it open, so that address
    /// has a job in progress until the returned upload is dropped.
    pub fn hold_upload(&self) -> (JobId, Upload<Transcript>) {
        let queue = &self.transcription.queue;
        let id = queue
            .reserve(Some(IpAddr::V4(Ipv4Addr::LOCALHOST)), None)
            .expect("the queue has room for a reservation");
        let upload = queue
            .begin_upload(&id)
            .expect("a new reservation takes an upload");
        (id, upload)
    }

    /// Reserves a job like a browser tab whose upload attempt is [`ATTEMPT`].
    pub async fn reserve(&self) -> anyhow::Result<String> {
        let response = self
            .client
            .post(self.url("/transcribe/jobs"))
            .form(&[("attempt", ATTEMPT)])
            .send()
            .await?;
        let reply = json_reply(response, StatusCode::OK).await?;
        let upload = reply["upload"].as_str().context("missing upload URL")?;
        assert!(upload.starts_with("/transcribe/jobs/") && upload.ends_with("/audio"));
        assert_eq!(reply, json!({"upload": upload}));
        Ok(upload.to_owned())
    }

    pub async fn upload(
        &self,
        upload: &str,
        file_name: &str,
        bytes: &[u8],
        language: Option<&str>,
    ) -> anyhow::Result<Response> {
        let file = multipart::Part::bytes(bytes.to_vec()).file_name(file_name.to_owned());
        let mut form = multipart::Form::new().part("file", file);
        if let Some(language) = language {
            form = form.text("language", language.to_owned());
        }
        Ok(self
            .client
            .post(self.url(upload))
            .multipart(form)
            .send()
            .await?)
    }

    pub async fn wait_until_done(&self, job: &str) -> anyhow::Result<()> {
        if matches!(self.wait_until_finished(job).await?, Status::Done(_)) {
            Ok(())
        } else {
            bail!("job {job} failed or was cancelled")
        }
    }

    pub async fn wait_until_finished(&self, job: &str) -> anyhow::Result<Status<Transcript>> {
        let id: JobId = job.rsplit('/').next().context("missing job id")?.parse()?;
        let mut watch = self.transcription.queue.watch(&id).context("missing job")?;
        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                let status = watch.status();
                if status.is_finished() {
                    return Ok(status);
                }
                if !watch.changed().await {
                    bail!("job {id} disappeared before finishing");
                }
            }
        })
        .await
        .context("transcription timed out")?
    }

    pub async fn stop(mut self) -> anyhow::Result<()> {
        self.transcription.queue.shutdown(Duration::ZERO).await;
        self.ocr.queue.shutdown(Duration::ZERO).await;
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        tokio::time::timeout(Duration::from_secs(5), &mut self.server).await???;
        self.wit.verify().await;
        Ok(())
    }
}

pub fn successful_dictation(token: &str, text: &str) -> Mock {
    // Word timings start after tafrigh's 500 ms of leading silence.
    let tokens: Vec<_> = text
        .split_whitespace()
        .enumerate()
        .map(|(index, word)| {
            json!({
                "token": word,
                "start": 500 + index * 100,
                "end": 600 + index * 100,
                "confidence": 0.99
            })
        })
        .collect();
    Mock::given(method("POST"))
        .and(path("/dictation"))
        .and(header("authorization", format!("Bearer {token}").as_str()))
        .and(header("content-type", "audio/opus"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({
                    "type": "FINAL_TRANSCRIPTION",
                    "is_final": true,
                    "text": text,
                    "speech": {"confidence": 0.99, "tokens": tokens}
                }))
                .set_delay(Duration::from_millis(25)),
        )
}

// Optional fixed ports let a developer share a machine without leaving their allocation.
// Run with RUST_TEST_THREADS=1 when using them; CI uses ephemeral ports by default.
impl Drop for TestApp {
    fn drop(&mut self) {
        // Also stop the listener if an assertion fails before normal teardown.
        self.server.abort();
    }
}

pub async fn json_reply(response: Response, expected: StatusCode) -> anyhow::Result<Value> {
    let status = response.status();
    let content_type = response.headers()["content-type"].to_str()?.to_owned();
    let body = response.text().await?;
    assert_eq!(status, expected, "{body}");
    assert!(content_type.starts_with("application/json"), "{body}");
    serde_json::from_str(&body).context("invalid JSON reply")
}

pub async fn assert_error(
    response: Response,
    status: StatusCode,
    message: &str,
) -> anyhow::Result<()> {
    assert_eq!(
        json_reply(response, status).await?,
        json!({"error": message})
    );
    Ok(())
}
