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
/// Also closes the download menus, so every page with a result loads it.
pub const COPY_SCRIPT: Asset = asset!("./app/copy.js");

/// One of a result's downloads: its full localized label, also its accessible name, and its URL.
pub struct DownloadLink {
    pub label: &'static str,
    pub href: String,
}

/// What a result's Copy all button copies. Each result's own script handles the click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyAll {
    /// The whole transcript, from `transcript.js`.
    Transcript,
    /// Every recognized page, from `ocr-review.js`.
    Ocr,
    /// The read-only field with this id, from `diacritize.js`.
    Field(&'static str),
}

/// A result's header on the transcript and OCR pages: its name, then its actions.
#[component]
pub async fn result_header(
    locale: Locale,
    name: String,
    copy: Option<CopyAll>,
    downloads: Vec<DownloadLink>,
) -> Result<impl View> {
    Ok(view! {
        <div class="result-heading">
            <p class="result-name">
                icon(data: icons::DOCUMENT, size: 20)
                <span dir="auto">(name)</span>
            </p>
            result_actions(locale: locale, copy: copy, downloads: downloads)
        </div>
    })
}

/// A result's two whole-result actions: Copy all text, when the result has text, then Download.
/// Several downloads open a small menu that lists each by its full label; a single download is a
/// plain link named by its full label.
#[component]
pub async fn result_actions(
    locale: Locale,
    copy: Option<CopyAll>,
    mut downloads: Vec<DownloadLink>,
) -> Result<impl View> {
    let text = &locale.text().review;
    let single = (downloads.len() == 1).then(|| downloads.remove(0));
    Ok(view! {
        <div class="result-actions">
            if let Some(copy) = copy {
                match copy {
                    CopyAll::Transcript => {
                        <button
                            type="button"
                            class="button secondary compact"
                            data-copy-all=(true)
                        >
                            icon(data: icons::COPY, size: 16)
                            <span>(text.copy_all)</span>
                        </button>
                    }
                    CopyAll::Ocr => {
                        <button
                            type="button"
                            class="button secondary compact"
                            data-ocr-copy-all=""
                        >
                            icon(data: icons::COPY, size: 16)
                            <span>(text.copy_all)</span>
                        </button>
                    }
                    CopyAll::Field(field) => {
                        <button
                            type="button"
                            class="button secondary compact"
                            data-copy=(field)
                        >
                            icon(data: icons::COPY, size: 16)
                            <span>(text.copy_all)</span>
                        </button>
                    }
                }
            }
            if let Some(link) = single {
                <a
                    class="button primary compact"
                    href=(link.href.as_str())
                    download=(true)
                    aria-label=(link.label)
                    title=(link.label)
                >
                    icon(data: icons::DOWNLOAD, size: 16)
                    <span>(text.download)</span>
                </a>
            } else {
                <details class="download-menu" data-download-menu="">
                    <summary class="button primary compact">
                        icon(data: icons::DOWNLOAD, size: 16)
                        <span>(text.download)</span>
                        <span class="download-chevron">
                            icon(data: icons::DOWN, size: 16)
                        </span>
                    </summary>
                    <ul class="download-list" role="list">
                        for link in &downloads {
                            <li>
                                <a href=(link.href.as_str()) download=(true)>
                                    (link.label)
                                </a>
                            </li>
                        }
                    </ul>
                </details>
            }
        </div>
    })
}

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

/// Says whether a copy worked. Results whose script offers its own fallback need only this.
#[component]
pub async fn copy_status(locale: Locale) -> Result<impl View> {
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
    })
}

/// The copy status, then the place for the selectable panel offered when copying fails.
#[component]
pub async fn copy_feedback(locale: Locale) -> Result<impl View> {
    let text = &locale.text().review;
    Ok(view! {
        copy_status(locale: locale)
        <div
            data-copy-fallback=(true)
            data-copy-label=(text.copy_label)
            data-copy-close=(text.copy_close)
        ></div>
    })
}
