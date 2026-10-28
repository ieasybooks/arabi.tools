//! Shared result controls, outside `app/` so they do not add module routes.

use topcoat::{
    Result,
    asset::{Asset, asset},
    icon::icon,
    view::{Child, View, component, view},
};

use crate::i18n::Locale;
use crate::icons;

pub const SEARCH_SCRIPT: Asset = asset!("./app/search.js");
/// Also runs the menus, so every page with a result loads it.
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

/// A small menu, like a file viewer's: a button that opens a list of choices under it. Choosing
/// one, clicking elsewhere or pressing Escape closes it, and focus returns to its button; a choice
/// marked `data-menu-keep` leaves it open, for steps such as zoom. `class` adds the menu's own
/// classes (`menu-start` opens it from its start edge), `button` styles its button, `label` fills
/// the button before its chevron, and the children are the list's items. `COPY_SCRIPT` runs it.
#[component]
pub async fn menu(
    class: &'static str,
    button: &'static str,
    #[into] label: Child<'_>,
    #[default] child: Child<'_>,
) -> Result<impl View> {
    Ok(view! {
        <details class=(format!("menu {class}")) data-menu="">
            <summary class=(button)>(label)</summary>
            <ul class="menu-list" role="list">(child)</ul>
        </details>
    })
}

/// A result's header: its name, then its actions. Beside the workspace's input bar, which already
/// names the file, it has no name, only the actions at its end.
#[component]
pub async fn result_header(
    locale: Locale,
    name: Option<String>,
    copy: Option<CopyAll>,
    downloads: Vec<DownloadLink>,
) -> Result<impl View> {
    Ok(view! {
        <div class="result-heading">
            if let Some(name) = name {
                <p class="result-name">
                    icon(data: icons::DOCUMENT, size: 20)
                    <span dir="auto">(name)</span>
                </p>
            }
            result_actions(
                locale: locale,
                copy: copy,
                downloads: downloads,
                quiet: false
            )
        </div>
    })
}

/// A result's two whole-result actions: Copy all text, a quiet button, when the result has text,
/// then Download. Several downloads open a small menu that lists each by its full label; a single
/// download is a plain link named by its full label. Download is the result's one filled button
/// unless `quiet`, where the page's own action is the only filled one. An OCR result copies all its
/// text or the page in view, so its Copy opens a menu of the two.
#[component]
pub async fn result_actions(
    locale: Locale,
    copy: Option<CopyAll>,
    mut downloads: Vec<DownloadLink>,
    quiet: bool,
) -> Result<impl View> {
    let text = &locale.text().review;
    let ocr = &locale.text().ocr.review;
    let download_class = if quiet {
        "button ghost compact"
    } else {
        "button primary compact"
    };
    let single = (downloads.len() == 1).then(|| downloads.remove(0));
    Ok(view! {
        <div class="result-actions">
            if let Some(copy) = copy {
                match copy {
                    CopyAll::Transcript => {
                        <button
                            type="button"
                            class="button ghost compact"
                            data-copy-all=(true)
                        >
                            icon(data: icons::COPY, size: 16)
                            <span>(text.copy_all)</span>
                        </button>
                    }
                    CopyAll::Ocr => {
                        menu(
                            class: "copy-menu",
                            button: "button ghost compact",
                            label: view! {
                                icon(data: icons::COPY, size: 16)
                                <span>(text.copy)</span>
                            },
                            <li>
                                <button type="button" data-ocr-copy-all="">
                                    (text.copy_all)
                                </button>
                            </li>
                            <li>
                                <button type="button" data-ocr-copy-page="">
                                    (ocr.copy_page)
                                </button>
                            </li>
                        )
                    }
                    CopyAll::Field(field) => {
                        <button
                            type="button"
                            class="button ghost compact"
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
                    class=(download_class)
                    href=(link.href.as_str())
                    download=(true)
                    aria-label=(link.label)
                    title=(link.label)
                >
                    icon(data: icons::DOWNLOAD, size: 16)
                    <span>(text.download)</span>
                </a>
            } else {
                menu(
                    class: "download-menu",
                    button: download_class,
                    label: view! {
                        icon(data: icons::DOWNLOAD, size: 16)
                        <span>(text.download)</span>
                    },
                    for link in &downloads {
                        <li>
                            <a href=(link.href.as_str()) download=(true)>
                                (link.label)
                            </a>
                        </li>
                    }
                )
            }
        </div>
    })
}

/// Load `SEARCH_SCRIPT` from the owning page, including pages with live results. `label` names
/// the field for what it searches, such as the transcript.
#[component]
pub async fn search_toolbar(locale: Locale, label: &'static str) -> Result<impl View> {
    let text = &locale.text().review;
    Ok(view! {
        <div
            class="result-search"
            data-search=(true)
            role="search"
            aria-label=(label)
            data-match-count=(text.match_count)
            data-match-short=(text.match_short)
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
                    aria-label=(label)
                    placeholder=(label)
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
            // Shown only while searching. The active match's cue or page is announced with the
            // count, out of sight: the cue picker or the page navigator already shows it.
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
