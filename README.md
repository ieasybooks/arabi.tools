# arabi.tools

Free Arabic language tools on the web at [arabi.tools](https://arabi.tools): audio transcription, PDF/image text recognition, and text/TXT/DOCX diacritization with Shakkelha and CATT, with Arabic and English interfaces. A project of the nonprofit [iEasyBooks](https://ieasybooks.com) initiative.

Each tool keeps your input in place while it works. Search and copy transcripts or recognized pages, compare OCR text with the original PDF/image in your browser, and review diacritized text beside its source. Results stay read-only and retain their existing downloads. The job link lets you reopen a result; OCR previews need the original file selected again after a reload.

## Run locally

Install the pinned Rust toolchain and development tools with `mise install`, set `WIT_TOKENS_AR` to one or more comma-separated Wit tokens, then run `topcoat dev` and open `http://127.0.0.1:3000`. Topcoat builds the app and its assets; `ffmpeg` (built with libopus, as Wit receives Opus audio) and `ffprobe` must be on `PATH`.

Without Wit tokens, run a local stand-in for Wit with `cargo run --example mock_wit`, then start the site with `WIT_API_URL=http://127.0.0.1:9876 WIT_TOKENS_AR=dev topcoat dev`.

For OCR, set `DRIVE_SERVICE_ACCOUNTS` to a JSON array of service-account keys with Drive storage and make PDFium chromium/8111 available beside the binary or via `TAHWEEL_PDFIUM`. Without valid OCR settings, transcription still works and OCR shows as temporarily unavailable. See [deployment](docs/deploy.md#ocr-runtime-and-credentials) for credential formatting and runtime packaging.

For diacritization, provision the pinned models and ONNX Runtime 1.31.0 before starting the site, then set `TASHKEEL_MODEL_CACHE` to the shared model cache and `ORT_DYLIB_PATH` to the runtime library. The container includes both; local setup and real-model test commands are in [deployment](docs/deploy.md#diacritization-runtime-and-models). Missing or invalid files leave diacritization temporarily unavailable while other configured tools keep working. Inference uses the `tashkeel` 0.1.0 Rust library on the CPU; submitting a job never downloads models.

Gilroy, Noto Naskh Arabic UI, Thmanyah Serif Display and Kitab are [bundled locally](src/app/fonts/README.md), so asset bundling needs no font downloads.

Run the checks listed in [AGENTS.md](AGENTS.md#verification) before sending changes; [tests/README.md](tests/README.md) describes the integration tests.

## Deploy

See [deployment, releases, and rollback](docs/deploy.md) for the Docker Compose and Cloudflare Tunnel setup.
