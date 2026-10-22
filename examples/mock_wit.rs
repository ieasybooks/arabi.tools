//! Run these in separate terminals (ffmpeg and topcoat must be on PATH):
//!
//! ```sh
//! cargo run --example mock_wit -- 9876
//! WIT_API_URL=http://127.0.0.1:9876 WIT_TOKENS_AR=dev topcoat dev
//! ```
//!
//! The port defaults to 9876. Set `MOCK_WIT_DELAY_MS` on the first command to change the delay per chunk (default 1500 ms), for example `MOCK_WIT_DELAY_MS=5000`.

use std::net::TcpListener;
use std::time::Duration;

use anyhow::Context;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port: u16 = std::env::args()
        .nth(1)
        .map_or(Ok(9876), |port| port.parse())
        .context("port must be a number from 0 to 65535")?;
    let delay = std::env::var("MOCK_WIT_DELAY_MS")
        .map_or(Ok(1500), |delay| delay.parse::<u64>())
        .context("MOCK_WIT_DELAY_MS must be a nonnegative integer")?;
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let server = MockServer::builder()
        .listener(listener)
        .disable_request_recording()
        .start()
        .await;
    Mock::given(method("GET"))
        .and(path("/apps"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"name": "Local Arabic app", "lang": "ar", "is_app_for_token": true}
        ])))
        .mount(&server)
        .await;
    // tafrigh adds 500 ms of silence before each chunk's speech.
    Mock::given(method("POST"))
        .and(path("/dictation"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({
                    "type": "FINAL_TRANSCRIPTION",
                    "is_final": true,
                    "text": "مرحبا بالعالم",
                    "speech": {"confidence": 0.99, "tokens": [
                        {"token": "مرحبا", "start": 500, "end": 600, "confidence": 0.99},
                        {"token": "بالعالم", "start": 600, "end": 700, "confidence": 0.99}
                    ]}
                }))
                .set_delay(Duration::from_millis(delay)),
        )
        .mount(&server)
        .await;
    println!(
        "Mock Wit listening at {} ({delay} ms per chunk). Press Ctrl+C to stop.",
        server.uri()
    );
    tokio::signal::ctrl_c().await?;
    Ok(())
}
