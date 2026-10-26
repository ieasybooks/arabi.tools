//! The tools the site offers. Each one owns its limits, its job queue and its work; the
//! queues share the running slots. The pages that use a tool live under `src/app/`.

pub mod ocr;
pub mod transcription;

use crate::queue::Limits;
use std::{path::Path, time::Duration};

/// Largest upload accepted. Decimal, to stay under Cloudflare's 100 MB request limit with
/// room for the form around the file.
pub const MAX_UPLOAD_BYTES: u64 = 99_000_000;
/// How long finished results are kept.
pub const KEEP_FINISHED: Duration = Duration::from_mins(30);
/// Jobs running at once across all tools.
pub const RUNNING_JOBS: usize = 4;

pub fn limits() -> Limits {
    Limits {
        jobs: 100,
        // Fewer than `RUNNING_JOBS`, so a busy tool always leaves a slot for another one.
        running: 3,
        // Proxies such as Cloudflare may hold a slow upload until it is complete.
        upload_start: Duration::from_mins(30),
        unwatched: Duration::from_mins(1),
        keep_finished: KEEP_FINISHED,
    }
}

/// The name for downloads of `file_name`: its stem, cleaned of
/// characters that do not belong in a file name.
fn download_name(file_name: &str, fallback: &str) -> String {
    const MAX_CHARS: usize = 100;
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let name: String = stem
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '/' | '\\' | '"' | ':' | '*' | '?' | '<' | '>' | '|')
        })
        .take(MAX_CHARS)
        .collect();
    let name = name.trim();
    if name.is_empty() {
        fallback.to_owned()
    } else {
        name.to_owned()
    }
}
