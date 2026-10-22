Run `cargo test --locked --test transcription` with `ffmpeg` and `ffprobe` on PATH. The tests start the real router and a local Wit mock on ephemeral loopback ports, and bundle assets from the test executable. No Wit credentials or running site are needed. See [fixtures/README.md](fixtures/README.md) for the speech recordings and regeneration commands.

Fonts are [vendored with their licenses](../src/app/fonts/README.md). After fetching Cargo dependencies, check that asset bundling and tests need no external downloads with the following commands. The proxy settings block outbound HTTP(S) while allowing the local mock; the integration tests bundle assets into a fresh temporary cache.

```sh
export CARGO_NET_OFFLINE=true
export HTTP_PROXY=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 ALL_PROXY=http://127.0.0.1:9
export NO_PROXY=127.0.0.1,localhost,::1
export http_proxy=$HTTP_PROXY https_proxy=$HTTPS_PROXY all_proxy=$ALL_PROXY no_proxy=$NO_PROXY
topcoat asset bundle --bin arlang-tools
cargo test --locked
```

`examples/mock_wit.rs` documents the two commands for checking the site in a browser, including a configurable delay per dictation request.

