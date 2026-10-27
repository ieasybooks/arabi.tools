//! A small client for the application's documented Topcoat socket protocol.
//! Tests send single text frames and reject oversized or fragmented replies.

use anyhow::{Context, ensure};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub struct RuntimeSocket(TcpStream);

impl RuntimeSocket {
    pub async fn open(url: &str) -> anyhow::Result<Self> {
        let url = reqwest::Url::parse(url)?;
        let host = url.host_str().context("host")?;
        let port = url.port_or_known_default().context("port")?;
        let mut stream = TcpStream::connect((host, port)).await?;
        stream.write_all(format!(
            "GET / HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: topcoat-runtime\r\n\r\n"
        ).as_bytes()).await?;
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            ensure!(headers.len() < 8192, "oversized handshake");
            headers.push(stream.read_u8().await?);
        }
        ensure!(
            headers.starts_with(b"HTTP/1.1 101"),
            "{}",
            String::from_utf8_lossy(&headers)
        );
        Ok(Self(stream))
    }

    pub async fn observe(&mut self, path: &str, job: &str) -> anyhow::Result<()> {
        let message = json!({
            "run": 1, "method": "POST", "path": path,
            "headers": { "Content-Type": "application/json" },
            "body": json!({"args": [job], "signals": {}}).to_string(),
        })
        .to_string();
        let bytes = message.as_bytes();
        ensure!(
            bytes.len() <= usize::from(u16::MAX),
            "oversized test request"
        );
        self.0.write_u8(0x81).await?;
        if bytes.len() < 126 {
            self.0.write_u8(0x80 | u8::try_from(bytes.len())?).await?;
        } else {
            self.0.write_u8(0xfe).await?;
            self.0.write_u16(u16::try_from(bytes.len())?).await?;
        }
        let mask = [1, 2, 3, 4];
        self.0.write_all(&mask).await?;
        let masked: Vec<_> = bytes
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ mask[i % 4])
            .collect();
        self.0.write_all(&masked).await?;
        Ok(())
    }

    pub async fn next(&mut self) -> anyhow::Result<Value> {
        ensure!(
            self.0.read_u8().await? == 0x81,
            "expected a single text frame"
        );
        let length = match self.0.read_u8().await? {
            126 => u64::from(self.0.read_u16().await?),
            127 => self.0.read_u64().await?,
            length => u64::from(length),
        };
        ensure!(length < 2_000_000, "oversized test response");
        let mut bytes = vec![0; usize::try_from(length)?];
        self.0.read_exact(&mut bytes).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn until(&mut self, text: &str) -> anyhow::Result<String> {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let frame = self.next().await?;
                ensure!(frame["frame"]["t"] != "error", "{frame}");
                let message = frame.to_string();
                if message.contains(text) {
                    return Ok(message);
                }
            }
        })
        .await?
    }
}
