`speech.mp3` and `speech.m4a` contain 11 seconds of public-domain speech by John F. Kennedy, converted from `tafrigh-rs/crates/tafrigh/tests/fixtures/short.s16le` (16 kHz, mono, signed 16-bit little-endian PCM). The recordings exercise real decoding and Silero speech segmentation; the local Wit mock supplies the Arabic transcript.

To regenerate from that checkout:

```sh
ffmpeg -nostdin -f s16le -ar 16000 -ac 1 -i ../tafrigh-rs/crates/tafrigh/tests/fixtures/short.s16le -map_metadata -1 -c:a libmp3lame -b:a 24k tests/fixtures/speech.mp3
ffmpeg -nostdin -f s16le -ar 16000 -ac 1 -i ../tafrigh-rs/crates/tafrigh/tests/fixtures/short.s16le -map_metadata -1 -c:a aac -b:a 24k tests/fixtures/speech.m4a
```

The over-three-hours fixture is generated in a temporary directory by its test and is not committed. Integration tests require `ffmpeg` and `ffprobe` on PATH, bundle assets from their own executable, and make no requests to the real Wit service.

The browser mock in `examples/mock_wit.rs` cycles three distinct responses: `مرحبا بالعالم كِتَاب مُدَرِّس كتاب`, `مـدرسة عـَلَم أمل إمل آمل كتاب`, and `😀 كتاب Arabic 123 <b> & "نص" [a.*] café`. Request order chooses the response, so a test should inspect cue order rather than assume that every recording starts at the first sample. These authored fixture strings exercise common marks, shadda, tatweel carrying a mark, distinct alef/hamza forms, repeated words, mixed direction, emoji and literal markup-looking text. The audio fixtures and actual speech segmentation are unchanged. Rust integration tests keep their own independent Wit mock and assertions.

Search tests add in-memory canonical-equivalence, retained-mark, blank-page and cross-segment cases. The transcript browser spec generates a one-second silent MP3 under its test output directory and a three-hour-equivalent 2160-cue document in the rendered viewer; neither adds a large binary fixture to the repository.

## OCR review

`ocr-review.pdf` is an original, generated three-page document: a red page marked 1 with embedded Arabic text, a blank white middle page, and a blue page marked 3. `ocr-review.png` and `ocr-review.jpg` are the first page as image inputs. Regenerate them with `node tests/fixtures/generate-ocr.mjs` after installing the browser test dependencies and Chromium. The generator uses the repository's Kitab font; the PDF embeds its used glyphs. These are synthetic fixtures, with no personal documents or external inputs.

The browser Drive mock recognizes the first/third fixture by its raster shade (also distinct after grayscale conversion), independently of concurrent request order, and returns distinct mixed Arabic/Latin text. Large blank pages return an empty string. Small white fixtures used by existing validation/export tests retain their old mock text. The 1,000-page browser-performance PDF is generated in `ocr-review.spec.js`, including an oversized final page; its completed text is inserted at the documented result-root boundary so this viewer check does not run 1,000 mock recognition calls. The spec also generates a one-page PDF whose fonts are not embedded and, with `ffmpeg`, a one-page JPEG 2000 scan.
