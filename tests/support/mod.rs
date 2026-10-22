use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use anyhow::{Context, bail};
use arlang_tools::app;
use arlang_tools::config::Config;
use arlang_tools::queue::{JobId, Limits, Queue, Upload};
use arlang_tools::tools::transcription::{self, Transcript, Transcription};
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

pub const ARABIC_TEXT: &str = "مرحبا بالعالم";
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

pub struct TestApp {
    pub client: Client,
    pub transcription: Transcription,
    pub wit: MockServer,
    base_url: String,
    shutdown: Option<oneshot::Sender<()>>,
    server: JoinHandle<std::io::Result<()>>,
    _assets: Arc<Assets>,
}

impl TestApp {
    pub async fn start() -> anyhow::Result<Self> {
        Self::start_with_limits(transcription::limits()).await
    }

    pub async fn start_with_limits(limits: Limits) -> anyhow::Result<Self> {
        let wit = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/apps"))
            .and(header("authorization", "Bearer dev"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"name": "Arabic test app", "lang": "ar", "is_app_for_token": true}
            ])))
            .expect(1)
            .mount(&wit)
            .await;
        // Word timings start after tafrigh's 500 ms of leading silence.
        Mock::given(method("POST"))
            .and(path("/dictation"))
            .and(header("authorization", "Bearer dev"))
            .and(header("content-type", "audio/wav"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({
                        "type": "FINAL_TRANSCRIPTION",
                        "is_final": true,
                        "text": ARABIC_TEXT,
                        "speech": {"confidence": 0.99, "tokens": [
                            {"token": "مرحبا", "start": 500, "end": 600, "confidence": 0.99},
                            {"token": "بالعالم", "start": 600, "end": 700, "confidence": 0.99}
                        ]}
                    }))
                    .set_delay(Duration::from_millis(25)),
            )
            .mount(&wit)
            .await;

        let mut wit_config = WitConfig::new(["dev"]);
        wit_config.api_url = wit.uri();
        wit_config.requests_per_second = 1000.0;
        wit_config.max_attempts = 2;
        wit_config.retry_base_delay = Duration::from_millis(5);
        wit_config.retry_max_delay = Duration::from_millis(20);
        wit_config.request_timeout = Duration::from_secs(5);
        let mut transcription = Transcription::new(vec![("ar".into(), wit_config)]).await?;
        transcription.queue = Queue::new(limits);
        let config = Config {
            host: "127.0.0.1".into(),
            port: 0,
            wit_tokens: vec![],
            wit_api_url: Some(wit.uri()),
            client_ip_header: None,
        };
        let assets = tokio::task::spawn_blocking(Assets::load).await??;
        let router = app::router(&config, transcription.clone(), assets.bundle.clone())?;
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
        let id: JobId = job.rsplit('/').next().context("missing job id")?.parse()?;
        let mut watch = self.transcription.queue.watch(&id).context("missing job")?;
        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if watch.status().is_finished() {
                    if self.transcription.queue.result(&id).is_some() {
                        return Ok(());
                    }
                    bail!("job {id} failed or was cancelled");
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
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        tokio::time::timeout(Duration::from_secs(5), &mut self.server).await???;
        self.wit.verify().await;
        Ok(())
    }
}

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
