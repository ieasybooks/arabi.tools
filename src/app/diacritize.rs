//! Arabic text and document diacritization.

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

use crate::result_controls::COPY_SCRIPT;
use crate::tools::diacritization::{
    Diacritization, EXTENSIONS, MAX_TEXT_CHARACTERS, MAX_UPLOAD_BYTES, Model,
};
use crate::workspace::FileKind;
use crate::{i18n::Locale, icons, meta::PageMeta, queue::Rejection};

pub(super) const SCRIPT: Asset = asset!("./diacritize.js");

/// The kinds of document this tool accepts, for the chosen file's icon.
const KINDS: &[FileKind] = &[FileKind::Document];

#[page]
pub async fn page(cx: &Cx) -> Result<impl View> {
    let locale = Locale::of(cx);
    let text = &locale.text().diacritization;
    let tool = app_context::<Diacritization>(cx);
    Ok(view! {
        tool_hero(locale: locale, lead: true)
        <section class="workspace">
            <div class="container workspace-layout">
                <div class="workspace-entry">
                    if !tool.is_available() {
                        <div class="workspace-card">
                            <p class="notice" role="status">(text.unavailable)</p>
                        </div>
                    } else {
                        match tool.queue.availability(client_ip(cx)) {
                            Err(rejection) => {
                                <div class="workspace-card">
                                    <div class="notice">
                                        icon(data: icons::WARNING, size: 20)
                                        <div class="notice-body">
                                            <p>(locale.diacritization_rejection(rejection))</p>
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
                                editor(locale: locale, initial: None)
                            }
                        }
                    }
                </div>
            </div>
        </section>
        <script src=(COPY_SCRIPT) defer=(true)></script>
        <script src=(crate::workspace::SCRIPT) defer=(true)></script>
        <script src=(SCRIPT) defer=(true)></script>
    })
}

/// The public tool is indexed; every job route is private.
pub(crate) fn meta(cx: &Cx, locale: Locale, origin: &str) -> Option<PageMeta> {
    let text = locale.text();
    let page_path = href!(page).resolve(cx);
    let path = uri(cx).path();
    let title = locale.page_title(text.diacritization.name);
    let description = text.diacritization.description.to_owned();
    if path == page_path {
        Some(PageMeta {
            title,
            description: description.clone(),
            index: true,
            structured_data: Some(json!({
                "@context": "https://schema.org",
                "@type": "WebApplication",
                "name": text.diacritization.name,
                "url": format!("{origin}{page_path}"),
                "description": description,
                "inLanguage": locale.code(),
                "applicationCategory": "UtilitiesApplication",
                "operatingSystem": "Any",
                "browserRequirements": "Requires JavaScript",
                "isAccessibleForFree": true,
                "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"},
                "isPartOf": {"@type": "WebSite", "name": text.site_name, "url": format!("{origin}/")},
            })),
        })
    } else if path.starts_with(&format!("{page_path}/")) {
        Some(PageMeta::private(title, description))
    } else {
        None
    }
}

/// One container holds the input and the output side by side, as in a translation tool. One model
/// choice serves both modes; a completed text job retains its original input.
#[component]
pub(super) async fn editor(
    cx: &Cx,
    locale: Locale,
    initial: Option<crate::queue::JobId>,
) -> Result<impl View> {
    let tool = app_context::<Diacritization>(cx);
    let document = initial.as_ref().and_then(|id| tool.queue.result(id));
    let source = document
        .as_ref()
        .and_then(|document| document.text.as_ref())
        .map_or_else(String::new, |text| text.source.clone());
    let model = document
        .as_ref()
        .map_or(Model::Shakkelha, |document| document.model);
    let document_mode = document
        .as_ref()
        .is_some_and(|document| document.text.is_none());
    let initial_job = initial
        .as_ref()
        .map_or_else(String::new, ToString::to_string);
    let initial_url = initial
        .as_ref()
        .map(|id| href!(jobs::job::page, jobs::job::Id(id.clone())).resolve(cx));
    let cancel_url = initial
        .as_ref()
        .filter(|id| jobs::job::owns_job(cx, id))
        .map(|id| href!(jobs::job::cancel, jobs::job::Id(id.clone())).resolve(cx));
    let job = signal(cx, || initial_job.clone());
    let restore_source = initial.is_some() && document.is_none();
    drop(document);
    Ok(view! {
        <div
            data-workspace="diacritization"
            data-initial-job=(initial_job)
            data-initial-url=(initial_url)
            data-cancel-url=(cancel_url)
            data-restore-source=(if restore_source { "true" } else { "false" })
            data-previous=(locale.text().workspace.previous_result)
            data-previous-model=(locale.text().workspace.previous_model)
        >
            <div class="diacritize-box">
                input_form(
                    locale: locale,
                    source: source,
                    model: model,
                    retry: initial.is_some(),
                    document_mode: document_mode,
                    source_needed: restore_source || document_mode
                )
                crate::workspace::review(
                    locale: locale,
                    placeholder: Some(locale.text().diacritization.result_placeholder)
                )
            </div>
            crate::workspace::privacy_note(text: locale.diacritization_privacy())
            crate::workspace::bridge(job: job.clone())
            <div class="workspace-observer" hidden=(initial.is_none())>
                jobs::job::observe(job: $(job.get()))
            </div>
        </div>
        <noscript>
            <p class="notice">(locale.text().diacritization.needs_javascript)</p>
        </noscript>
    })
}

#[component]
async fn input_form(
    locale: Locale,
    source: String,
    model: Model,
    retry: bool,
    document_mode: bool,
    source_needed: bool,
) -> Result<impl View> {
    let text = &locale.text().diacritization;
    Ok(view! {
        <form
            id="diacritize-form"
            class="diacritize-form"
            method="post"
            action=(href!(page))
            novalidate=(true)
            data-reserve=(href!(jobs::reserve))
            data-retry=(if retry { "true" } else { "false" })
            data-initial-mode=(if document_mode { "document" } else { "text" })
            data-reserving=(locale.text().workspace.reserving)
            data-validating=(locale.text().workspace.validating)
            data-observing=(locale.text().workspace.observing)
            data-bytes=(locale.text().workspace.bytes)
            data-kilobytes=(locale.text().workspace.kilobytes)
            data-megabytes=(locale.text().workspace.megabytes)
            data-extensions=(EXTENSIONS.join(","))
            data-max-bytes=(MAX_UPLOAD_BYTES)
            data-max-characters=(MAX_TEXT_CHARACTERS)
            data-count=(text.character_count)
            data-text-limit=(locale.diacritization_text_limit())
            data-text-missing=(text.text_missing)
            data-missing=(text.missing)
            data-wrong-type=(text.wrong_type)
            data-too-large=(locale.too_large())
            data-failed=(text.upload_failed)
            data-model-unavailable=(text.model_unavailable)
            data-uploading=(text.uploading)
            data-submitting=(text.submitting)
            data-cancelling=(text.cancelling)
            data-cancelled=(locale.text().cancelled_user)
            data-cancel-failed=(text.cancel_failed)
        >
            mode_tabs(locale: locale)
            text_input(locale: locale, source: source)
            document_input(locale: locale)
            <p id="diacritize-error" class="error" role="alert" hidden=(true)></p>
            <a class="return-to-job" hidden=(true)>(text.back_to_job)</a>
            if source_needed {
                <p class="hint source-needed">
                    (locale.text().workspace.source_needed)
                </p>
            }
            // The panel's foot: the model, then the run button for the chosen mode.
            <div class="diacritize-foot">
                model_choice(locale: locale, model: model)
                <div class="form-actions">
                    <button
                        type="submit"
                        class="button primary"
                        data-mode="text"
                        disabled=(true)
                    >
                        (text.submit_text)
                    </button>
                    <button
                        type="submit"
                        class="button primary"
                        data-mode="document"
                        disabled=(true)
                        hidden=(true)
                    >
                        (text.submit_document)
                    </button>
                </div>
            </div>
            <div class="upload-progress" hidden=(true)>
                <progress max="100" value="0" aria-label=(text.uploading)></progress>
                <p id="job-status" class="submission-status" role="status"></p>
                <p class="submission-percent" dir="ltr"></p>
                <button type="button" class="button secondary compact cancel-upload">
                    (text.cancel)
                </button>
                crate::workspace::progress_links(locale: locale)
            </div>
        </form>
    })
}

/// The text: a borderless field under the tabs, its character count small at its foot.
#[component]
async fn text_input(locale: Locale, source: String) -> Result<impl View> {
    let text = &locale.text().diacritization;
    let count = source.chars().count();
    Ok(view! {
        <div id="diacritize-text-panel" class="diacritize-panel">
            <label class="visually-hidden" for="source-text">(text.source_label)</label>
            <textarea
                id="source-text"
                class="diacritize-text"
                name="text"
                dir="auto"
                rows="7"
                placeholder=(text.placeholder)
                aria-describedby="text-limit text-count diacritize-error"
            >
                "\n"
                (source)
            </textarea>
            <p
                id="text-count"
                class="diacritize-count"
                aria-label=(locale.diacritization_count(count))
            >
                <bdi dir="ltr">(format!("{count} / {MAX_TEXT_CHARACTERS}"))</bdi>
                (text.character_count.split("{max}").last().unwrap_or_default())
            </p>
            <p id="text-limit" class="visually-hidden">
                (locale.diacritization_text_limit())
            </p>
        </div>
    })
}

#[component]
async fn document_input(locale: Locale) -> Result<impl View> {
    let text = &locale.text().diacritization;
    Ok(view! {
        <div id="diacritize-document-panel" class="diacritize-panel" hidden=(true)>
            <div class="dropzone">
                <input
                    id="document"
                    type="file"
                    name="file"
                    accept=".txt,.docx,text/plain,application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                    aria-label=(text.file_label)
                    aria-describedby="diacritize-error"
                >
                <span class="dropzone-icon">icon(data: icons::UPLOAD, size: 24)</span>
                <p class="dropzone-title">(text.choose_file)</p>
                <p class="dropzone-hint">(locale.diacritization_limits())</p>
                crate::workspace::file_row(locale: locale, kinds: KINDS)
            </div>
        </div>
    })
}

/// What to diacritize: plain underline tabs across the top of the input panel.
#[component]
async fn mode_tabs(locale: Locale) -> Result<impl View> {
    let text = &locale.text().diacritization;
    Ok(view! {
        <fieldset class="diacritize-top diacritize-modes">
            <legend class="visually-hidden">(text.input_mode)</legend>
            <div class="diacritize-choices">
                <label>
                    <input
                        type="radio"
                        name="input_mode"
                        value="text"
                        checked=(true)
                        aria-controls="diacritize-text-panel"
                    >
                    <span>(text.text_mode)</span>
                </label>
                <label>
                    <input
                        type="radio"
                        name="input_mode"
                        value="document"
                        aria-controls="diacritize-document-panel"
                    >
                    <span>(text.document_mode)</span>
                </label>
            </div>
        </fieldset>
    })
}

/// The model: a quiet group of native radios after a small label, under a note when one of the two
/// is unavailable.
#[component]
async fn model_choice(cx: &Cx, locale: Locale, model: Model) -> Result<impl View> {
    let text = &locale.text().diacritization;
    let tool = app_context::<Diacritization>(cx);
    let shakkelha = tool.model_available(Model::Shakkelha);
    let catt = tool.model_available(Model::Catt);
    Ok(view! {
        if !shakkelha || !catt {
            <p class="hint model-unavailable">(text.model_unavailable)</p>
        }
        <fieldset class="diacritize-models">
            <legend class="visually-hidden">(text.model)</legend>
            // The legend names the group; this shows the same word in the row in every engine.
            <span class="diacritize-models-label" aria-hidden="true">(text.model)</span>
            <label>
                <input
                    type="radio"
                    name="model"
                    value="shakkelha"
                    checked=(model == Model::Shakkelha)
                    disabled=(!shakkelha)
                >
                <bdi lang="ar" dir="rtl">(text.shakkelha)</bdi>
            </label>
            <label>
                <input
                    type="radio"
                    name="model"
                    value="catt"
                    checked=(model == Model::Catt)
                    disabled=(!catt)
                >
                <bdi lang="en" dir="ltr">(text.catt)</bdi>
            </label>
        </fieldset>
    })
}

#[component]
pub(super) async fn tool_hero(locale: Locale, lead: bool) -> Result<impl View> {
    let text = &locale.text().diacritization;
    Ok(view! {
        <section class="tool-hero">
            <div class="container">
                <p class="crumbs"><a href=(href!(super::home))>(text.all_tools)</a></p>
                <h1>(text.name)</h1>
                if lead {
                    <p class="lead">(text.description)</p>
                }
            </div>
        </section>
    })
}
