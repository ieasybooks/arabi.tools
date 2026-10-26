//! Offline Arabic diacritization with bounded, reusable CPU engines.

mod pool;
mod text;

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use anyhow::{Context, ensure};
use tempfile::TempDir;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::queue::{Progress, Queue, Rejection, Slots, Upload};
use pool::{Engine, EnginePool};
use text::TextChunks;

pub use super::{KEEP_FINISHED, MAX_UPLOAD_BYTES};
pub use tashkeel::Model;

pub const MAX_TEXT_CHARACTERS: usize = 1_000;
pub const EXTENSIONS: [&str; 2] = ["txt", "docx"];

/// Clones share the model pool, validation lane and job queue.
#[derive(Clone)]
pub struct Diacritization {
    pub queue: Queue<Document>,
    engines: Arc<OnceLock<Arc<EnginePool>>>,
    initialization: Arc<Semaphore>,
    validation: Arc<Semaphore>,
}

/// Finished files stay on disk until the queue and all downloads release this result.
pub struct Document {
    pub name: String,
    pub model: Model,
    pub kind: DocumentKind,
    pub original_path: PathBuf,
    pub path: PathBuf,
    /// Only text-mode jobs have a preview; file contents are never cloned into pages.
    pub text: Option<TextResult>,
    _directory: TempDir,
}

pub struct TextResult {
    pub source: String,
    pub output: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    Text,
    Txt,
    Docx,
}

impl DocumentKind {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Text | Self::Txt => "txt",
            Self::Docx => "docx",
        }
    }

    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Text | Self::Txt => "text/plain; charset=utf-8",
            Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        }
    }
}

/// Owns the original upload through validation, waiting, processing and result retention.
pub struct UploadedDocument {
    directory: TempDir,
    pub path: PathBuf,
    kind: DocumentKind,
}

impl UploadedDocument {
    pub async fn new(file_name: &str) -> Result<Self, Problem> {
        let kind = match extension(file_name).as_deref() {
            Some("txt") => DocumentKind::Txt,
            Some("docx") => DocumentKind::Docx,
            _ => return Err(Problem::WrongType),
        };
        tokio::task::spawn_blocking(move || {
            let directory = directory().map_err(|_| Problem::Unreadable)?;
            let path = directory
                .path()
                .join(format!("original.{}", kind.extension()));
            Ok(Self {
                directory,
                path,
                kind,
            })
        })
        .await
        .map_err(|_| Problem::Unreadable)?
    }
}

/// Preflight keeps metadata and the original file, never a second in-memory document.
pub struct ValidatedDocument {
    upload: UploadedDocument,
    paragraphs: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    Missing,
    WrongType,
    TooLarge,
    InvalidUtf8,
    InvalidDocx,
    TextTooLong,
    Unreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    Problem(Problem),
    Rejected(Rejection),
}

impl Diacritization {
    pub fn unavailable(slots: &Slots) -> Self {
        Self {
            queue: Queue::new(super::limits(), slots),
            engines: Arc::default(),
            initialization: Arc::new(Semaphore::new(1)),
            validation: Arc::new(Semaphore::new(1)),
        }
    }

    /// Verifies pinned artifacts in `cache_dir/{model}/v1` and probes both engines.
    /// No request or startup path downloads models. Serve the site while awaiting this.
    pub async fn initialize(
        &self,
        cache_dir: &Path,
        runtime_path: Option<PathBuf>,
    ) -> anyhow::Result<()> {
        let permit = self.initialization.clone().acquire_owned().await?;
        let engines = self.engines.clone();
        let cache_dir = cache_dir.to_owned();
        tokio::task::spawn_blocking(move || {
            // The worker owns this permit even if its async caller disconnects.
            let _permit = permit;
            ensure!(
                engines.get().is_none(),
                "diacritization was already initialized"
            );
            let pool = EnginePool::load(cache_dir, runtime_path, super::limits().running)?;
            engines
                .set(Arc::new(pool))
                .map_err(|_| anyhow::anyhow!("diacritization was already initialized"))
        })
        .await?
    }

    pub fn is_available(&self) -> bool {
        self.engines.get().is_some()
    }

    /// Both model choices become available together after successful startup.
    pub fn model_available(&self, _model: Model) -> bool {
        self.is_available()
    }

    pub async fn validate(&self, upload: UploadedDocument) -> Result<ValidatedDocument, Problem> {
        let permit = self
            .validation
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Problem::Unreadable)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let paragraphs = check_document(&upload)?;
            Ok(ValidatedDocument { upload, paragraphs })
        })
        .await
        .map_err(|_| Problem::Unreadable)?
    }

    pub fn submit_text(
        &self,
        reservation: Upload<Document>,
        model: Model,
        text: String,
    ) -> Result<(), SubmitError> {
        validate_text(&text).map_err(SubmitError::Problem)?;
        self.submit(reservation, model, Input::Text(text))
            .map_err(SubmitError::Rejected)
    }

    pub fn submit_document(
        &self,
        reservation: Upload<Document>,
        model: Model,
        document: ValidatedDocument,
        name: String,
    ) -> Result<(), Rejection> {
        self.submit(reservation, model, Input::Document(document, name))
    }

    fn submit(
        &self,
        reservation: Upload<Document>,
        model: Model,
        input: Input,
    ) -> Result<(), Rejection> {
        let pool = self.engines.get().cloned().ok_or(Rejection::Closed)?;
        reservation.submit_cancellable(move |progress, cancellation| async move {
            let permit = tokio::select! {
                biased;
                () = cancellation.cancelled() => return Err(tashkeel::Error::Cancelled.into()),
                permit = pool.permits.clone().acquire_owned() => permit?,
            };
            // Never race this await against cancellation. Native work must finish before
            // the future returns and the queue releases its running slot.
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                check_cancelled(&cancellation)?;
                let mut engine = pool.borrow(model)?;
                check_cancelled(&cancellation)?;
                process(input, model, engine.engine(), &progress, &cancellation)
            })
            .await?
        })
    }
}

pub fn validate_text(text: &str) -> Result<(), Problem> {
    if text.is_empty() {
        return Err(Problem::Missing);
    }
    if text.chars().take(MAX_TEXT_CHARACTERS + 1).count() > MAX_TEXT_CHARACTERS {
        return Err(Problem::TextTooLong);
    }
    Ok(())
}

pub fn extension(name: &str) -> Option<String> {
    let extension = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    EXTENSIONS
        .contains(&extension.as_str())
        .then_some(extension)
}

pub fn download_name(name: &str) -> String {
    super::download_name(name, "document")
}

fn directory() -> std::io::Result<TempDir> {
    tempfile::Builder::new().prefix("diacritization-").tempdir()
}

fn input_file(path: &Path) -> Result<File, Problem> {
    let file = File::open(path).map_err(|_| Problem::Unreadable)?;
    let metadata = file.metadata().map_err(|_| Problem::Unreadable)?;
    if !metadata.is_file() {
        return Err(Problem::Unreadable);
    }
    if metadata.len() > MAX_UPLOAD_BYTES {
        return Err(Problem::TooLarge);
    }
    Ok(file)
}

fn read_docx(path: &Path) -> Result<Vec<u8>, Problem> {
    let mut bytes = Vec::new();
    input_file(path)?
        .take(MAX_UPLOAD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Problem::Unreadable)?;
    if bytes.len() as u64 > MAX_UPLOAD_BYTES {
        return Err(Problem::TooLarge);
    }
    Ok(bytes)
}

fn check_document(upload: &UploadedDocument) -> Result<usize, Problem> {
    if upload.kind == DocumentKind::Docx {
        let bytes = read_docx(&upload.path)?;
        let mut paragraphs = 0;
        tashkeel::docx::transform(&bytes, |text| {
            paragraphs += 1;
            Ok(text.to_owned())
        })
        .map_err(|_| Problem::InvalidDocx)?;
        Ok(paragraphs)
    } else {
        let file = input_file(&upload.path)?;
        let mut chunks = TextChunks::new(file.take(MAX_UPLOAD_BYTES + 1));
        let mut bytes = 0;
        while let Some(chunk) = chunks.next_chunk().map_err(|error| {
            if error.kind() == std::io::ErrorKind::InvalidData {
                Problem::InvalidUtf8
            } else {
                Problem::Unreadable
            }
        })? {
            bytes += chunk.len() as u64;
            if bytes > MAX_UPLOAD_BYTES {
                return Err(Problem::TooLarge);
            }
        }
        Ok(0)
    }
}

enum Input {
    Text(String),
    Document(ValidatedDocument, String),
}

fn process(
    input: Input,
    model: Model,
    engine: &mut dyn Engine,
    progress: &Progress<Document>,
    cancellation: &CancellationToken,
) -> anyhow::Result<Document> {
    match input {
        Input::Text(source) => {
            let directory = directory()?;
            let original_path = directory.path().join("original.txt");
            let path = directory.path().join("result.txt");
            fs::write(&original_path, &source)?;
            let output = engine.diacritize(&source, &mut |update| {
                progress.set(percent(update.completed as u128, update.total as u128));
                cancellation_flow(cancellation)
            })?;
            check_cancelled(cancellation)?;
            fs::write(&path, &output)?;
            Ok(Document {
                name: "text".into(),
                model,
                kind: DocumentKind::Text,
                original_path,
                path,
                text: Some(TextResult { source, output }),
                _directory: directory,
            })
        }
        Input::Document(document, name) => {
            let upload = document.upload;
            let path = upload
                .directory
                .path()
                .join(format!("result.{}", upload.kind.extension()));
            if upload.kind == DocumentKind::Docx {
                transform_docx(
                    &upload.path,
                    &path,
                    document.paragraphs,
                    engine,
                    progress,
                    cancellation,
                )?;
            } else {
                transform_text(&upload.path, &path, engine, progress, cancellation)?;
            }
            check_cancelled(cancellation)?;
            Ok(Document {
                name,
                model,
                kind: upload.kind,
                original_path: upload.path,
                path,
                text: None,
                _directory: upload.directory,
            })
        }
    }
}

fn transform_text(
    input: &Path,
    output: &Path,
    engine: &mut dyn Engine,
    progress: &Progress<Document>,
    cancellation: &CancellationToken,
) -> anyhow::Result<()> {
    let file = input_file(input).map_err(|problem| anyhow::anyhow!("invalid TXT: {problem:?}"))?;
    let total = u128::from(file.metadata()?.len());
    let mut chunks = TextChunks::new(file.take(MAX_UPLOAD_BYTES + 1));
    let mut output = BufWriter::new(File::create(output)?);
    let mut bytes = 0_u128;
    while let Some(chunk) = chunks.next_chunk()? {
        check_cancelled(cancellation)?;
        bytes += chunk.len() as u128;
        ensure!(
            bytes <= u128::from(MAX_UPLOAD_BYTES),
            "TXT exceeds the upload limit"
        );
        let result = engine.diacritize(&chunk, &mut |_| cancellation_flow(cancellation))?;
        output.write_all(result.as_bytes())?;
        progress.set(percent(bytes, total));
    }
    check_cancelled(cancellation)?;
    output.flush().context("saving diacritized text")
}

fn transform_docx(
    input: &Path,
    output: &Path,
    paragraphs: usize,
    engine: &mut dyn Engine,
    progress: &Progress<Document>,
    cancellation: &CancellationToken,
) -> anyhow::Result<()> {
    let bytes = read_docx(input).map_err(|problem| anyhow::anyhow!("invalid DOCX: {problem:?}"))?;
    check_cancelled(cancellation)?;
    let mut done = 0_u128;
    let result = tashkeel::docx::transform(&bytes, |text| {
        check_cancelled(cancellation)?;
        let result = engine.diacritize(text, &mut |update| {
            let chunks = update.total.max(1) as u128;
            progress.set(percent(
                done * chunks + update.completed as u128,
                paragraphs as u128 * chunks,
            ));
            cancellation_flow(cancellation)
        })?;
        done += 1;
        progress.set(percent(done, paragraphs as u128));
        Ok(result)
    })?;
    check_cancelled(cancellation)?;
    fs::write(output, result).context("saving diacritized DOCX")
}

fn percent(done: u128, total: u128) -> u8 {
    u8::try_from(
        done.saturating_mul(100)
            .checked_div(total)
            .unwrap_or(0)
            .min(99),
    )
    .unwrap_or(99)
}

fn cancellation_flow(cancellation: &CancellationToken) -> ControlFlow<()> {
    if cancellation.is_cancelled() {
        ControlFlow::Break(())
    } else {
        ControlFlow::Continue(())
    }
}

fn check_cancelled(cancellation: &CancellationToken) -> tashkeel::Result<()> {
    if cancellation.is_cancelled() {
        Err(tashkeel::Error::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
