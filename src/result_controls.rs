//! Shared result controls, outside `app/` so they do not add module routes.

use topcoat::{
    Result,
    asset::{Asset, asset},
    view::{View, component, view},
};

use crate::i18n::Locale;

pub const SEARCH_SCRIPT: Asset = asset!("./app/search.js");
pub const COPY_SCRIPT: Asset = asset!("./app/copy.js");

/// Load `SEARCH_SCRIPT` from the owning page, including pages with live results.
#[component]
pub async fn search_toolbar(locale: Locale) -> Result<impl View> {
    let text = &locale.text().review;
    Ok(view! {
        <div
            class="result-search"
            data-search=(true)
            role="search"
            aria-label=(text.search)
            data-match-count=(text.match_count)
            data-no-matches=(text.no_matches)
            data-searching=(text.searching)
            data-unavailable=(text.search_unavailable)
            hidden=(true)
        >
            <label class="search-field">
                <span>(text.search)</span>
                <input
                    type="search"
                    data-search-query=(true)
                    dir="auto"
                    autocomplete="off"
                    spellcheck="false"
                >
            </label>
            <div class="search-options">
                <label>
                    <input type="checkbox" data-search-marks=(true) checked=(true)>
                    (text.ignore_marks)
                </label>
                <label>
                    <input type="checkbox" data-search-tatweel=(true) checked=(true)>
                    (text.ignore_tatweel)
                </label>
            </div>
            <div class="search-actions">
                <button
                    type="button"
                    class="button secondary"
                    data-search-previous=(true)
                    disabled=(true)
                >
                    (text.previous_match)
                </button>
                <button
                    type="button"
                    class="button secondary"
                    data-search-next=(true)
                    disabled=(true)
                >
                    (text.next_match)
                </button>
                <button type="button" class="button secondary" data-search-clear=(true)>
                    (text.clear_search)
                </button>
            </div>
            <p class="search-position">
                <span data-search-count=(true)></span>
                <bdi data-search-active=(true)></bdi>
            </p>
            <p
                class="visually-hidden"
                data-search-status=(true)
                role="status"
                aria-atomic="true"
            ></p>
        </div>
    })
}

#[component]
pub async fn copy_feedback(locale: Locale) -> Result<impl View> {
    let text = &locale.text().review;
    Ok(view! {
        <p
            class="copy-status"
            data-copy-status=(true)
            data-copied=(text.copied)
            data-copy-failed=(text.copy_failed)
            role="status"
            aria-atomic="true"
        ></p>
        <div
            data-copy-fallback=(true)
            data-copy-label=(text.copy_label)
            data-copy-close=(text.copy_close)
        ></div>
    })
}
