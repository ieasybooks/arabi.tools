//! The transcription tool's upload page.

mod jobs;

use serde_json::json;
use topcoat::{
    Result,
    asset::{Asset, asset},
    context::{Cx, app_context},
    icon::icon,
    router::{
        href, page,
        request::{client_ip, uri},
    },
    runtime::signal,
    view::{View, component, view},
};

use crate::i18n::Locale;
use crate::icons;
use crate::meta::PageMeta;
use crate::queue::Rejection;
use crate::result_controls::{COPY_SCRIPT, SEARCH_SCRIPT};
use crate::tools::transcription::{EXTENSIONS, MAX_UPLOAD_BYTES, Transcription};
use crate::workspace::FileKind;

pub(super) const REVIEW_SCRIPT: Asset = asset!("./transcript.js");

pub(super) const UPLOAD_SCRIPT: Asset = asset!("./upload.js");

/// The kinds of file this tool accepts, for the chosen file's icon.
const KINDS: &[FileKind] = &[FileKind::Audio];

#[page]
pub async fn page(cx: &Cx) -> Result<impl View> {
    let locale = Locale::of(cx);
    let text = &locale.text().transcription;
    let transcription = app_context::<Transcription>(cx);
    Ok(view! {
        tool_hero(locale: locale, lead: true)
        <section class="workspace">
            <div class="container workspace-layout">
                <div class="workspace-entry">
                    match transcription.queue.availability(client_ip(cx)) {
                        Err(rejection) => {
                            <div class="workspace-card">
                                <div class="notice">
                                    icon(data: icons::WARNING, size: 20)
                                    <div class="notice-body">
                                        <p>(locale.rejection(rejection))</p>
                                        if rejection == Rejection::ActiveJob {
                                            if let Some(job) = jobs::unfinished_job(cx) {
                                                <p><a href=(job)>(text.back_to_job)</a></p>
                                            }
                                        }
                                    </div>
                                </div>
                            </div>
                        }
                        Ok(()) => {
                            upload_form(locale: locale)
                        }
                    }
                </div>
            </div>
        </section>
        <script src=(crate::workspace::SCRIPT) defer=(true)></script>
        <script src=(UPLOAD_SCRIPT) defer=(true)></script>
        <script src=(COPY_SCRIPT) defer=(true)></script>
        <script src=(SEARCH_SCRIPT) defer=(true)></script>
        <script src=(REVIEW_SCRIPT) defer=(true)></script>
    })
}

/// This input stays mounted while the shard observes only the accepted job.
#[component]
async fn upload_form(cx: &Cx, locale: Locale) -> Result<impl View> {
    let text = &locale.text().transcription;
    let workspace = &locale.text().workspace;
    let job = signal(cx, String::new);
    Ok(view! {
        <div data-workspace="transcript" data-previous=(workspace.previous_result)>
            <form
                id="upload-form"
                class="upload workspace-input"
                method="post"
                action=(href!(page))
                novalidate=(true)
                data-tool="transcribe"
                data-reserve=(href!(jobs::reserve))
                data-extensions=(EXTENSIONS.join(","))
                data-max-bytes=(MAX_UPLOAD_BYTES)
                data-missing=(text.missing)
                data-wrong-type=(text.wrong_type)
                data-wrong-file=(text.wrong_file)
                data-too-large=(locale.audio_too_large())
                data-failed=(text.upload_failed)
                data-uploading=(text.uploading)
                data-reserving=(workspace.reserving)
                data-validating=(workspace.validating)
                data-observing=(workspace.observing)
                data-bytes=(workspace.bytes)
                data-kilobytes=(workspace.kilobytes)
                data-megabytes=(workspace.megabytes)
            >
                <div class="dropzone">
                    <input
                        id="audio"
                        type="file"
                        name="file"
                        accept=".mp3,.m4a,audio/mpeg,audio/mp4,audio/x-m4a"
                        aria-label=(text.file_label)
                        aria-describedby="upload-error"
                    >
                    <span class="dropzone-icon">
                        icon(data: icons::UPLOAD, size: 24)
                    </span>
                    <p class="dropzone-title">(text.choose_file)</p>
                    <p class="dropzone-hint">(locale.upload_limits())</p>
                    crate::workspace::file_row(locale: locale, kinds: KINDS)
                </div>
                <p id="upload-error" class="error" role="alert" hidden=(true)></p>
                <a class="return-to-job" hidden=(true)>(text.back_to_job)</a>
                <div class="form-actions">
                    <button type="submit" class="button primary" disabled=(true)>
                        (text.submit)
                    </button>
                </div>
                <div class="upload-progress" hidden=(true)>
                    <progress max="100" aria-label=(text.uploading)></progress>
                    <p id="job-status" class="submission-status" role="status"></p>
                    <p class="submission-percent" dir="ltr"></p>
                    <p class="server-detail hint"></p>
                    crate::workspace::progress_links(
                        locale: locale,
                        keep_open: text.keep_open
                    )
                </div>
            </form>
            crate::workspace::privacy_note(text: locale.privacy_note())
            crate::workspace::bridge(job: job.clone())
            <div class="workspace-observer" hidden=(true)>
                jobs::job::observe(job: $(job.get()))
            </div>
            crate::workspace::review(locale: locale, placeholder: None)
        </div>
        <noscript><p class="notice">(text.needs_javascript)</p></noscript>
    })
}

/// What the tool's pages tell search engines and link previews: the upload page is indexed and
/// described as a free web app; job pages are private. `None` for other paths.
pub(crate) fn meta(cx: &Cx, locale: Locale, origin: &str) -> Option<PageMeta> {
    let text = locale.text();
    let page_path = href!(page).resolve(cx);
    let path = uri(cx).path();
    let title = locale.page_title(text.transcription.name);
    let description = locale.transcription_summary();
    if path == page_path {
        let structured_data = json!({
            "@context": "https://schema.org",
            "@type": "WebApplication",
            "name": text.transcription.name,
            "url": format!("{origin}{page_path}"),
            "description": description,
            "inLanguage": locale.code(),
            "applicationCategory": "MultimediaApplication",
            "operatingSystem": "Any",
            "browserRequirements": "Requires JavaScript",
            "isAccessibleForFree": true,
            "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"},
            "isPartOf": {"@type": "WebSite", "name": text.site_name, "url": format!("{origin}/")},
        });
        Some(PageMeta {
            title,
            description,
            index: true,
            structured_data: Some(structured_data),
        })
    } else if path.starts_with(&format!("{page_path}/")) {
        Some(PageMeta::private(title, description))
    } else {
        None
    }
}

/// The tool's title, with a link back to the list of tools and, on the upload page, its
/// description.
#[component]
pub(crate) async fn tool_hero(locale: Locale, lead: bool) -> Result<impl View> {
    let text = locale.text();
    Ok(view! {
        <section class="tool-hero">
            <div class="container">
                <p class="crumbs">
                    <a href=(href!(super::home))>(text.transcription.all_tools)</a>
                </p>
                <h1>(text.transcription.name)</h1>
                if lead {
                    <p class="lead">(text.transcription.description)</p>
                }
            </div>
        </section>
    })
}
