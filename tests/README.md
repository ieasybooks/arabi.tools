# Tests

## Rust tests

Run `cargo test --locked` with `ffmpeg` (built with libopus) and `ffprobe` on PATH (if `/tmp` is a small tmpfs, point `TMPDIR` at a directory on disk). Unit tests live next to the code. `tests/transcription.rs` starts the real router against a local Wit mock on ephemeral ports and drives it over HTTP: MP3 and M4A uploads through to the downloads, refusals and queue limits, upstream failures, silent recordings, Arabic and English recognition, upload-size boundaries, and both page languages. Assets are bundled from the test executable and the fonts are vendored, so the tests need no network once dependencies are fetched. See [fixtures/README.md](fixtures/README.md) for the recordings.

## Browser test

`tests/browser/` drives the real site in headless Chromium with Playwright, in Arabic and in English: choosing a file, the form staying locked while it is sent, the job page updating live until done, both downloads, a reloaded tab retrying after losing its reservation, and a tab opened from the upload page being refused while that page's upload is on its way. Run it from the repository root:

```sh
npm ci --prefix tests/browser
npx --prefix tests/browser playwright install --with-deps --only-shell chromium
cargo build --locked --bin arlang-tools --example mock_wit --example mock_ocr
topcoat asset bundle --bin arlang-tools
npm test --prefix tests/browser
```

It starts mock Wit on port 9804, the site with mock Drive on port 3204, and the site without OCR credentials on port 3205. All three ports must be free. Set `TAHWEEL_TEST_PDFIUM` to the pinned PDFium library for OCR browser tests. CI runs it on pull requests and keeps traces and screenshots of failures as the `browser-failures` artifact.

`examples/mock_wit.rs` explains how to try the site by hand without Wit tokens.

## OCR tests

`tests/ocr.rs` uses a local Drive mock with fresh generated keys, PNG/JPEG images and generated PDFs. Set `TAHWEEL_TEST_PDFIUM` to include PDF validation, the 1,000-page limit recheck and cancellation cleanup tests. CI installs the pinned library using `deploy/pdfium.sh`. The tests cover both interface languages, all three downloads, progress, queue limits, invalid uploads, unavailable credentials and upload deletion. They never use production credentials. `tests/browser/ocr.spec.js` covers both languages in Chromium, including PDF uploads, live progress, page previews, TXT/DOCX/JSON downloads, errors and unavailable OCR with working transcription. Set `OCR_SCREENSHOT_DIR` to an existing directory to save successful upload/result screenshots.

`python3 tests/deploy_env.py` verifies generated credential JSON through the installed Docker Compose parser, without starting containers or accessing real keys.
