//! Audio transcription with Wit, through tafrigh.

use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context, bail};
use serde::Deserialize;
use tafrigh::audio::Decoder;
use tafrigh::output::{BuiltinFormat, OutputWriter};
use tafrigh::segment::silero::{SileroConfig, SileroSegmenter};
use tafrigh::{ChunkOptions, ChunkedTranscriber, Segment, TranscribeEvent, Transcriber};
use tafrigh_wit::{WitConfig, WitRecognizer};
use tempfile::TempPath;

use crate::queue::{Limits, Queue, Rejection, Upload};

/// Largest upload accepted. Decimal, to stay under Cloudflare's 100 MB request limit with
/// room for the form around the file.
pub const MAX_UPLOAD_BYTES: u64 = 99_000_000;
/// Longest recording accepted.
pub const MAX_DURATION: Duration = Duration::from_hours(3);
/// File extensions accepted, lowercase.
pub const EXTENSIONS: [&str; 2] = ["mp3", "m4a"];
/// How long finished transcripts are kept.
pub const KEEP_FINISHED: Duration = Duration::from_mins(30);

/// Longest a probe of an uploaded file may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

pub fn limits() -> Limits {
    Limits {
        jobs: 100,
        running: 4,
        // Proxies such as Cloudflare may hold a slow upload until it is complete.
        upload_start: Duration::from_mins(30),
        unwatched: Duration::from_mins(1),
        keep_finished: KEEP_FINISHED,
    }
}

/// The transcription tool: its languages and its job queue. Clones share both.
#[derive(Clone)]
pub struct Transcription {
    pub queue: Queue<Transcript>,
    languages: Arc<[Language]>,
}

/// A language that recordings can be transcribed in.
pub struct Language {
    /// Wit's language code, e.g. `ar`.
    pub code: String,
    transcriber: Arc<ChunkedTranscriber>,
}

/// A finished transcription.
pub struct Transcript {
    /// Name for downloaded files, without an extension.
    pub name: String,
    /// Code of the language it is in, e.g. `ar`.
    pub language: String,
    /// Plain text, one segment per line.
    pub text: String,
    /// SRT subtitles.
    pub srt: String,
}

/// Why an uploaded file was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioProblem {
    /// The form carried no file.
    Missing,
    /// The file name does not end in an accepted extension.
    WrongType,
    /// The file is larger than [`MAX_UPLOAD_BYTES`].
    TooLarge,
    /// The recording is longer than [`MAX_DURATION`].
    TooLong,
    /// ffprobe found no MP3 or M4A audio in the file.
    Unreadable,
}

impl Transcription {
    /// Sets up one transcriber per language, checking with Wit that each language's tokens
    /// belong to apps of that language.
    pub async fn new(languages: Vec<(String, WitConfig)>) -> anyhow::Result<Self> {
        let decoder = Decoder::default();
        decoder.check().await.context("ffmpeg is required")?;
        let segmenter = SileroSegmenter::new(SileroConfig::default())?;
        let mut configured = Vec::new();
        for (code, wit) in languages {
            let recognizer = WitRecognizer::new(wit)
                .await
                .with_context(|| format!("checking the Wit tokens for `{code}`"))?;
            if !recognizer.language().eq_ignore_ascii_case(&code) {
                bail!(
                    "the Wit tokens for `{code}` belong to `{}` apps",
                    recognizer.language()
                );
            }
            let transcriber =
                ChunkedTranscriber::new(segmenter.clone(), recognizer, ChunkOptions::default())?
                    .with_decoder(decoder.clone());
            configured.push(Language {
                code,
                transcriber: Arc::new(transcriber),
            });
        }
        Ok(Self {
            queue: Queue::new(limits()),
            languages: configured.into(),
        })
    }

    /// Configured languages, the default first.
    pub fn languages(&self) -> &[Language] {
        &self.languages
    }

    /// The language with `code`, or the default one when `code` is `None`.
    pub fn language(&self, code: Option<&str>) -> Option<&Language> {
        match code {
            Some(code) => self.languages.iter().find(|language| language.code == code),
            None => self.languages.first(),
        }
    }
}

impl Language {
    /// Queues `audio` for transcription in this language. The file is deleted as soon as the job
    /// ends, however it ends.
    pub fn submit(
        &self,
        upload: Upload<Transcript>,
        audio: TempPath,
        name: String,
    ) -> Result<(), Rejection> {
        let transcriber = Arc::clone(&self.transcriber);
        let language = self.code.clone();
        upload.submit(move |progress| async move {
            let tracker = Mutex::new(Tracker::default());
            let report = |event: TranscribeEvent| {
                let mut tracker = tracker.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(percent) = tracker.update(&event) {
                    progress.set(percent);
                }
            };
            let segments = transcriber.transcribe(&audio, &report).await?;
            drop(audio);
            render(name, language, &segments)
        })
    }
}

/// Whether `file_name` ends in one of the accepted [`EXTENSIONS`].
pub fn has_accepted_extension(file_name: &str) -> bool {
    Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|accepted| extension.eq_ignore_ascii_case(accepted))
        })
}

/// The name to give downloads of the transcript of `file_name`: its stem, cleaned of
/// characters that do not belong in a file name.
pub fn download_name(file_name: &str) -> String {
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
        "transcript".to_owned()
    } else {
        name.to_owned()
    }
}

/// Checks with ffprobe that `path` holds MP3 or M4A audio no longer than [`MAX_DURATION`].
pub async fn check_audio(path: &Path) -> Result<(), AudioProblem> {
    #[derive(Deserialize)]
    struct Probe {
        format: Format,
        #[serde(default)]
        streams: Vec<Stream>,
    }
    #[derive(Deserialize)]
    struct Format {
        format_name: String,
        duration: Option<String>,
    }
    #[derive(Deserialize)]
    struct Stream {
        codec_type: Option<String>,
    }

    let output = tokio::process::Command::new("ffprobe")
        .args(["-v", "error", "-of", "json"])
        .args([
            "-show_entries",
            "format=format_name,duration:stream=codec_type",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(PROBE_TIMEOUT, output)
        .await
        .ok()
        .and_then(Result::ok)
        .filter(|output| output.status.success())
        .ok_or(AudioProblem::Unreadable)?;
    let probe: Probe =
        serde_json::from_slice(&output.stdout).map_err(|_| AudioProblem::Unreadable)?;
    let known_format = probe
        .format
        .format_name
        .split(',')
        .any(|name| EXTENSIONS.contains(&name));
    let has_audio = probe
        .streams
        .iter()
        .any(|stream| stream.codec_type.as_deref() == Some("audio"));
    let duration = probe
        .format
        .duration
        .and_then(|duration| duration.parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0);
    match duration {
        Some(seconds) if known_format && has_audio => {
            if seconds > MAX_DURATION.as_secs_f64() {
                Err(AudioProblem::TooLong)
            } else {
                Ok(())
            }
        }
        _ => Err(AudioProblem::Unreadable),
    }
}

/// Writes the transcript files the site offers and reads them back.
fn render(name: String, language: String, segments: &[Segment]) -> anyhow::Result<Transcript> {
    let dir = tempfile::tempdir()?;
    let written = OutputWriter::new()
        .formats([BuiltinFormat::Txt, BuiltinFormat::Srt])
        .write(dir.path(), "transcript", segments)?;
    let [text, srt] = written.files.as_slice() else {
        bail!("expected a text and an SRT file, got {:?}", written.files);
    };
    Ok(Transcript {
        name,
        language,
        text: std::fs::read_to_string(text)?,
        srt: std::fs::read_to_string(srt)?,
    })
}

/// Estimates a transcription's progress from tafrigh's events.
#[derive(Debug, Default)]
struct Tracker {
    duration: Option<f64>,
    decoded: f64,
    queued: usize,
    recognized: usize,
    chunks: Option<usize>,
    percent: u8,
}

impl Tracker {
    /// Takes in `event`; returns the new percentage when the estimate went up.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn update(&mut self, event: &TranscribeEvent) -> Option<u8> {
        match *event {
            TranscribeEvent::Started { duration } => self.duration = duration,
            TranscribeEvent::Decoded { seconds } => self.decoded = seconds,
            TranscribeEvent::ChunkQueued { .. } => self.queued += 1,
            TranscribeEvent::ChunkRecognized { .. } | TranscribeEvent::ChunkSkipped { .. } => {
                self.recognized += 1;
            }
            TranscribeEvent::Segmented { chunks } => self.chunks = Some(chunks),
            _ => {}
        }
        let total = match (self.chunks, self.duration) {
            (Some(chunks), _) => chunks as f64,
            // Until decoding ends, expect speech to continue at the rate found so far.
            (None, Some(duration)) if self.decoded > 0.0 => {
                self.queued as f64 * duration / self.decoded
            }
            _ => return None,
        };
        if total < 1.0 {
            return None;
        }
        // 100 means the transcript is ready, which only the finished job shows.
        let percent = (self.recognized as f64 / total * 100.0).min(99.0) as u8;
        (percent > self.percent).then(|| {
            self.percent = percent;
            percent
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_mp3_and_m4a_names() {
        assert!(has_accepted_extension("lesson.mp3"));
        assert!(has_accepted_extension("درس.M4A"));
        assert!(!has_accepted_extension("lesson.wav"));
        assert!(!has_accepted_extension("mp3"));
    }

    #[test]
    fn cleans_download_names() {
        assert_eq!(download_name("درس الفقه.mp3"), "درس الفقه");
        assert_eq!(download_name("a\"b/c.m4a"), "c");
        assert_eq!(download_name("we\"ird:name.mp3"), "weirdname");
        assert_eq!(download_name("  .m4a"), "transcript");
    }

    #[test]
    fn progress_rises_with_recognized_chunks() {
        let mut tracker = Tracker::default();
        assert_eq!(
            tracker.update(&TranscribeEvent::Started {
                duration: Some(100.0)
            }),
            None
        );
        tracker.update(&TranscribeEvent::Decoded { seconds: 50.0 });
        for index in 0..5 {
            tracker.update(&TranscribeEvent::ChunkQueued {
                index,
                start: 0.0,
                end: 1.0,
            });
        }
        // 5 chunks in the first half suggest 10 in total; 1 recognized is 10%.
        let recognized = TranscribeEvent::ChunkRecognized {
            index: 0,
            start: 0.0,
            end: 1.0,
            empty: false,
        };
        assert_eq!(tracker.update(&recognized), Some(10));
        tracker.update(&TranscribeEvent::Segmented { chunks: 4 });
        assert_eq!(tracker.update(&recognized), Some(50));
        for _ in 0..3 {
            tracker.update(&recognized);
        }
        assert_eq!(tracker.percent, 99, "100 is kept for the finished job");
    }
}
