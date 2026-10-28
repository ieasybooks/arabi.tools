//! Shared result controls, outside `app/` so they do not add module routes.

use topcoat::{
    Result,
    asset::{Asset, asset},
    icon::icon,
    view::{View, component, view},
};

use crate::i18n::Locale;
use crate::icons;

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
            // A find bar: the count, the two steps and Clear sit inside the field, at its end.
            <div class="search-field">
                icon(data: icons::SEARCH, size: 16)
                <input
                    type="search"
                    data-search-query=(true)
                    dir="auto"
                    autocomplete="off"
                    spellcheck="false"
                    aria-label=(text.search)
                    placeholder=(text.search)
                >
                <span class="search-tools">
                    <span class="search-count" data-search-count=(true)></span>
                    <button
                        type="button"
                        class="icon-button"
                        data-search-previous=(true)
                        aria-label=(text.previous_match)
                        title=(text.previous_match)
                        disabled=(true)
                    >
                        icon(data: icons::UP, size: 16)
                    </button>
                    <button
                        type="button"
                        class="icon-button"
                        data-search-next=(true)
                        aria-label=(text.next_match)
                        title=(text.next_match)
                        disabled=(true)
                    >
                        icon(data: icons::DOWN, size: 16)
                    </button>
                    <button
                        type="button"
                        class="icon-button"
                        data-search-clear=(true)
                        aria-label=(text.clear_search)
                        title=(text.clear_search)
                    >
                        icon(data: icons::CLEAR, size: 16)
                    </button>
                </span>
            </div>
            // Shown only while searching, with the active match's cue or page at the end.
            <div class="search-options">
                <label>
                    <input type="checkbox" data-search-marks=(true) checked=(true)>
                    (text.ignore_marks)
                </label>
                <label>
                    <input type="checkbox" data-search-tatweel=(true) checked=(true)>
                    (text.ignore_tatweel)
                </label>
                <bdi class="search-active" data-search-active=(true)></bdi>
            </div>
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
