use std::io::{Cursor, Read};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use crate::queue::{CancelReason, JobId, Status};

use super::*;

struct TestEngine;

impl Engine for TestEngine {
    fn diacritize(
        &mut self,
        text: &str,
        progress: &mut dyn FnMut(tashkeel::Progress) -> ControlFlow<()>,
    ) -> tashkeel::Result<String> {
        for completed in [0, 1] {
            if progress(tashkeel::Progress {
                completed,
                total: 1,
            })
            .is_break()
            {
                return Err(tashkeel::Error::Cancelled);
            }
        }
        Ok(text.replace("مرحبا", "مَرْحَبًا"))
    }
}

fn with_engine(
    slots: &Slots,
    load: impl Fn(Model) -> anyhow::Result<Box<dyn Engine>> + Send + Sync + 'static,
) -> Diacritization {
    let tool = Diacritization::unavailable(slots);
    assert!(
        tool.engines
            .set(Arc::new(EnginePool::new(3, Box::new(load))))
            .is_ok()
    );
    tool
}

fn test_tool() -> Diacritization {
    with_engine(&Slots::new(4), |_| Ok(Box::new(TestEngine)))
}

// Match the optional client address accepted by the queue API.
#[allow(clippy::unnecessary_wraps)]
fn client(number: u8) -> Option<IpAddr> {
    Some(Ipv4Addr::new(127, 0, 0, number).into())
}

async fn upload(name: &str, bytes: &[u8]) -> UploadedDocument {
    let upload = UploadedDocument::new(name).await.unwrap();
    tokio::fs::write(&upload.path, bytes).await.unwrap();
    upload
}

async fn finished(tool: &Diacritization, id: &JobId) -> Arc<Document> {
    let mut watch = tool.queue.watch(id).unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match watch.status() {
                Status::Done(document) => return document,
                Status::Failed | Status::Cancelled(_) => panic!("diacritization did not finish"),
                _ => assert!(watch.changed().await),
            }
        }
    })
    .await
    .unwrap()
}

fn docx(text: &str) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">{text}</w:t></w:r></w:p></w:body></w:document>"#
    );
    for (name, bytes) in [
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="bin" ContentType="application/octet-stream"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.as_slice(),
        ),
        (
            "_rels/.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/document.xml", document.as_bytes()),
        ("word/media/original.bin", b"unchanged opaque content"),
    ] {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

fn part(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut result = Vec::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_end(&mut result)
        .unwrap();
    result
}

#[test]
fn text_limit_counts_unicode_scalars_without_normalizing_source() {
    assert_eq!(validate_text(""), Err(Problem::Missing));
    assert_eq!(validate_text(&"🙂".repeat(1_000)), Ok(()));
    assert_eq!(validate_text(&"اَ".repeat(500)), Ok(()));
    assert_eq!(
        validate_text(&"🙂".repeat(1_001)),
        Err(Problem::TextTooLong)
    );
    assert_eq!(validate_text(&"اَ".repeat(501)), Err(Problem::TextTooLong));
    assert_eq!(validate_text(" \t\r\n"), Ok(()));
}

#[test]
fn streaming_text_keeps_utf8_whitespace_and_long_words_without_a_document_cap() {
    let source = format!(
        "\u{feff} مرحبا\r\n\n{}؟ {}\u{2028}{} نهاية",
        "بَ🙂".repeat(12_000),
        "word ".repeat(1_000),
        "x".repeat(2_005)
    );
    let mut chunks = TextChunks::new(source.as_bytes());
    let mut actual = String::new();
    let mut count = 0;
    while let Some(chunk) = chunks.next_chunk().unwrap() {
        assert_ne!(chunk, "");
        assert!(chunk.chars().count() <= 500);
        actual.push_str(&chunk);
        count += 1;
    }
    assert_eq!(actual, source);
    assert!(count > 80);
    assert!(chunks.next_chunk().unwrap().is_none());
}

#[test]
fn streaming_text_rejects_invalid_and_truncated_utf8_at_chunk_boundaries() {
    for invalid in [
        &b"\xff"[..],
        &b"\xd8"[..],
        &b"\xe0\x80\x80"[..],
        &b"\xf4\x90\x80\x80"[..],
    ] {
        let mut source = vec![b'a'; 500];
        source.extend_from_slice(invalid);
        let mut chunks = TextChunks::new(source.as_slice());
        let error = loop {
            match chunks.next_chunk() {
                Err(error) => break error,
                Ok(Some(_)) => {}
                Ok(None) => panic!("invalid UTF-8 accepted"),
            }
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }
}

#[tokio::test]
async fn uploads_validate_contents_and_drop_failed_originals() {
    let tool = test_tool();
    assert_eq!(extension("كتاب.DOCX"), Some("docx".into()));
    assert_eq!(extension("كتاب.TXT"), Some("txt".into()));
    assert_eq!(
        UploadedDocument::new("book.doc").await.err(),
        Some(Problem::WrongType)
    );
    assert_eq!(download_name("../book\"?.txt"), "book");
    for (name, bytes, expected) in [
        ("bad.txt", &b"\xff\xfe"[..], Problem::InvalidUtf8),
        (
            "bad.docx",
            &b"not a Word document"[..],
            Problem::InvalidDocx,
        ),
    ] {
        let upload = upload(name, bytes).await;
        let path = upload.path.clone();
        assert_eq!(tool.validate(upload).await.err(), Some(expected));
        assert!(!path.exists());
    }
    let upload = upload("large.txt", b"").await;
    tokio::fs::OpenOptions::new()
        .write(true)
        .open(&upload.path)
        .await
        .unwrap()
        .set_len(MAX_UPLOAD_BYTES + 1)
        .await
        .unwrap();
    assert_eq!(tool.validate(upload).await.err(), Some(Problem::TooLarge));
}

#[tokio::test]
async fn docx_validation_retains_the_librarys_default_paragraph_bound() {
    let tool = test_tool();
    let too_long = "x".repeat(tashkeel::docx::Limits::default().max_paragraph_bytes + 1);
    let upload = upload("bounded.docx", &docx(&too_long)).await;
    assert_eq!(
        tool.validate(upload).await.err(),
        Some(Problem::InvalidDocx)
    );
}

#[tokio::test]
async fn missing_models_stay_unavailable_without_creating_a_cache() {
    let directory = tempfile::tempdir().unwrap();
    let cache = directory.path().join("missing");
    let tool = Diacritization::unavailable(&Slots::new(4));
    assert!(tool.initialize(&cache, None).await.is_err());
    assert!(!tool.is_available());
    assert!(!tool.model_available(Model::Shakkelha));
    assert!(!tool.model_available(Model::Catt));
    assert!(!cache.exists());
    let id = tool.queue.reserve(client(1), None).unwrap();
    let reservation = tool.queue.begin_upload(&id).unwrap();
    assert_eq!(
        tool.submit_text(reservation, Model::Shakkelha, "مرحبا".into()),
        Err(SubmitError::Rejected(Rejection::Closed))
    );
    assert!(tool.queue.watch(&id).is_none());
}

#[tokio::test]
async fn invalid_text_releases_its_reservation_without_queueing() {
    let tool = test_tool();
    let id = tool.queue.reserve(client(1), None).unwrap();
    let reservation = tool.queue.begin_upload(&id).unwrap();
    assert_eq!(
        tool.submit_text(reservation, Model::Catt, "🙂".repeat(1_001)),
        Err(SubmitError::Problem(Problem::TextTooLong))
    );
    assert!(tool.queue.watch(&id).is_none());
    assert!(tool.queue.reserve(client(1), None).is_ok());
}

#[tokio::test]
async fn text_results_keep_originals_and_small_previews_and_expire_after_downloads() {
    let tool = test_tool();
    let id = tool.queue.reserve(client(1), None).unwrap();
    let reservation = tool.queue.begin_upload(&id).unwrap();
    let source = "\u{feff} مرحبا 🙂\t\r\n\n";
    tool.submit_text(reservation, Model::Shakkelha, source.into())
        .unwrap();
    let document = finished(&tool, &id).await;
    assert_eq!(document.kind, DocumentKind::Text);
    assert_eq!(document.model, Model::Shakkelha);
    assert_eq!(document.text.as_ref().unwrap().source, source);
    assert_eq!(
        document.text.as_ref().unwrap().output,
        source.replace("مرحبا", "مَرْحَبًا")
    );
    assert_eq!(
        tokio::fs::read_to_string(&document.original_path)
            .await
            .unwrap(),
        source
    );
    assert_eq!(
        tokio::fs::read_to_string(&document.path).await.unwrap(),
        document.text.as_ref().unwrap().output
    );
    let path = document.path.clone();
    let original = document.original_path.clone();
    tokio::time::pause();
    tokio::time::advance(KEEP_FINISHED + Duration::from_secs(1)).await;
    tool.queue.sweep();
    assert!(tool.queue.result(&id).is_none());
    assert!(
        path.exists(),
        "an active download owns the result directory"
    );
    drop(document);
    assert!(!path.exists());
    assert!(!original.exists());
}

#[tokio::test]
async fn txt_documents_have_no_text_mode_cap_or_in_memory_preview() {
    let tool = test_tool();
    let source = format!("\u{feff}{}\r\n", "مرحبا ".repeat(1_100));
    let upload = tool
        .validate(upload("كتاب.TXT", source.as_bytes()).await)
        .await
        .unwrap();
    let id = tool.queue.reserve(client(1), None).unwrap();
    tool.submit_document(
        tool.queue.begin_upload(&id).unwrap(),
        Model::Catt,
        upload,
        "كتاب".into(),
    )
    .unwrap();
    let document = finished(&tool, &id).await;
    assert_eq!(document.kind, DocumentKind::Txt);
    assert_eq!(document.name, "كتاب");
    assert_eq!(document.model, Model::Catt);
    assert!(document.text.is_none());
    assert_eq!(
        tokio::fs::read_to_string(&document.original_path)
            .await
            .unwrap(),
        source
    );
    assert_eq!(
        tokio::fs::read_to_string(&document.path).await.unwrap(),
        source.replace("مرحبا", "مَرْحَبًا")
    );
}

#[tokio::test]
async fn docx_results_preserve_the_original_and_formatting() {
    let tool = test_tool();
    let original = docx(" مرحبا ");
    let upload = tool
        .validate(upload("book.docx", &original).await)
        .await
        .unwrap();
    let id = tool.queue.reserve(client(1), None).unwrap();
    tool.submit_document(
        tool.queue.begin_upload(&id).unwrap(),
        Model::Shakkelha,
        upload,
        "book".into(),
    )
    .unwrap();
    let document = finished(&tool, &id).await;
    assert_eq!(document.kind, DocumentKind::Docx);
    assert!(document.text.is_none());
    assert_eq!(
        tokio::fs::read(&document.original_path).await.unwrap(),
        original
    );
    let result = tokio::fs::read(&document.path).await.unwrap();
    for name in [
        "[Content_Types].xml",
        "_rels/.rels",
        "word/media/original.bin",
    ] {
        assert_eq!(part(&result, name), part(&original, name));
    }
    assert_eq!(
        String::from_utf8(part(&result, "word/document.xml")).unwrap(),
        String::from_utf8(part(&original, "word/document.xml"))
            .unwrap()
            .replace("مرحبا", "مَرْحَبًا")
    );
}

struct FailingEngine {
    calls: usize,
}

impl Engine for FailingEngine {
    fn diacritize(
        &mut self,
        text: &str,
        progress: &mut dyn FnMut(tashkeel::Progress) -> ControlFlow<()>,
    ) -> tashkeel::Result<String> {
        self.calls += 1;
        if self.calls == 2 {
            return Err(tashkeel::Error::Inference("test failure".into()));
        }
        TestEngine.diacritize(text, progress)
    }
}

#[tokio::test]
async fn inference_failure_removes_original_and_partial_output_and_releases_the_slot() {
    let tool = with_engine(&Slots::new(1), |_| Ok(Box::new(FailingEngine { calls: 0 })));
    let upload = upload("book.txt", "مرحبا ".repeat(200).as_bytes()).await;
    let directory = upload.path.parent().unwrap().to_owned();
    let upload = tool.validate(upload).await.unwrap();
    let id = tool.queue.reserve(client(1), None).unwrap();
    let mut watch = tool.queue.watch(&id).unwrap();
    tool.submit_document(
        tool.queue.begin_upload(&id).unwrap(),
        Model::Catt,
        upload,
        "book".into(),
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !watch.status().is_finished() {
            assert!(watch.changed().await);
        }
    })
    .await
    .unwrap();
    assert!(matches!(watch.status(), Status::Failed));
    assert!(tool.queue.result(&id).is_none());
    assert!(!directory.exists());
    let next = tool.queue.reserve(client(1), None).unwrap();
    tool.submit_text(
        tool.queue.begin_upload(&next).unwrap(),
        Model::Catt,
        "مرحبا".into(),
    )
    .unwrap();
    assert_eq!(
        finished(&tool, &next).await.text.as_ref().unwrap().output,
        "مَرْحَبًا"
    );
}

struct CountedEngine {
    live: Arc<AtomicUsize>,
}

impl Engine for CountedEngine {
    fn diacritize(
        &mut self,
        text: &str,
        progress: &mut dyn FnMut(tashkeel::Progress) -> ControlFlow<()>,
    ) -> tashkeel::Result<String> {
        TestEngine.diacritize(text, progress)
    }
}

impl Drop for CountedEngine {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
}

#[test]
fn pool_reuses_sessions_and_never_exceeds_three_across_model_choices() {
    let loaded = Arc::new(AtomicUsize::new(0));
    let live = Arc::new(AtomicUsize::new(0));
    let count = loaded.clone();
    let active = live.clone();
    let pool = EnginePool::new(
        3,
        Box::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            assert!(active.fetch_add(1, Ordering::SeqCst) < 3);
            Ok(Box::new(CountedEngine {
                live: active.clone(),
            }))
        }),
    );
    drop(pool.borrow(Model::Shakkelha).unwrap());
    drop(pool.borrow(Model::Catt).unwrap());
    for _ in 0..5 {
        drop(pool.borrow(Model::Shakkelha).unwrap());
        drop(pool.borrow(Model::Catt).unwrap());
    }
    assert_eq!(loaded.load(Ordering::SeqCst), 2);
    let first = pool.borrow(Model::Shakkelha).unwrap();
    let second = pool.borrow(Model::Shakkelha).unwrap();
    let third = pool.borrow(Model::Shakkelha).unwrap();
    assert_eq!(live.load(Ordering::SeqCst), 3);
    assert_eq!(loaded.load(Ordering::SeqCst), 4);
    assert!(pool.borrow(Model::Catt).is_err());
    drop(first);
    let replacement = pool.borrow(Model::Catt).unwrap();
    assert_eq!(live.load(Ordering::SeqCst), 3);
    drop((second, third, replacement));
    assert_eq!(live.load(Ordering::SeqCst), 3);
    drop(pool);
    assert_eq!(live.load(Ordering::SeqCst), 0);
}

#[test]
fn failed_lazy_load_does_not_permanently_consume_pool_capacity() {
    let fail = AtomicBool::new(true);
    let pool = EnginePool::new(
        1,
        Box::new(move |_| {
            ensure!(!fail.swap(false, Ordering::SeqCst), "failed model load");
            Ok(Box::new(TestEngine))
        }),
    );
    assert!(pool.borrow(Model::Catt).is_err());
    drop(pool.borrow(Model::Catt).unwrap());
    drop(pool.borrow(Model::Shakkelha).unwrap());
}

struct SlowEngine {
    entered: Option<tokio::sync::oneshot::Sender<()>>,
    finish: std::sync::mpsc::Receiver<()>,
}

impl Engine for SlowEngine {
    fn diacritize(
        &mut self,
        text: &str,
        progress: &mut dyn FnMut(tashkeel::Progress) -> ControlFlow<()>,
    ) -> tashkeel::Result<String> {
        if let Some(entered) = self.entered.take() {
            if progress(tashkeel::Progress {
                completed: 1,
                total: 2,
            })
            .is_break()
            {
                return Err(tashkeel::Error::Cancelled);
            }
            entered.send(()).unwrap();
            self.finish.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        if progress(tashkeel::Progress {
            completed: 2,
            total: 2,
        })
        .is_break()
        {
            return Err(tashkeel::Error::Cancelled);
        }
        Ok(text.to_owned())
    }
}

#[tokio::test]
async fn cancellation_keeps_native_work_its_files_and_slot_until_it_returns() {
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, finish) = std::sync::mpsc::channel();
    let slow = Mutex::new(Some(SlowEngine {
        entered: Some(entered),
        finish,
    }));
    let tool = with_engine(&Slots::new(1), move |_| {
        Ok(Box::new(slow.lock().unwrap().take().unwrap()))
    });
    let upload = upload("book.docx", &docx("مرحبا")).await;
    let original_path = upload.path.clone();
    let upload = tool.validate(upload).await.unwrap();
    let id = tool.queue.reserve(client(1), None).unwrap();
    let mut watch = tool.queue.watch(&id).unwrap();
    tool.submit_document(
        tool.queue.begin_upload(&id).unwrap(),
        Model::Catt,
        upload,
        "book".into(),
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), started)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        watch.status(),
        Status::Running { percent: 50, .. }
    ));
    assert!(tool.queue.cancel(&id));
    assert!(matches!(
        watch.status(),
        Status::Cancelled(CancelReason::User)
    ));
    assert_eq!(
        tool.queue.reserve(client(1), None),
        Err(Rejection::ActiveJob)
    );
    let next = tool.queue.reserve(client(2), None).unwrap();
    tool.submit_text(
        tool.queue.begin_upload(&next).unwrap(),
        Model::Catt,
        "التالي".into(),
    )
    .unwrap();
    assert!(matches!(
        tool.queue.watch(&next).unwrap().status(),
        Status::Waiting { .. }
    ));
    assert!(original_path.exists());
    assert!(tool.queue.result(&id).is_none());
    release.send(()).unwrap();
    let result = finished(&tool, &next).await;
    assert_eq!(result.text.as_ref().unwrap().output, "التالي");
    assert!(!original_path.exists());
    assert!(tool.queue.reserve(client(1), None).is_ok());
    assert!(tool.queue.result(&id).is_none());
}

#[tokio::test]
#[ignore = "requires TASHKEEL_MODEL_CACHE and ORT_DYLIB_PATH"]
async fn real_models_diacritize_text_txt_and_docx_offline() {
    let cache = PathBuf::from(std::env::var_os("TASHKEEL_MODEL_CACHE").expect("model cache"));
    let runtime = PathBuf::from(std::env::var_os("ORT_DYLIB_PATH").expect("ONNX Runtime"));
    let tool = Diacritization::unavailable(&Slots::new(4));
    tool.initialize(&cache, Some(runtime)).await.unwrap();
    for model in [Model::Shakkelha, Model::Catt] {
        let source = "\u{feff} مرحبا بالعالم!\r\nالعربية لغة جميلة. 123 🙂";
        let id = tool.queue.reserve(client(1), None).unwrap();
        tool.submit_text(tool.queue.begin_upload(&id).unwrap(), model, source.into())
            .unwrap();
        let result = finished(&tool, &id).await;
        let output = &result.text.as_ref().unwrap().output;
        assert_ne!(output, source);
        assert_eq!(tashkeel::text::strip_tashkeel(output), source);
        assert_eq!(result.model, model);
        for (name, source, kind) in [
            ("book.txt", source.as_bytes().to_vec(), DocumentKind::Txt),
            ("book.docx", docx("مرحبا بالعالم"), DocumentKind::Docx),
        ] {
            let uploaded = tool.validate(upload(name, &source).await).await.unwrap();
            let id = tool.queue.reserve(client(1), None).unwrap();
            tool.submit_document(
                tool.queue.begin_upload(&id).unwrap(),
                model,
                uploaded,
                "book".into(),
            )
            .unwrap();
            let document = finished(&tool, &id).await;
            assert_eq!(document.kind, kind);
            assert!(document.text.is_none());
            assert_eq!(
                tokio::fs::read(&document.original_path).await.unwrap(),
                source
            );
            let output = tokio::fs::read(&document.path).await.unwrap();
            assert_ne!(output, source);
            if kind == DocumentKind::Txt {
                assert_eq!(
                    tashkeel::text::strip_tashkeel(std::str::from_utf8(&output).unwrap())
                        .as_bytes(),
                    source
                );
            } else {
                assert_eq!(
                    part(&output, "word/media/original.bin"),
                    part(&source, "word/media/original.bin")
                );
                tashkeel::docx::transform(&output, |text| Ok(text.to_owned())).unwrap();
            }
        }
    }
}
