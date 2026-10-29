//! Shared browser workspace controls. Tool shards retain their own queue contracts.

use topcoat::{
    Result,
    asset::{Asset, asset},
    context::Cx,
    icon::{IconData, icon},
    router::{
        header::{CACHE_CONTROL, HeaderValue},
        response::response_headers,
    },
    runtime::{Event, Signal},
    view::{View, component, view},
};

use crate::icons;

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

/// A kind of file a tool accepts, which picks the chosen file's icon by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileKind {
    Audio,
    Document,
    Image,
}

impl FileKind {
    fn icon(self) -> IconData {
        match self {
            Self::Audio => icons::AUDIO,
            Self::Document => icons::DOCUMENT,
            Self::Image => icons::IMAGE,
        }
    }

    fn extensions(self) -> &'static str {
        match self {
            Self::Audio => "mp3,m4a",
            Self::Document => "pdf,docx,txt",
            Self::Image => "jpg,jpeg,png",
        }
    }
}

/// The chosen file, which takes the drop zone's place like an attachment: an icon for its
/// kind, its name with its size and type under it, then Remove. The invisible file input still
/// covers the row, so clicking it chooses another file. The script fills it in and shows only
/// the icon for the file's kind; it stays hidden until a file is chosen.
#[component]
pub(crate) async fn file_row(
    locale: crate::i18n::Locale,
    kinds: &'static [FileKind],
) -> Result<impl View> {
    let text = &locale.text().workspace;
    Ok(view! {
        <div class="file-row">
            for kind in kinds {
                <span class="file-icon" data-extensions=(kind.extensions())>
                    icon(data: kind.icon(), size: 20)
                </span>
            }
            <span class="file-details">
                <span class="file-name" dir="auto"></span>
                <span class="file-size" dir="auto"></span>
            </span>
            <button
                type="button"
                class="icon-button remove-file"
                aria-label=(text.remove_file)
                title=(text.remove_file)
            >
                icon(data: icons::CLEAR, size: 16)
            </button>
        </div>
    })
}

/// The page's one muted line, with its lock, on what happens to the visitor's files and results:
/// under the input until there is a result, then at the foot of the result.
#[component]
pub(crate) async fn privacy_note(text: String) -> Result<impl View> {
    Ok(view! {
        <p class="privacy-note">
            icon(data: icons::LOCK, size: 16)
            <span>(text)</span>
        </p>
    })
}

/// Shown under progress: a keep-open warning and the job's own page while it runs,
/// and a jump to its finished result that appears on keyboard focus, like a skip link.
#[component]
pub(crate) async fn progress_links(
    locale: crate::i18n::Locale,
    keep_open: &'static str,
) -> Result<impl View> {
    let text = &locale.text().workspace;
    Ok(view! {
        <p class="keep-open" hidden=(true)>
            icon(data: icons::WARNING, size: 20)
            <span>(keep_open)</span>
        </p>
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
