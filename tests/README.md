Run `cargo test --locked --test transcription` with `ffmpeg` and `ffprobe` on PATH. The tests start the real router and a local Wit mock on ephemeral loopback ports, and bundle assets from the test executable. No Wit credentials or running site are needed. See [fixtures/README.md](fixtures/README.md) for the speech recordings and regeneration commands.

`examples/mock_wit.rs` documents the two commands for checking the site in a browser, including a configurable delay per dictation request.
