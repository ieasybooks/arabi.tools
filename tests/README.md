# Tests

## Rust tests

Run `cargo test --locked` with `ffmpeg` (built with libopus) and `ffprobe` on PATH (if `/tmp` is a small tmpfs, point `TMPDIR` at a directory on disk). Unit tests live next to the code. `tests/transcription.rs` starts the real router against a local Wit mock on ephemeral ports and drives it over HTTP: MP3 and M4A uploads through to the downloads, refusals and queue limits, upstream failures, silent recordings, Arabic and English recognition, upload-size boundaries, and both page languages. Assets are bundled from the test executable and the fonts are vendored, so the tests need no network once dependencies are fetched. See [fixtures/README.md](fixtures/README.md) for the recordings.

## Browser test

`tests/browser/` drives the real site in headless Chromium with Playwright, in Arabic and in English: choosing a file, the form staying locked while it is sent, the job page updating live until done, both downloads, a reloaded tab retrying after losing its reservation, and a tab opened from the upload page being refused while that page's upload is on its way. Run it from the repository root:

```sh
npm ci --prefix tests/browser
npx --prefix tests/browser playwright install --with-deps --only-shell chromium
cargo build --locked --bin arlang-tools --example mock_wit
topcoat asset bundle --bin arlang-tools
npm test --prefix tests/browser
```

It starts the mock Wit on port 9804 and the site on port 3204, so both must be free. CI runs it on pull requests and keeps traces and screenshots of failures as the `browser-failures` artifact.

`examples/mock_wit.rs` explains how to try the site by hand without Wit tokens.
