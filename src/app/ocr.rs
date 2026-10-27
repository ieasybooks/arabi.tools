//! The OCR tool's upload page.

mod jobs;

use std::cmp::Ordering;

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
use crate::tools::ocr::{EXTENSIONS, MAX_UPLOAD_BYTES, Ocr};

pub(super) const UPLOAD_SCRIPT: Asset = asset!("./upload.js");

#[page]
pub async fn page(cx: &Cx) -> Result<impl View> {
    let locale = Locale::of(cx);
    let text = &locale.text().ocr;
    let ocr = app_context::<Ocr>(cx);
    Ok(view! {
        tool_hero(locale: locale, lead: true)
        <section class="workspace">
            <div class="container workspace-layout workspace-wide">
                <div class="workspace-entry">
                    steps(locale: locale, current: Step::Upload, failed: false)
                    if !ocr.is_available() {
                        <p class="notice" role="status">(text.unavailable)</p>
                    } else {
                        match ocr.queue.availability(client_ip(cx)) {
                            Err(rejection) => {
                                <div class="notice">
                                    icon(data: icons::WARNING, size: 20)
                                    <div class="notice-body">
                                        <p>(locale.ocr_rejection(rejection))</p>
                                        if rejection == Rejection::ActiveJob {
                                            if let Some(job) = jobs::unfinished_job(cx) {
                                                <p><a href=(job)>(text.back_to_job)</a></p>
                                            }
                                        }
                                    </div>
                                </div>
                            }
                            Ok(()) => {
                                upload_form(locale: locale)
                            }
                        }
                    }
                </div>
                before_you_start(locale: locale)
            </div>
        </section>
        <script src=(super::workspace::SCRIPT) defer=(true)></script>
        <script src=(UPLOAD_SCRIPT) defer=(true)></script>
        // Integration: load the ocr viewer scripts here, on the page, before runtime hydration.
    })
}

/// This input stays mounted while the shard observes only the accepted job.
#[component]
async fn upload_form(cx: &Cx, locale: Locale) -> Result<impl View> {
    let text = &locale.text().ocr;
    let workspace = &locale.text().workspace;
    let job = signal(cx, String::new);
    Ok(view! {
        <div data-workspace="ocr" data-previous=(workspace.previous_result)>
            <form
                id="upload-form"
                class="upload workspace-input"
                method="post"
                action=(href!(page))
                novalidate=(true)
                data-tool="ocr"
                data-reserve=(href!(jobs::reserve))
                data-extensions=(EXTENSIONS.join(","))
                data-max-bytes=(MAX_UPLOAD_BYTES)
                data-missing=(text.missing)
                data-wrong-type=(text.wrong_type)
                data-too-large=(locale.too_large())
                data-failed=(text.upload_failed)
                data-no-file=(text.no_file)
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
                        id="document"
                        type="file"
                        name="file"
                        accept=".pdf,.jpg,.jpeg,.png,application/pdf,image/jpeg,image/png"
                        aria-label=(text.file_label)
                        aria-describedby="upload-error"
                    >
                    <span class="dropzone-icon">
                        icon(data: icons::UPLOAD, size: 24)
                    </span>
                    <p class="dropzone-title">(text.choose_file)</p>
                    <p class="dropzone-hint">(locale.ocr_limits())</p>
                    <p class="file-name" dir="auto">(text.no_file)</p>
                </div>
                super::workspace::file_actions(locale: locale)
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
                </div>
                <p id="upload-error" class="error" role="alert" hidden=(true)></p>
                <a class="return-to-job" hidden=(true)>(text.back_to_job)</a>
            </form>
            super::workspace::bridge(job: job.clone())
            <div class="workspace-observer" hidden=(true)>
                jobs::job::observe(job: $(job.get()))
            </div>
            super::workspace::review(locale: locale)
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
    let title = locale.page_title(text.ocr.name);
    let description = text.ocr.description.to_owned();
    if path == page_path {
        let structured_data = json!({
            "@context": "https://schema.org",
            "@type": "WebApplication",
            "name": text.ocr.name,
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

/// Notes under the form: automatic language detection and when files are deleted.
#[component]
async fn before_you_start(locale: Locale) -> Result<impl View> {
    Ok(view! {
        <aside class="workspace-aside">
            <h2 class="aside-title">(locale.text().ocr.before_you_start)</h2>
            <ul class="notes" role="list">
                <li>
                    icon(data: icons::INFO, size: 20)
                    <p>(locale.text().ocr.auto_language)</p>
                </li>
                <li>
                    icon(data: icons::LOCK, size: 20)
                    <p>(locale.text().ocr.privacy)</p>
                </li>
            </ul>
        </aside>
    })
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
                    <a href=(href!(super::home))>(text.ocr.all_tools)</a>
                </p>
                <h1>(text.ocr.name)</h1>
                if lead {
                    <p class="lead">(text.ocr.description)</p>
                }
            </div>
        </section>
    })
}

/// A step of an OCR, as shown above the form and the job's status.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Step {
    Upload,
    Recognize,
    Download,
}

/// The three steps of an OCR, with `current` marked, and failed if `failed`.
#[component]
pub(crate) async fn steps(locale: Locale, current: Step, failed: bool) -> Result<impl View> {
    let text = &locale.text().ocr;
    let all = [
        (Step::Upload, text.step_upload),
        (Step::Recognize, text.step_recognize),
        (Step::Download, text.step_download),
    ];
    Ok(view! {
        <ol class="steps" role="list" aria-label=(text.steps_label)>
            for (number, (step, label)) in (1..).zip(all) {
                <li
                    class="step"
                    data-state=(match step.cmp(&current) {
                        Ordering::Less => "done",
                        Ordering::Equal if failed => "failed",
                        Ordering::Equal => "current",
                        Ordering::Greater => "upcoming",
                    })
                    aria-current=((step == current).then_some("step"))
                >
                    <span class="step-marker">
                        if step < current {
                            icon(data: icons::CHECK, size: 16)
                        } else {
                            (number.to_string())
                        }
                    </span>
                    <span class="step-label">(label)</span>
                </li>
            }
        </ol>
    })
}
