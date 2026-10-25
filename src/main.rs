use std::time::Duration;

use anyhow::Context;
use arlang_tools::app;
use arlang_tools::config::Config;
use arlang_tools::tools::ocr::Ocr;
use arlang_tools::tools::transcription::Transcription;
use tafrigh_wit::WitConfig;
use tokio::net::TcpListener;
use topcoat::asset::AssetBundle;
use topcoat::router::RouterService;
use tracing_subscriber::EnvFilter;

/// How long running jobs get to finish when the server is asked to stop. Docker waits a
/// little longer before killing the container (see `compose.yaml`).
const SHUTDOWN_GRACE: Duration = Duration::from_secs(270);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env()?;
    let languages = config
        .wit_tokens
        .iter()
        .map(|wit| {
            let mut wit_config = WitConfig::new(wit.tokens.clone());
            if let Some(url) = &config.wit_api_url {
                wit_config.api_url.clone_from(url);
            }
            (wit.language.clone(), wit_config)
        })
        .collect();
    let transcription = Transcription::new(languages).await?;
    transcription.queue.spawn_sweeper();

    let ocr = Ocr::unavailable().with_pdfium_library(config.pdfium_library.clone());
    ocr.queue.spawn_sweeper();

    let assets =
        AssetBundle::load().context("loading the asset bundle (run `topcoat asset bundle`)")?;
    let router = app::router(&config, transcription.clone(), ocr.clone(), assets)?;
    let listener = TcpListener::bind((config.host.as_str(), config.port))
        .await
        .with_context(|| format!("listening on {}:{}", config.host, config.port))?;
    tracing::info!("listening on http://{}", listener.local_addr()?);

    let initialization = tokio::spawn({
        let ocr = ocr.clone();
        let drive = config.drive.clone();
        async move {
            if let Some(drive) = drive
                && let Err(error) = ocr.initialize(drive).await
            {
                tracing::warn!("OCR unavailable: {error:#}");
            }
        }
    });

    // Status pages hold their connections open; once jobs are done, close them quickly.
    let service = RouterService::new(router).shutdown_timeout(Duration::from_secs(5));
    topcoat::serve_until(listener, service, async move {
        shutdown_signal().await;
        tracing::info!("stopping: no new jobs, letting running ones finish");
        tokio::join!(
            transcription.queue.shutdown(SHUTDOWN_GRACE),
            // Stop new OCR pages immediately and let active Drive calls clean up.
            ocr.queue.shutdown(Duration::ZERO),
            // The startup probe also gets to finish deleting its Drive document.
            async {
                let _ = initialization.await;
            },
        );
    })
    .await?;
    Ok(())
}

/// Resolves on Ctrl+C or, on Unix, `SIGTERM`.
async fn shutdown_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = interrupt => {}
        () = terminate => {}
    }
}
