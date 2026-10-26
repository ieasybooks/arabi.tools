//! PDF and image OCR through one process-wide Drive recognizer.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use anyhow::{Context, bail, ensure};
use pdfium_render::prelude::{Pdfium, PdfiumError};
use tahweel::{DocumentStatus, Event, Format, Input, OutputWriter, Tahweel};
use tahweel_drive::{DriveAuth, DriveConfig, DriveRecognizer, ServiceAccountKey};
use tempfile::TempDir;
use tokio::sync::Semaphore;

use crate::queue::{Queue, Rejection, Slots, Upload};

pub use super::{KEEP_FINISHED, MAX_UPLOAD_BYTES};
pub const MAX_PAGES: usize = 1_000;
pub const EXTENSIONS: [&str; 4] = ["pdf", "jpg", "jpeg", "png"];

/// Clones share the recognizer's account lanes and the queue.
#[derive(Clone)]
pub struct Ocr {
    pub queue: Queue<Document>,
    recognizer: Arc<OnceLock<DriveRecognizer>>,
    pdfium_library: Option<PathBuf>,
    /// Preflight runs before queueing; keep many simultaneous uploads from decoding at once.
    validation: Arc<Semaphore>,
}

/// A finished document, with each download rendered once and retained with the preview.
pub struct Document {
    pub name: String,
    pub pages: Vec<String>,
    pub text: Vec<u8>,
    pub docx: Vec<u8>,
    pub json: Vec<u8>,
}

/// Owns the upload directory until validation, queueing and processing have all finished.
pub struct UploadedDocument {
    directory: TempDir,
    pub path: PathBuf,
}

impl UploadedDocument {
    pub fn new(file_name: &str) -> Result<Self, DocumentProblem> {
        let extension = extension(file_name).ok_or(DocumentProblem::WrongType)?;
        let directory = tempfile::Builder::new()
            .prefix("ocr-upload-")
            .tempdir()
            .map_err(|_| DocumentProblem::Unreadable)?;
        let path = directory.path().join(format!("document.{extension}"));
        Ok(Self { directory, path })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentProblem {
    Missing,
    WrongType,
    TooLarge,
    TooManyPages,
    Unreadable,
}

/// Parses the environment's JSON array without including its contents in errors.
pub fn drive_config(json: &str) -> anyhow::Result<DriveConfig> {
    let values: Vec<serde_json::Value> = serde_json::from_str(json)
        .map_err(|_| anyhow::anyhow!("DRIVE_SERVICE_ACCOUNTS must be a JSON array"))?;
    ensure!(!values.is_empty(), "DRIVE_SERVICE_ACCOUNTS is empty");
    let keys = values
        .iter()
        .map(|value| ServiceAccountKey::from_json(&value.to_string()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| anyhow::anyhow!("DRIVE_SERVICE_ACCOUNTS contains an invalid key"))?;
    let mut config = DriveConfig::new(DriveAuth::ServiceAccounts(keys));
    config.language = None;
    Ok(config)
}

impl Ocr {
    /// OCR without a recognizer yet; [`initialize`](Self::initialize) starts it. Jobs run in
    /// the shared `slots`.
    pub fn unavailable(slots: &Slots) -> Self {
        Self {
            queue: Queue::new(super::limits(), slots),
            recognizer: Arc::default(),
            pdfium_library: None,
            validation: Arc::new(Semaphore::new(1)),
        }
    }

    #[must_use]
    pub fn with_pdfium_library(mut self, path: Option<PathBuf>) -> Self {
        self.pdfium_library = path;
        self
    }

    pub async fn new(
        config: DriveConfig,
        pdfium_library: Option<PathBuf>,
        slots: &Slots,
    ) -> anyhow::Result<Self> {
        let ocr = Self::unavailable(slots).with_pdfium_library(pdfium_library);
        ocr.initialize(config).await?;
        Ok(ocr)
    }

    /// Starts the shared recognizer once. Serve the site while awaiting this: a slow Drive
    /// startup must not delay transcription. All clones become available after the probe.
    pub async fn initialize(&self, config: DriveConfig) -> anyhow::Result<()> {
        let recognizer = DriveRecognizer::new(config)
            .await
            .context("starting Drive OCR")?;
        // Token validation alone does not detect accounts without Drive storage. A blank
        // page exercises conversion, export and deletion before accepting any uploads.
        let mut image = std::io::Cursor::new(Vec::new());
        image::GrayImage::from_pixel(128, 128, image::Luma([255]))
            .write_to(&mut image, image::ImageFormat::Png)?;
        recognizer
            .recognize_page(&tahweel::PageImage::new(
                image.into_inner(),
                tahweel::ImageFormat::Png,
                128,
                128,
            ))
            .await
            .context("checking Drive OCR storage")?;
        self.recognizer
            .set(recognizer)
            .map_err(|_| anyhow::anyhow!("Drive OCR was already initialized"))
    }

    pub fn is_available(&self) -> bool {
        self.recognizer.get().is_some()
    }

    /// Checks the actual document before it joins the queue. The blocking task owns the
    /// directory, so disconnecting during validation cannot leave an upload behind.
    pub async fn validate(
        &self,
        upload: UploadedDocument,
    ) -> Result<UploadedDocument, DocumentProblem> {
        let library = self.pdfium_library.clone();
        let permit = self
            .validation
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| DocumentProblem::Unreadable)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            check_document(&upload.path, library.as_deref())?;
            Ok(upload)
        })
        .await
        .map_err(|_| DocumentProblem::Unreadable)?
    }

    pub fn submit(
        &self,
        reservation: Upload<Document>,
        upload: UploadedDocument,
        name: String,
    ) -> Result<(), Rejection> {
        let Some(recognizer) = self.recognizer.get().cloned() else {
            return Err(Rejection::Closed);
        };
        let library = self.pdfium_library.clone();
        reservation.submit_cancellable(move |progress, cancellation| async move {
            let mut builder = Tahweel::builder(recognizer)
                .output(OutputWriter::new().formats([]))
                .cancellation_token(cancellation.clone());
            if let Some(library) = library {
                builder = builder.pdfium_library(library);
            }
            let pipeline = builder.build();
            let tracker = Mutex::new(PageProgress::default());
            let report = |event| {
                let mut tracker = tracker.lock().unwrap_or_else(PoisonError::into_inner);
                if tracker.update(&event) {
                    if tracker.total > MAX_PAGES {
                        cancellation.cancel();
                    }
                    progress.pages(tracker.done, tracker.total);
                }
            };
            // Keep awaiting after cancellation: active pages must export and delete their
            // Drive documents before the directory and the running slot are released.
            let result = pipeline
                .run([Input::from(upload.path.clone())], &report)
                .await;
            drop(upload.directory);
            let result = result?;
            ensure!(
                !result.cancelled && result.failures() == 0,
                "OCR did not finish"
            );
            let mut documents = result.documents.into_iter();
            let document = documents.next().context("OCR returned no document")?;
            ensure!(documents.next().is_none(), "expected exactly one document");
            let DocumentStatus::Converted { pages, .. } = document.status else {
                bail!("OCR did not return pages");
            };
            ensure!(pages.len() <= MAX_PAGES, "OCR exceeded the page limit");
            tokio::task::spawn_blocking(move || render(name, pages)).await?
        })
    }
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

fn check_document(path: &Path, library: Option<&Path>) -> Result<(), DocumentProblem> {
    let size = std::fs::metadata(path)
        .map_err(|_| DocumentProblem::Unreadable)?
        .len();
    if size > MAX_UPLOAD_BYTES {
        return Err(DocumentProblem::TooLarge);
    }
    let extension = extension(&path.to_string_lossy()).ok_or(DocumentProblem::WrongType)?;
    if extension == "pdf" {
        let pdfium = load_pdfium(library).map_err(|_| DocumentProblem::Unreadable)?;
        let document = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|_| DocumentProblem::Unreadable)?;
        let pages =
            usize::try_from(document.pages().len()).map_err(|_| DocumentProblem::Unreadable)?;
        if pages > MAX_PAGES {
            return Err(DocumentProblem::TooManyPages);
        }
        if pages == 0 {
            return Err(DocumentProblem::Unreadable);
        }
    } else {
        let reader = image::ImageReader::open(path)
            .and_then(image::ImageReader::with_guessed_format)
            .map_err(|_| DocumentProblem::Unreadable)?;
        let expected = if extension == "png" {
            image::ImageFormat::Png
        } else {
            image::ImageFormat::Jpeg
        };
        if reader.format() != Some(expected) {
            return Err(DocumentProblem::WrongType);
        }
        reader.decode().map_err(|_| DocumentProblem::Unreadable)?;
    }
    Ok(())
}

/// Uses the same explicit / executable-adjacent / system search as tahweel. `PDFium` keeps
/// its bindings for the process lifetime, shared by preflight checks and pipeline runs.
fn load_pdfium(explicit: Option<&Path>) -> Result<Pdfium, PdfiumError> {
    let initialized = |bindings| match bindings {
        Ok(bindings) => Ok(Pdfium::new(bindings)),
        Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => Ok(Pdfium::default()),
        Err(error) => Err(error),
    };
    if let Some(path) = explicit {
        return initialized(Pdfium::bind_to_library(path));
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
        && let Ok(pdfium) = initialized(Pdfium::bind_to_library(
            Pdfium::pdfium_platform_library_name_at_path(directory),
        ))
    {
        return Ok(pdfium);
    }
    initialized(Pdfium::bind_to_system_library())
}

#[derive(Default)]
struct PageProgress {
    done: usize,
    total: usize,
}

impl PageProgress {
    fn update(&mut self, event: &Event) -> bool {
        match event {
            Event::DocumentStarted { pages, .. } => self.total = *pages,
            Event::PageRecognized { .. } => self.done += 1,
            _ => return false,
        }
        true
    }
}

fn render(name: String, pages: Vec<String>) -> anyhow::Result<Document> {
    let writer = OutputWriter::new();
    Ok(Document {
        name,
        text: writer.render(Format::Txt, &pages)?,
        docx: writer.render(Format::Docx, &pages)?,
        json: writer.render(Format::Json, &pages)?,
        pages,
    })
}
