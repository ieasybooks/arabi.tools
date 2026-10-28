//! Shared browser workspace controls. Tool shards retain their own queue contracts.

use topcoat::{
    Result,
    asset::{Asset, asset},
    context::Cx,
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

#[component]
pub(crate) async fn file_actions(locale: crate::i18n::Locale) -> Result<impl View> {
    let text = &locale.text().workspace;
    Ok(view! {
        <div class="file-actions">
            <p class="file-size hint" dir="auto"></p>
            <button type="button" class="button secondary compact replace-file">
                (text.replace_file)
            </button>
            <button type="button" class="button ghost compact remove-file">
                (text.remove_file)
            </button>
        </div>
    })
}

/// Shown beside a job's progress: the job's own page, and a jump to its finished result.
#[component]
pub(crate) async fn progress_links(locale: crate::i18n::Locale) -> Result<impl View> {
    let text = &locale.text().workspace;
    Ok(view! {
        <p class="progress-links">
            <button type="button" class="link-button view-result" hidden=(true)>
                (text.view_result)
            </button>
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
            <div class="workspace-result" tabindex="-1" aria-label=(text.result)></div>
            if let Some(placeholder) = placeholder {
                <p class="result-placeholder">(placeholder)</p>
            }
        </section>
    })
}
