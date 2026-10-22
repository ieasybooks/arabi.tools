`speech.mp3` and `speech.m4a` contain 11 seconds of public-domain speech by John F. Kennedy, converted from `tafrigh-rs/crates/tafrigh/tests/fixtures/short.s16le` (16 kHz, mono, signed 16-bit little-endian PCM). The recordings exercise real decoding and Silero speech segmentation; the local Wit mock supplies the Arabic transcript.

To regenerate from that checkout:

```sh
ffmpeg -nostdin -f s16le -ar 16000 -ac 1 -i ../tafrigh-rs/crates/tafrigh/tests/fixtures/short.s16le -map_metadata -1 -c:a libmp3lame -b:a 24k tests/fixtures/speech.mp3
ffmpeg -nostdin -f s16le -ar 16000 -ac 1 -i ../tafrigh-rs/crates/tafrigh/tests/fixtures/short.s16le -map_metadata -1 -c:a aac -b:a 24k tests/fixtures/speech.m4a
```

The over-three-hours fixture is generated in a temporary directory by its test and is not committed. Integration tests require `ffmpeg` and `ffprobe` on PATH, bundle assets from their own executable, and make no requests to the real Wit service.
