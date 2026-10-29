# Tool workspace screenshots

Captured on 2026-10-28 from UI source committed as `3e62f6a`. These are screenshots of the local application using the repository's audio/PDF fixtures, mock recognition services, and real diacritization models. The numbered PDF and mixed-script transcript are test data, not examples of recognition accuracy. The images are documentation only, excluded from the application bundle and Docker context.

Desktop captures use a 1280px viewport; phone captures use a 390px viewport in desktop Chromium. They demonstrate responsive layout, not a physical-device test. “Waiting” captures hold the reservation response to show the actual queue-request state; they do not imply a measured processing percentage. Search screenshots show the active viewport; the other captures show the full page. The OCR error is a deliberately refused TXT file.

[Audit coverage and adopted defaults](coverage.md) · [Browser test guide](../../tests/README.md)

## Representative results

| Tool | Desktop | Phone |
| --- | --- | --- |
| Diacritization | [Arabic original/result comparison](diacritize-ar-desktop.png) | [English workspace](diacritize-en-phone.png) |
| Transcription | [Arabic search results](transcribe-ar-desktop.png) | [English transcript](transcribe-en-phone.png) |
| OCR | [English original/text comparison](ocr-en-desktop.png) | [Arabic text view](ocr-ar-phone.png) |

## All captured states

### Diacritization

| Language / width | Input | Ready | Waiting | Result |
| --- | --- | --- | --- | --- |
| Arabic / 1280px | [Input](states/ar-1280-diacritize-1-empty.png) | [Ready](states/ar-1280-diacritize-2-ready.png) | [Waiting](states/ar-1280-diacritize-3-processing.png) | [Result](states/ar-1280-diacritize-4-result.png) |
| Arabic / 390px | [Input](states/ar-390-diacritize-1-empty.png) | [Ready](states/ar-390-diacritize-2-ready.png) | [Waiting](states/ar-390-diacritize-3-processing.png) | [Result](states/ar-390-diacritize-4-result.png) |
| English / 1280px | [Input](states/en-1280-diacritize-1-empty.png) | [Ready](states/en-1280-diacritize-2-ready.png) | [Waiting](states/en-1280-diacritize-3-processing.png) | [Result](states/en-1280-diacritize-4-result.png) |
| English / 390px | [Input](states/en-390-diacritize-1-empty.png) | [Ready](states/en-390-diacritize-2-ready.png) | [Waiting](states/en-390-diacritize-3-processing.png) | [Result](states/en-390-diacritize-4-result.png) |

### Transcription

| Language / width | Input | Ready | Waiting | Result | Search |
| --- | --- | --- | --- | --- | --- |
| Arabic / 1280px | [Input](states/ar-1280-transcribe-1-empty.png) | [Ready](states/ar-1280-transcribe-2-ready.png) | [Waiting](states/ar-1280-transcribe-3-processing.png) | [Result](states/ar-1280-transcribe-4-result.png) | [Search](states/ar-1280-transcribe-5-search.png) |
| Arabic / 390px | [Input](states/ar-390-transcribe-1-empty.png) | [Ready](states/ar-390-transcribe-2-ready.png) | [Waiting](states/ar-390-transcribe-3-processing.png) | [Result](states/ar-390-transcribe-4-result.png) | [Search](states/ar-390-transcribe-5-search.png) |
| English / 1280px | [Input](states/en-1280-transcribe-1-empty.png) | [Ready](states/en-1280-transcribe-2-ready.png) | [Waiting](states/en-1280-transcribe-3-processing.png) | [Result](states/en-1280-transcribe-4-result.png) | [Search](states/en-1280-transcribe-5-search.png) |
| English / 390px | [Input](states/en-390-transcribe-1-empty.png) | [Ready](states/en-390-transcribe-2-ready.png) | [Waiting](states/en-390-transcribe-3-processing.png) | [Result](states/en-390-transcribe-4-result.png) | [Search](states/en-390-transcribe-5-search.png) |

### OCR

| Language / width | Input | Ready | Waiting | Result | Search | Error |
| --- | --- | --- | --- | --- | --- | --- |
| Arabic / 1280px | [Input](states/ar-1280-ocr-1-empty.png) | [Ready](states/ar-1280-ocr-2-ready.png) | [Waiting](states/ar-1280-ocr-3-processing.png) | [Result](states/ar-1280-ocr-4-result.png) | [Search](states/ar-1280-ocr-5-search.png) | [Error](states/ar-1280-ocr-6-error.png) |
| Arabic / 390px | [Input](states/ar-390-ocr-1-empty.png) | [Ready](states/ar-390-ocr-2-ready.png) | [Waiting](states/ar-390-ocr-3-processing.png) | [Result](states/ar-390-ocr-4-result.png) | [Search](states/ar-390-ocr-5-search.png) | [Error](states/ar-390-ocr-6-error.png) |
| English / 1280px | [Input](states/en-1280-ocr-1-empty.png) | [Ready](states/en-1280-ocr-2-ready.png) | [Waiting](states/en-1280-ocr-3-processing.png) | [Result](states/en-1280-ocr-4-result.png) | [Search](states/en-1280-ocr-5-search.png) | [Error](states/en-1280-ocr-6-error.png) |
| English / 390px | [Input](states/en-390-ocr-1-empty.png) | [Ready](states/en-390-ocr-2-ready.png) | [Waiting](states/en-390-ocr-3-processing.png) | [Result](states/en-390-ocr-4-result.png) | [Search](states/en-390-ocr-5-search.png) | [Error](states/en-390-ocr-6-error.png) |
