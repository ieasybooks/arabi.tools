//! Shared browser workspace controls. Tool shards retain their own queue contracts.

use topcoat::{
    Result,
    asset::{Asset, asset},
    context::Cx,
    icon::icon,
    router::{
        header::{CACHE_CONTROL, HeaderValue},
        response::response_headers,
    },
    runtime::{Event, Signal},
    view::{View, component, view},
};

pub(crate) const SCRIPT: Asset = asset!("./app/workspace.js");

pub(crate) fn private_response(cx: &Cx) {
    response_headers(cx).append(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
}

/// A documented DOM event updates the signal; the bound acknowledgement lets a
/// controller retry an event sent before Topcoat has hydrated the page.
#[component]
pub(crate) async fn bridge(job: Signal<String>) -> Result<impl View> {
    Ok(view! {
        <input
            type="hidden"
            class="workspace-bridge"
            :data-ack=$(job.get())
            @input=$(|event: Event| job.set(event.target.value))
        >
    })
}

/// The chosen file, which takes the drop zone's place: its name, size and type, then Replace
/// and Remove. The script fills it in; it stays hidden until a file is chosen.
#[component]
pub(crate) async fn file_row(locale: crate::i18n::Locale) -> Result<impl View> {
    let text = &locale.text().workspace;
    Ok(view! {
        <div class="file-row">
            <span class="file-icon">
                icon(data: crate::icons::TRANSCRIPT, size: 20)
            </span>
            <span class="file-details">
                <span class="file-name" dir="auto"></span>
                <span class="file-size" dir="auto"></span>
            </span>
            <button type="button" class="button ghost compact replace-file">
                (text.replace_file)
            </button>
            <button
                type="button"
                class="icon-button remove-file"
                aria-label=(text.remove_file)
                title=(text.remove_file)
            >
                icon(data: crate::icons::CLEAR, size: 16)
            </button>
        </div>
    })
}

/// One muted line beside a card: a privacy note, with its lock, or advice about the result.
#[component]
pub(crate) async fn note(text: String, privacy: bool) -> Result<impl View> {
    Ok(view! {
        <p
            class=(if privacy {
                "workspace-note privacy-note"
            } else {
                "workspace-note result-note"
            })
        >
            if privacy {
                icon(data: crate::icons::LOCK, size: 16)
            }
            <span>(text)</span>
        </p>
    })
}

/// Shown under a job's progress: the job's own page while it runs, and a jump to its finished
/// result that appears on keyboard focus, like a skip link.
#[component]
pub(crate) async fn progress_links(locale: crate::i18n::Locale) -> Result<impl View> {
    let text = &locale.text().workspace;
    Ok(view! {
        <p class="progress-links">
            <a class="view-result" href="#workspace-result" hidden=(true)>
                (text.view_result)
            </a>
            <a class="workspace-recovery" hidden=(true)>(text.recovery)</a>
        </p>
    })
}

/// `placeholder` fills the result card before the first result arrives.
#[component]
pub(crate) async fn review(
    locale: crate::i18n::Locale,
    placeholder: Option<&'static str>,
) -> Result<impl View> {
    let text = &locale.text().workspace;
    Ok(view! {
        <section class="workspace-review">
            <div class="observation-notice">
                <p class="observation-status" role="status" hidden=(true)>
                    (text.delayed)
                </p>
                <button
                    type="button"
                    class="button secondary compact retry-observation"
                    hidden=(true)
                >
                    (text.retry_observation)
                </button>
            </div>
            <p class="previous-result" hidden=(true)></p>
            <div
                id="workspace-result"
                class="workspace-result"
                tabindex="-1"
                aria-label=(text.result)
            ></div>
            if let Some(placeholder) = placeholder {
                <p class="result-placeholder">(placeholder)</p>
            }
        </section>
    })
}
