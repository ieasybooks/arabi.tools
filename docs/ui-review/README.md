# Tool workspace screenshots

Representative Arabic and English workspace states captured on 2026-10-28 from UI source commit `fdd2f55`. These are screenshots of the local application using the repository's audio/PDF fixtures, mock recognition services, and real diacritization models. The numbered PDF and mixed-script transcript are test data, not examples of recognition accuracy. The images are documentation only and are not bundled into the application.

Desktop captures use a 1280px viewport; phone captures use a 390px viewport in desktop Chromium. They demonstrate responsive layout, not a physical-device test. Browser coverage and the local runtime setup are documented in [the test guide](../../tests/README.md).

| Tool | Desktop | Phone |
| --- | --- | --- |
| Diacritization | [Arabic original/result comparison](diacritize-ar-desktop.png) | [English workspace](diacritize-en-phone.png) |
| Transcription | [Arabic search results](transcribe-ar-desktop.png) | [English transcript](transcribe-en-phone.png) |
| OCR | [English original/text comparison](ocr-en-desktop.png) | [Arabic text view](ocr-ar-phone.png) |
