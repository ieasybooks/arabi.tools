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
            <button type="button" class="button secondary replace-file">
                (text.replace_file)
            </button>
            <button type="button" class="button secondary remove-file">
                (text.remove_file)
            </button>
        </div>
    })
}

#[component]
pub(crate) async fn review(locale: crate::i18n::Locale) -> Result<impl View> {
    let text = &locale.text().workspace;
    Ok(view! {
        <section class="workspace-review">
            <p class="observation-status notice" role="status" hidden=(true)>
                (text.delayed)
            </p>
            <button
                type="button"
                class="button secondary retry-observation"
                hidden=(true)
            >
                (text.retry_observation)
            </button>
            <a class="workspace-recovery" hidden=(true)>(text.recovery)</a>
            <button type="button" class="button secondary view-result" hidden=(true)>
                (text.view_result)
            </button>
            <p class="previous-result hint" hidden=(true)></p>
            <div class="workspace-result" tabindex="-1" aria-label=(text.result)></div>
        </section>
    })
}
