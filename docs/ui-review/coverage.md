# UI enhancement audit coverage

This document maps the adopted audit to the final workspace implementation. The PR description records the current verification totals, and [the gallery](README.md) records the screenshot source and fixture limits. Test references describe what is covered; they do not imply that every test was rerun after every presentation-only change.

## Adopted defaults

| Choice | Adopted behavior |
| --- | --- |
| Source lifetime | Keep the uploaded File in the mounted browser workspace; server deletion stays unchanged. Reload/direct links can require local reselection. |
| Reselected source | Require confirmation that it is the original; validate format and page count and label it locally attached. A filename/page count does not prove identity. |
| Results | Read-only, with source editable between jobs and the existing processing lock retained. |
| OCR comparison | Page-level pairing, including blank pages. |
| Search | Literal within each cue/page, canonical comparison, visible independent common-harakat/tatweel options enabled by default. Latin matching is case-sensitive and Arabic letters are not substituted. |
| OCR copy-all | Join page text with two newlines; preserve the TXT export's PAGE_SEPARATOR representation. |
| URL/history | Keep the workspace URL and an accepted job's private recovery URL. Direct job URLs and downloads remain available. |
| Language switching | Retain the redirect, explain loss of unsent/local-only state, and permit source reselection afterward. |
| Persistence | Store preferences/attempt identity only; no automatic text/file drafts. |
| Retention | Static statement based on completion, with existing server lifetimes. |
| Deferred additions | Editable results, audio playback, new ASR/OCR cancellation endpoints, persistent drafts, server source retention, word boxes/line synchronization, DOCX previews, model changes, a new frontend framework, and the manual editor. |

The one wording deviation from the audit is “Status updates are delayed,” because the framework does not expose a confirmed disconnection signal. The page offers observation recovery without resubmission.

## Requirement and acceptance mapping

Rust paths are relative to the repository; browser filenames are relative to `tests/browser/`. Browser test names use `{locale}`/`{tool}` for the actual Arabic/English and tool expansions. Manual evidence and limitations are reported separately; reading a test is not counted as executing it.

| ID | Implementation | Evidence |
| --- | --- | --- |
| UI01 | Persistent controller and documented Topcoat signal/observer bridge; runtime follows controllers | `workspace.spec.js` — “diacritization proof keeps the document and source through two jobs”, “diacritization proof reconciles a submission before hydration”; both upload specs assert document and input identity |
| UI02 | Exact accepted `File` in `arabiTools.sources`, `arabi:source`, viewer-owned local reselection and disposal | `integration.spec.js` — “integrated OCR source, search, replacement and reload in {locale}”; `workspace-lifecycle.spec.js` — “{tool} keeps its input and source ownership across retry in {locale}, desktop and 320px” |
| UI03 | One current-job observation; terminal Watch/result release and same-job reconnect | `tests/tashkeel.rs` — `workspace_observer_keeps_a_job_alive_reconnects_and_releases_on_abandonment`; OCR/transcription observer tests; `workspace.spec.js` — reconnect proof |
| UI04 | Complete text index; one OCR page mounted, all transcript cues selectable | `search.spec.js` — whole-document adapter cases; `ocr-review.spec.js` — 1000-page case; `transcript.spec.js` — three-hour transcript case |
| UI05 | Reservation, native byte transfer, validation, queue and processing states are separate | `workspace-lifecycle.spec.js` — “upload stages distinguish reservation, transfer and validation and prevent double submit”; Rust queue-position observer test |
| UI06 | Shared `search.js`/toolbar, literal NFC grapheme matching, independent marks/tatweel toggles, wrapped navigation and delayed announcements | `search.spec.js` — “search acceptance examples in {engine}”, both whole-document highlight modes, stale-search case; integrated OCR/transcript cases |
| UI07 | Page-level local OCR source/text comparison, shared navigator, fit/zoom, narrow Original/Text choice, bounded rendering and local recovery | All `ocr-review.spec.js` cases; `integration.spec.js` OCR cases prove actual in-place source, search, blank page and replacement |
| UI08 | Stable cue IDs, cue selector, timestamp preference, separate content direction, cue/all copy | `transcript.spec.js` — “transcript search, reading and copy in {locale} at {width}px”, highlight/selection cases and long-result case |
| UI09 | One stable source editor, paired read-only output, producing-model label and retained previous result | `workspace.spec.js` two-job/cancellation proofs; `diacritization.spec.js` — “Diacritization retains input, copies, downloads and retries both models in {locale}” |
| UI10 | Exact 1000 Unicode-scalar count; no trim, normalization, maxlength or truncation | `diacritization.spec.js` — “Diacritization counts Unicode scalars and accepts exactly 1000 in {locale}”; Rust text-boundary and real-model tests |
| UI11 | Bulk TXT/DOCX input with retained local file, original and same-format downloads; no large in-page preview | `diacritization.spec.js` TXT/DOCX and invalid-document cases; `real_http_uploads_preserve_txt_docx_and_original_downloads`; existing disk-backed document tests |
| UI12 | Fresh HTTP cancellation, fixed job/attempt identity, retry while uncertain, completion wins, neutral cancelled state | `diacritization.spec.js` cancellation cases; `workspace-lifecycle.spec.js` before-reservation/missing-job reconciliation; existing native cleanup/slot test |
| UI13 | One shared tool/header width, compact titles, labelled source/result panes, wrapping header, named focusable regions and skip link | `shell.spec.js`; transcript/OCR narrow and focus cases; desktop/390/320 and enlarged-text browser checks |
| UI14 | Approved `--faint: #6e5f50`, `--gold-text: #7d6232`; consistent state tokens and active underline | Unrounded sRGB contrast calculations for every new state (selected, error, disabled, hover, focus; lowest 4.73:1, gold-text on danger-soft); browser focus/highlight checks |
| UI15 | Bilingual labels, concise polite statuses, field errors, native keyboard controls, reduced motion; result outside repeated announcements | Shell, workspace, transcript and OCR keyboard tests; `result-fallbacks.spec.js` focused-completion and no-JavaScript cases; no screen-reader test claimed |
| UI16 | Filename/size, replacement through file selection, Remove, duplicate lock and common copy helper with on-demand exact-scope fallback | `workspace-lifecycle.spec.js` — “upload stages distinguish reservation, transfer and validation and prevent double submit” and both-tool ownership cases; `integration.spec.js` shared-helper assertions; clipboard-denied/missing tests |
| UI17 | Shared `i18n.rs` vocabulary, canonical repository links, accurate local font documentation and collection cards | `shell.spec.js`, `file-labels.spec.js`, `src/i18n.rs`, and the collection image generator |

| ID | Passing behavior and automated evidence |
| --- | --- |
| T01 | `workspace.spec.js` — “diacritization proof keeps the document and source through two jobs”: both real models, same source/document, selection, textarea/document scroll and focus |
| T02 | Same two-job test and both-language diacritization retry cases: one new job, original input, correct producing model |
| T03 | `workspace.spec.js` — previous-output cancellation and stale/incomplete-rendering proofs; `integration.spec.js` refused OCR retry retains the prior source/text/download state |
| T04 | `workspace.spec.js` — “diacritization proof reconnects the same running job without another submission”; Rust short-interval connected/reconnect Watch test |
| T05 | `tests/tashkeel.rs` — `workspace_observer_keeps_a_job_alive_reconnects_and_releases_on_abandonment`; `tests/transcription.rs` — `workspace_observer_keeps_queue_position_private_and_releases_on_abandonment`; browser source-map disposal checks |
| T06 | `workspace.spec.js` previous-output cancellation proof uses a second job; both-language diacritization cancellation cases exercise current HTTP cookie and unrelated-browser refusal |
| T07 | `diacritization.spec.js` — uncertain-cancellation retry, completion winning, submission and actual processing cancellation; `workspace-lifecycle.spec.js` — missing-job reconciliation; Rust native cleanup retains its slot until completion |
| T08 | Both-language scalar-boundary browser cases and Rust Unicode/whitespace/model reference tests |
| T09 | Both-language TXT/DOCX and classified invalid-document browser cases; real HTTP TXT/DOCX/original bytes and existing DOCX structure/cleanup tests |
| T10 | `search.spec.js` — 43 acceptance/edge cases per named engine, including exact/common marks/tatweel, surviving marks, canonical equivalence, emoji, literal punctuation and retained letter distinctions |
| T11 | `search.spec.js` — whole-document adapter with both highlight modes; `integration.spec.js` real in-place transcript/OCR search; large-result final-cue/final-page reveal |
| T12 | `integration.spec.js` OCR cases verify red page 1, white blank page 2, blue page 3 and exact page text, search navigation, unchanged zoom and new-job replacement; OCR rapid-navigation tests |
| T13 | `ocr-review.spec.js` — “OCR missing source, local recovery and 320px keyboard views in {locale}”; integrated language/reload/reselection checks assert no new OCR POST |
| T14 | `ocr-review.spec.js` — preview failures, canvas-render failure, unreadable images, abandoned loading; text/copy/downloads and local retry stay usable |
| T15 | `transcript.spec.js` — “three-hour transcript keeps all cues searchable, selectable and copyable”; `ocr-review.spec.js` — “OCR 1000-page viewer bounds canvases and DOM while retaining every page text”; rendering/search measurements attached by these tests |
| T16 | `workspace-lifecycle.spec.js` — “upload stages distinguish reservation, transfer and validation and prevent double submit” uses a native XHR receiver to distinguish last-byte completion from a held validation reply; held reservation and real queue/processing tests cover other stages; the gallery identifies its held-reservation processing state |
| T17 | Restored `transcription.spec.js` locking/reload/duplicated-tab tests; lifecycle lost-reservation/accepted-upload-lost-reply/blocked-storage/tool-scoping cases; `review-recovery.spec.js` failed-before-receipt, late-upload, recovery-request/response/timeout and accepted-upload cases; Rust admission/recovery tests |
| T18 | All tool export tests preserved; integrated shared-helper assertions; transcript exact TXT/cue payload and OCR page/two-newline-all payload; denied/missing clipboard fallback; unchanged download bytes/filenames |
| T19 | `transcript.spec.js` — “silent audio stays successful and has no phantom search or copied explanation”; `ocr-review.spec.js` — “OCR all-blank document keeps every page and never copies placeholder text”; blank-middle tests |
| T20 | Lifecycle locale draft/accepted/active-job, reload/history cases and integrated OCR source recovery; direct result pages load the same scripts; no resubmission |
| T21 | `tests/tashkeel.rs` — `terminal_workspace_observer_releases_the_disk_result_while_socket_stays_open`; `tests/transcription.rs` — `workspace_terminal_observer_does_not_retain_a_transcript_after_expiry`; `tests/ocr.rs` — `workspace_observer_uses_the_ocr_queue_and_releases_its_completed_document`; existing started-download lifetime tests; browser download-unavailable simulation retains copyable text (not a real 30-minute browser wait) |
| T22 | Shell skip/native focus, workspace identity/focus/selection, OCR radio/navigation/pan, transcript direction/highlight/selection; both-language viewport/enlarged-text checks and `result-fallbacks.spec.js`; `review-recovery.spec.js` keyboard Clear, recovery-link focus, and localized progress warnings |
| T23 | `ocr-review.spec.js` — “OCR bundle modules, decoders and worker load from a deep route and dispose on removal”, “OCR preview renders a page whose fonts and character maps are not bundled” and “OCR preview decodes a JPEG 2000 scan with the bundled decoder only when needed”; release-container asset smoke in `.github/workflows/container.yml`; source/reselection request monitoring stays local |
| T24 | `ocr.spec.js` — “OCR unavailable in {locale}, transcription still available”; `diacritization.spec.js` — “Diacritization is unavailable without models in {locale}”; unavailable walkthrough confirms both tools and independent transcription in both engines/locales; Rust unavailable/missing-model/OCR-initialization coverage |

The deferred items remain editable results/regenerated exports, ASR playback, new ASR/OCR cancellation endpoints, persistent drafts, server upload retention, word boxes/line-synchronized OCR scrolling, DOCX previews, a frontend framework/build step, model changes and the manual editor. No required UI/T item is intentionally deferred.

## Review repairs

An earlier independent review reproduced three regressions in both interface languages: a focused recovery link disappeared at completion; a direct diacritization result without JavaScript positioned its actions over the site header; and the no-JavaScript OCR result hid every page heading. `result-fallbacks.spec.js` has six regression cases that failed before the repairs and check the corrected behavior in Chromium and WebKit. The fallback checks cover desktop and 320px layouts, visible page identity, result/action placement, and downloads. No-JavaScript submission remains outside scope.

A subsequent review of `6464f26` identified unused reservations locking failed uploads, hidden keep-open warnings, and Clear hiding its own keyboard focus. Upload recovery now asks a private POST route to atomically revoke an unused reservation before enabling retry. The same queue lock protects upload admission, so a late upload is rejected; receiving, accepted and finished work remains observed. Visible progress includes the existing localized keep-open warning while queued/running, and Clear returns focus to the search field before emptying it. `review-recovery.spec.js` covers these repairs in both languages, including lost responses for already accepted work; the queue and HTTP tests cover the recovery admission boundary.

## Verification limits

The recovery repair passes Rust/Topcoat formatting, Clippy with warnings denied, and `cargo test --locked`: 103 passed and 5 ignored. Targeted browser verification covers 50 passing cases in each of Chromium and WebKit: 26 recovery/warning/focus cases, 19 lifecycle cases, and 5 workspace cases. The two timeout cases were finally rerun with a real HTTP receiver in both engines, replacing the intercepted-request results; all other cases use the same application source. Six controlled progress captures confirm readable localized warnings without overflow at 320px. The broader default suite and production container checks remain separate CI checks for this push.

The preceding visual revision used source commit `3e62f6a`, published with the gallery at `6464f26`. Its formatting, clippy and `cargo test --locked` checks passed: 101 Rust tests passed and 5 were ignored. That browser evidence covers 120 passing cases per suite profile, counting each case once. Chromium uses 26 reruns after the final OCR menu repair, 49 earlier presentation/fallback passes and 45 unchanged passes; WebKit uses 51 final-repair runs, 24 earlier diacritization passes and 45 unchanged passes. Named cross-engine search/highlight tests still use their named engine. Incomplete resource-interrupted attempts are excluded. These are historical cumulative totals; the PR records the additional recovery-repair validation separately.

Those visual checks cover 32 states at 320px with 200% text in both engines/locales, four additional simulated-touch checks, and the gallery's 60 desktop/phone captures. They reported no page errors or horizontal overflow. The narrow OCR cases verify open zoom/copy/download menus, keyboard Escape and focus return with enlarged text.

Recognition uses the repository's local ASR/Drive mocks; diacritization browser cases use the provisioned real models. Desktop WebKit on Linux and simulated touch/enlarged-text contexts are not physical Safari/iOS or screen-reader testing. WebKit clipboard success uses exact-payload readback because Playwright cannot grant its clipboard permission; denied/missing clipboard fallback is exercised in both engines.

The vendored [PDF.js build](../../src/app/vendor/pdfjs/README.md) supplies the library, worker, and JBIG2/JPEG 2000 decoders locally. Unembedded fonts use system fonts; omitted legacy CJK character maps can leave some source-preview text undrawn. Recognized text and downloads remain available. Existing [font license notices](../../src/app/fonts/README.md) are still absent.

Browser expiry is simulated; short-interval Rust tests cover the actual retention logic and active-download lifetime. Container asset smoke is a separate CI check; browser asset checks alone are not evidence that a new container build passed.
