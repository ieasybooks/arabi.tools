//! Audio transcription with Wit, through tafrigh.

use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context, bail, ensure};
use serde::Deserialize;
use tafrigh::audio::Decoder;
use tafrigh::output::{BuiltinFormat, OutputWriter};
use tafrigh::segment::silero::{SileroConfig, SileroSegmenter};
use tafrigh::{ChunkOptions, ChunkedTranscriber, Segment, TranscribeEvent, Transcriber};
use tafrigh_wit::{WitConfig, WitRecognizer};
use tempfile::TempPath;
use tokio::sync::watch;

use crate::queue::{Queue, Rejection, Slots, Upload};

pub use super::{KEEP_FINISHED, MAX_UPLOAD_BYTES, limits};

/// Longest recording accepted.
pub const MAX_DURATION: Duration = Duration::from_hours(3);
/// File extensions accepted, lowercase.
pub const EXTENSIONS: [&str; 2] = ["mp3", "m4a"];
/// Longest a probe of an uploaded file may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

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
    max_duration: Duration,
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
    /// The subtitles' cues, in order.
    pub cues: Vec<Cue>,
}

/// One cue of the SRT subtitles: when it is said, and what.
pub struct Cue {
    /// SRT timestamps, such as `00:01:02,500`.
    pub start: String,
    pub end: String,
    pub text: String,
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
    /// belong to apps of that language. Jobs run in the shared `slots`.
    pub async fn new(languages: Vec<(String, WitConfig)>, slots: &Slots) -> anyhow::Result<Self> {
        Self::new_with_max_duration(languages, MAX_DURATION, slots).await
    }

    /// Sets up transcribers with a custom decoded-audio limit. The upfront metadata check
    /// still uses [`MAX_DURATION`].
    pub async fn new_with_max_duration(
        languages: Vec<(String, WitConfig)>,
        max_duration: Duration,
        slots: &Slots,
    ) -> anyhow::Result<Self> {
        let decoder = Decoder::default();
        decoder.check().await.context("ffmpeg is required")?;
        let segmenter = SileroSegmenter::new(SileroConfig::default())?;
        let mut configured = Vec::new();
        for (code, wit) in languages {
            let recognizer = WitRecognizer::new(wit)
                .await
                .with_context(|| format!("setting up Wit for `{code}`"))?;
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
                max_duration,
            });
        }
        Ok(Self {
            queue: Queue::new(limits(), slots),
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
        let max_duration = self.max_duration;
        upload.submit(move |progress| async move {
            let tracker = Mutex::new(Tracker::default());
            let (limit_exceeded, mut exceeded) = watch::channel(false);
            let report = |event: TranscribeEvent| {
                if exceeds_duration_limit(&event, max_duration) {
                    limit_exceeded.send_replace(true);
                    return;
                }
                let mut tracker = tracker.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(percent) = tracker.update(&event) {
                    progress.set(percent);
                }
            };
            let segments = tokio::select! {
                biased;
                _ = exceeded.wait_for(|exceeded| *exceeded) => {
                    bail!("decoded audio exceeds the {max_duration:?} duration limit");
                }
                result = transcriber.transcribe(&audio, &report) => result,
            };
            // A final decoded event can arrive in the same poll as transcription completes.
            ensure!(
                !*exceeded.borrow(),
                "decoded audio exceeds the {max_duration:?} duration limit"
            );
            let segments = segments?;
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

/// The name for a downloaded transcript, cleaned of characters that do not belong in file names.
pub fn download_name(file_name: &str) -> String {
    super::download_name(file_name, "transcript")
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

fn exceeds_duration_limit(event: &TranscribeEvent, max_duration: Duration) -> bool {
    matches!(event, TranscribeEvent::Decoded { seconds } if *seconds > max_duration.as_secs_f64())
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
    let srt = std::fs::read_to_string(srt)?;
    Ok(Transcript {
        name,
        language,
        text: std::fs::read_to_string(text)?,
        cues: cues(&srt),
        srt,
    })
}

/// Reads the cues back from SRT as tafrigh writes it: each cue's number, time range and text,
/// with a blank line after each cue.
fn cues(srt: &str) -> Vec<Cue> {
    srt.split("\n\n")
        .map(|block| block.trim_matches('\n'))
        .filter_map(|block| {
            let mut lines = block.lines().skip(1);
            let (start, end) = lines.next()?.split_once(" --> ")?;
            Some(Cue {
                start: start.to_owned(),
                end: end.to_owned(),
                text: lines.collect::<Vec<_>>().join(" "),
            })
        })
        .collect()
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
    fn cues_match_the_srt_file() -> anyhow::Result<()> {
        let segments = [
            Segment::new(0.5, 2.25, "اللغة العربية من أوسع اللغات مفردات"),
            Segment::new(2.25, 3.0, ""),
            Segment::new(61.0, 3725.125, "وقد حفظت لنا كتب التراث كثيرا من شواهدها"),
        ];
        let transcript = render("lesson".to_owned(), "ar".to_owned(), &segments)?;
        assert_eq!(
            transcript.cues.len(),
            transcript.srt.matches(" --> ").count()
        );
        for (number, cue) in (1..).zip(&transcript.cues) {
            let block = format!("{number}\n{} --> {}\n{}\n", cue.start, cue.end, cue.text);
            assert!(
                transcript.srt.contains(&block),
                "{block:?} in {:?}",
                transcript.srt
            );
        }
        let last = transcript.cues.last().unwrap();
        assert_eq!(last.end, "01:02:05,125");
        assert_eq!(last.text, "وقد حفظت لنا كتب التراث كثيرا من شواهدها");
        Ok(())
    }

    #[test]
    fn only_decoded_audio_past_the_limit_triggers_the_cutoff() {
        for limit in [Duration::from_secs(2), MAX_DURATION] {
            let seconds = limit.as_secs_f64();
            assert!(!exceeds_duration_limit(
                &TranscribeEvent::Started {
                    duration: Some(seconds + 1.0),
                },
                limit,
            ));
            for decoded in [seconds - 0.001, seconds] {
                assert!(!exceeds_duration_limit(
                    &TranscribeEvent::Decoded { seconds: decoded },
                    limit,
                ));
            }
            assert!(exceeds_duration_limit(
                &TranscribeEvent::Decoded {
                    seconds: seconds + 0.001,
                },
                limit,
            ));
        }
    }

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
