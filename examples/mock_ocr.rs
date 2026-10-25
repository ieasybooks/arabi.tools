//! Runs the full site with local Drive and Wit mocks for browser tests. Start `mock_wit` first.
use anyhow::Context;
use arlang_tools::{
    app,
    config::Config,
    tools::{ocr::Ocr, transcription::Transcription},
};
use std::time::Duration;
use tafrigh_wit::WitConfig;
use topcoat::{asset::AssetBundle, router::RouterService};

#[allow(dead_code)]
#[path = "../tests/support/drive.rs"]
mod drive;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let drive = drive::Drive::start(Duration::from_millis(750)).await;
    let ocr = Ocr::new(
        drive.config(),
        std::env::var_os("TAHWEEL_TEST_PDFIUM").map(Into::into),
    )
    .await?;
    ocr.queue.spawn_sweeper();
    let mut wit = WitConfig::new(["dev"]);
    wit.api_url = "http://127.0.0.1:9804".into();
    let transcription = Transcription::new(vec![("ar".into(), wit)]).await?;
    transcription.queue.spawn_sweeper();
    let config = Config {
        host: "127.0.0.1".into(),
        port: 3204,
        wit_tokens: vec![],
        wit_api_url: None,
        client_ip_header: None,
        drive: None,
        pdfium_library: None,
    };
    // The example serves the same app assets as the normal binary. Topcoat's bundler
    // accepts binary targets, so load that bundle beside the examples directory.
    let executable = std::env::current_exe()?;
    let assets = AssetBundle::load_dir(
        executable
            .parent()
            .context("example directory")?
            .join("../assets"),
    )?;
    let router = app::router(&config, transcription.clone(), ocr.clone(), assets)?;
    let listener = tokio::net::TcpListener::bind((config.host.as_str(), config.port)).await?;
    topcoat::serve_until(
        listener,
        RouterService::new(router).shutdown_timeout(Duration::from_secs(1)),
        async {
            let _ = tokio::signal::ctrl_c().await;
            tokio::join!(
                transcription.queue.shutdown(Duration::ZERO),
                ocr.queue.shutdown(Duration::ZERO)
            );
        },
    )
    .await?;
    drive.assert_clean();
    Ok(())
}
