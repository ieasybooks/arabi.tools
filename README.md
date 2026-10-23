# arabi.tools

Free Arabic language tools on the web at [arabi.tools](https://arabi.tools), starting with audio transcription, with Arabic and English interfaces. A project of the nonprofit [iEasyBooks](https://ieasybooks.com) initiative.

## Run locally

Install the pinned Rust toolchain and development tools with `mise install`, set `WIT_TOKENS_AR` to one or more comma-separated Wit tokens, then run `topcoat dev` and open `http://127.0.0.1:3000`. Topcoat builds the app and its assets; `ffmpeg` (built with libopus, as Wit receives Opus audio) and `ffprobe` must be on `PATH`.

Without Wit tokens, run a local stand-in for Wit with `cargo run --example mock_wit`, then start the site with `WIT_API_URL=http://127.0.0.1:9876 WIT_TOKENS_AR=dev topcoat dev`.

Inter and IBM Plex Sans Arabic are [vendored with their licenses](src/app/fonts/README.md), so asset bundling needs no font downloads.

Run the checks listed in [AGENTS.md](AGENTS.md#verification) before sending changes; [tests/README.md](tests/README.md) describes the integration tests.

## Deploy

See [deployment, releases, and rollback](docs/deploy.md) for the Docker Compose and Cloudflare Tunnel setup.
