//! Settings read from the environment.

use std::{env, path::PathBuf};

use crate::tools::ocr::drive_config;
use tahweel_drive::DriveConfig;

use anyhow::{Context, bail};
use topcoat::router::HeaderName;

/// Prefix of the variables holding Wit tokens, one per language: `WIT_TOKENS_AR=a,b`.
const WIT_TOKENS_PREFIX: &str = "WIT_TOKENS_";

/// The language listed first, and so the default, when it is configured.
const PREFERRED_LANGUAGE: &str = "ar";

#[derive(Debug)]
pub struct Config {
    /// Address to listen on (`HOST`, default `127.0.0.1`).
    pub host: String,
    /// Port to listen on (`PORT`, default `3000`).
    pub port: u16,
    /// Wit client access tokens per transcription language, preferred language first.
    pub wit_tokens: Vec<WitTokens>,
    /// Base URL of the Wit API (`WIT_API_URL`), e.g. a local mock. Unset uses Wit itself.
    pub wit_api_url: Option<String>,
    /// Header carrying the visitor's address, set by the one proxy in front of the app
    /// (`CLIENT_IP_HEADER`, e.g. `CF-Connecting-IP`). Unset uses the connection's address.
    pub client_ip_header: Option<HeaderName>,
    /// OCR credentials parsed independently; invalid or missing settings disable only OCR.
    pub drive: Option<DriveConfig>,
    /// Optional `PDFium` path for local development (`TAHWEEL_PDFIUM`).
    pub pdfium_library: Option<PathBuf>,
    /// Pre-provisioned diacritization model cache (`TASHKEEL_MODEL_CACHE`).
    pub tashkeel_model_cache: Option<PathBuf>,
    /// Optional explicit native runtime (`ORT_DYLIB_PATH`).
    pub onnxruntime_library: Option<PathBuf>,
}

/// The Wit tokens of one language.
pub struct WitTokens {
    /// Language code, e.g. `ar`.
    pub language: String,
    pub tokens: Vec<String>,
}

impl std::fmt::Debug for WitTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WitTokens")
            .field("language", &self.language)
            .field("tokens", &format_args!("[{} redacted]", self.tokens.len()))
            .finish()
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
        let port = match env::var("PORT") {
            Ok(port) => port.parse().context("PORT must be a port number")?,
            Err(_) => 3000,
        };
        let client_ip_header = env::var("CLIENT_IP_HEADER")
            .ok()
            .filter(|name| !name.trim().is_empty())
            .map(|name| HeaderName::try_from(name.trim()))
            .transpose()
            .context("CLIENT_IP_HEADER must be a header name")?;
        Ok(Self {
            host,
            port,
            wit_tokens: wit_tokens(env::vars())?,
            wit_api_url: env::var("WIT_API_URL").ok().filter(|url| !url.is_empty()),
            client_ip_header,
            drive: env::var("DRIVE_SERVICE_ACCOUNTS").ok().and_then(|json| {
                match drive_config(&json) {
                    Ok(config) => Some(config),
                    Err(error) => {
                        tracing::warn!("OCR unavailable: {error}");
                        None
                    }
                }
            }),
            pdfium_library: env::var_os("TAHWEEL_PDFIUM").map(PathBuf::from),
            tashkeel_model_cache: env::var_os("TASHKEEL_MODEL_CACHE")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
            onnxruntime_library: env::var_os("ORT_DYLIB_PATH")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
        })
    }
}

fn wit_tokens(vars: impl Iterator<Item = (String, String)>) -> anyhow::Result<Vec<WitTokens>> {
    let mut languages: Vec<WitTokens> = vars
        .filter_map(|(name, value)| {
            let language = name.strip_prefix(WIT_TOKENS_PREFIX)?.to_ascii_lowercase();
            let tokens = value
                .split(',')
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(str::to_owned)
                .collect();
            Some(WitTokens { language, tokens })
        })
        .collect();
    if let Some(empty) = languages.iter().find(|wit| wit.tokens.is_empty()) {
        bail!(
            "{WIT_TOKENS_PREFIX}{} has no tokens",
            empty.language.to_uppercase()
        );
    }
    if languages.is_empty() {
        bail!("set at least one {WIT_TOKENS_PREFIX}<LANGUAGE>, e.g. {WIT_TOKENS_PREFIX}AR=token");
    }
    languages.sort_by(|a, b| {
        (a.language != PREFERRED_LANGUAGE, &a.language)
            .cmp(&(b.language != PREFERRED_LANGUAGE, &b.language))
    });
    Ok(languages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> impl Iterator<Item = (String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn reads_tokens_per_language_with_arabic_first() {
        let languages = wit_tokens(vars(&[
            ("PATH", "/bin"),
            ("WIT_TOKENS_EN", "e1"),
            ("WIT_TOKENS_AR", " a1, ,a2 "),
        ]))
        .unwrap();
        let summary: Vec<_> = languages
            .iter()
            .map(|wit| (wit.language.as_str(), wit.tokens.clone()))
            .collect();
        assert_eq!(
            summary,
            [
                ("ar", vec!["a1".to_owned(), "a2".to_owned()]),
                ("en", vec!["e1".to_owned()]),
            ]
        );
    }

    #[test]
    fn requires_tokens() {
        assert!(wit_tokens(vars(&[("PATH", "/bin")])).is_err());
        assert!(wit_tokens(vars(&[("WIT_TOKENS_AR", " , ")])).is_err());
    }

    #[test]
    fn redacts_tokens() {
        let debug = format!(
            "{:?}",
            WitTokens {
                language: "ar".into(),
                tokens: vec!["secret".into()],
            }
        );
        assert!(!debug.contains("secret"), "{debug}");
    }
}
