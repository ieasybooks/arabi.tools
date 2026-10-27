`speech.mp3` and `speech.m4a` contain 11 seconds of public-domain speech by John F. Kennedy, converted from `tafrigh-rs/crates/tafrigh/tests/fixtures/short.s16le` (16 kHz, mono, signed 16-bit little-endian PCM). The recordings exercise real decoding and Silero speech segmentation; the local Wit mock supplies the Arabic transcript.

To regenerate from that checkout:

```sh
ffmpeg -nostdin -f s16le -ar 16000 -ac 1 -i ../tafrigh-rs/crates/tafrigh/tests/fixtures/short.s16le -map_metadata -1 -c:a libmp3lame -b:a 24k tests/fixtures/speech.mp3
ffmpeg -nostdin -f s16le -ar 16000 -ac 1 -i ../tafrigh-rs/crates/tafrigh/tests/fixtures/short.s16le -map_metadata -1 -c:a aac -b:a 24k tests/fixtures/speech.m4a
```

The over-three-hours fixture is generated in a temporary directory by its test and is not committed. Integration tests require `ffmpeg` and `ffprobe` on PATH, bundle assets from their own executable, and make no requests to the real Wit service.

The browser mock in `examples/mock_wit.rs` cycles three distinct responses: `مرحبا بالعالم كِتَاب مُدَرِّس كتاب`, `مـدرسة عـَلَم أمل إمل آمل كتاب`, and `😀 كتاب Arabic 123 <b> & "نص" [a.*] café`. Request order chooses the response, so a test should inspect cue order rather than assume that every recording starts at the first sample. These authored fixture strings exercise common marks, shadda, tatweel carrying a mark, distinct alef/hamza forms, repeated words, mixed direction, emoji and literal markup-looking text. The audio fixtures and actual speech segmentation are unchanged. Rust integration tests keep their own independent Wit mock and assertions.

Search tests add in-memory canonical-equivalence, retained-mark, blank-page and cross-segment cases. The transcript browser spec generates a one-second silent MP3 under its test output directory and a three-hour-equivalent 2160-cue document in the rendered viewer; neither adds a large binary fixture to the repository.
