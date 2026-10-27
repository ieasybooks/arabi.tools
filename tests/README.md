# Tests

## Rust tests

Run `cargo test --locked` with `ffmpeg` (built with libopus) and `ffprobe` on PATH (if `/tmp` is a small tmpfs, point `TMPDIR` at a directory on disk). Unit tests live next to the code. `tests/transcription.rs` starts the real router against a local Wit mock on ephemeral ports and drives it over HTTP: MP3 and M4A uploads through to the downloads, refusals and queue limits, upstream failures, silent recordings, Arabic and English recognition, upload-size boundaries, and both page languages. Assets are bundled from the test executable and the fonts are vendored, so the tests need no network once dependencies are fetched. See [fixtures/README.md](fixtures/README.md) for the recordings.

## Browser test

`tests/browser/` drives the real site in headless Chromium with Playwright, in Arabic and in English: choosing a file, the form staying locked while it is sent, the job page updating live until done, both downloads, a reloaded tab retrying after losing its reservation, and a tab opened from the upload page being refused while that page's upload is on its way. Run it from the repository root:

```sh
npm ci --prefix tests/browser
npx --prefix tests/browser playwright install --with-deps --only-shell chromium
npx --prefix tests/browser playwright install --with-deps webkit
cargo build --locked --bin arlang-tools --example mock_wit --example mock_ocr
topcoat asset bundle --bin arlang-tools
npm test --prefix tests/browser
```

It starts mock Wit on port 9804, the site with mock Drive and real diacritization on port 3204, and the site without OCR or diacritization on port 3205. All three ports must be free. Set `TAHWEEL_TEST_PDFIUM` to the pinned PDFium library for OCR browser tests, and provision the diacritization models and runtime as described below before running the browser suite. CI runs it on pull requests and keeps traces and screenshots of failures as the `browser-failures` artifact.

`examples/mock_wit.rs` explains how to try the site by hand without Wit tokens.

## OCR tests

`tests/ocr.rs` uses a local Drive mock with fresh generated keys, PNG/JPEG images and generated PDFs. Set `TAHWEEL_TEST_PDFIUM` to include PDF validation, the 1,000-page limit recheck and cancellation cleanup tests. CI installs the pinned library using `deploy/pdfium.sh`. The tests cover both interface languages, all three downloads, progress, queue limits, invalid uploads, unavailable credentials and upload deletion. They never use production credentials. `tests/browser/ocr.spec.js` covers both languages in Chromium, including PDF uploads, live progress, page previews, TXT/DOCX/JSON downloads, errors and unavailable OCR with working transcription. Set `OCR_SCREENSHOT_DIR` to an existing directory to save successful upload/result screenshots.

`python3 tests/deploy_env.py` verifies generated credential JSON through the installed Docker Compose parser, without starting containers or accessing real keys.

## Diacritization tests

The library tests cover queue cancellation, bounded engine reuse, Unicode text streaming, document validation, temporary-file cleanup, and download lifetime without loading models. `tests/tashkeel.rs` adds HTTP coverage for discovery, unavailable service, private results, and request limits. The ignored tests use both real models to check reference diacritics, text limits, source preservation, TXT/DOCX downloads, validation errors, and cancellation.

Provision the pinned artifacts and run the real-model tests on Linux x64:

```sh
sh deploy/onnxruntime.sh amd64 /tmp/arabi-onnxruntime
sh deploy/tashkeel-models.sh /tmp/arabi-tashkeel-models
export ORT_DYLIB_PATH=/tmp/arabi-onnxruntime/libonnxruntime.so
export TASHKEEL_MODEL_CACHE=/tmp/arabi-tashkeel-models
cargo test --locked -- --ignored --test-threads=1
```

Keep these variables set for the browser suite. `tests/browser/diacritization.spec.js` covers both languages, model switching without losing the source, copy/download, Unicode limits, TXT/DOCX uploads, validation errors, cancellation retries after network failures, completion winning a cancellation race, privacy, and unavailable service. DOCX regressions distinguish unsupported Word content from input/output limits and check that internal diagnostics never appear in the page. It uses real CPU inference. Set `DIACRITIZATION_SCREENSHOT_DIR` to an existing directory to save successful desktop and mobile screenshots.

## Search and transcript review

`search.spec.js` calls the bundled `arabiTools.search.find` on a real result page in Chromium and WebKit. It covers literal matching, exact/harakat/tatweel options, canonical equivalence, original UTF-16 grapheme spans, emoji, retained marks and letter distinctions, blank content and segment boundaries. Adapter tests cover unmounted pages, complete counts, focus, wraparound and stale work. WebKit must be installed for these focused cases (`npx --prefix tests/browser playwright install webkit`).

Highlights use CSS Custom Highlight ranges when available, with range-positioned backgrounds as the fallback; neither path splits text nodes or changes Arabic shaping. The fallback repaints after resizing and font loading. Browsers without `Intl.Segmenter` show a localized search-unavailable explanation while keeping reading, copy and downloads available. Copy-all reconstructs the TXT representation from immutable cue text, with an exceptional server-provided payload when SRT cue parsing folded original line breaks. Clipboard failure creates a selectable panel for the requested scope and reveals it with minimal scrolling; closing it or removing the result discards the panel.

`transcript.spec.js` checks both locales at desktop and 320 CSS pixels, initial/live result enhancement and disposal, copy scopes and denied/missing clipboard fallback, timestamp preferences, original TXT/SRT bytes and filenames, successful silence, and both highlight implementations in Chromium and WebKit. It generates 2160 five-second cues (a three-hour, roughly 26,000-word transcript) in the actual viewer, reports rendering/search measurements, and checks that the final cue remains searchable and copy-all contains every cue. No real recognition service is used. The mock's varied text is described in [fixtures/README.md](fixtures/README.md).

## OCR review browser coverage

`tests/browser/ocr-review.spec.js` covers paired source/text pages (including a blank middle page), Arabic/English desktop and 320px keyboard views, zoom/fit, PDF/image recovery without uploading, invalid page entries, clipboard fallback for the requested scope, unchanged downloads, and cleanup of workers/object URLs after a live result is removed. It exercises the public page adapter through `window.arabiTools.ocrReview.get(root)`; the shared search toolbar attaches in the `data-ocr-search-slot` integration slot. A 1,000-page fixture reports page-switch times, result element count, canvas bounds at high pixel density and Chromium heap/DOM measurements as a test attachment. Only the current source page is rasterized.

Run `npm test --prefix tests/browser -- ocr-review.spec.js` after the build and asset bundle above. For the focused WebKit run, install Playwright WebKit and use `OCR_REVIEW_BROWSER=webkit npm test --prefix tests/browser -- ocr-review.spec.js`. `OCR_SCREENSHOT_DIR` saves desktop, narrow, missing-source and preview-error evidence. On the shared fleet machine use its `browser-suite` wrapper for these commands. The synthetic source fixtures and their generator are documented in [fixtures/README.md](fixtures/README.md#ocr-review).
