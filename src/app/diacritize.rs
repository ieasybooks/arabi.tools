//! Arabic text and document diacritization.

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
    view::{View, component, view},
};

use crate::tools::diacritization::{
    Diacritization, EXTENSIONS, MAX_TEXT_CHARACTERS, MAX_UPLOAD_BYTES, Model,
};
use crate::{i18n::Locale, icons, meta::PageMeta, queue::Rejection};

pub(super) const SCRIPT: Asset = asset!("./diacritize.js");

#[page]
pub async fn page(cx: &Cx) -> Result<impl View> {
    let locale = Locale::of(cx);
    let text = &locale.text().diacritization;
    let tool = app_context::<Diacritization>(cx);
    Ok(view! {
        tool_hero(locale: locale, lead: true)
        <section class="workspace">
            <div class="container workspace-layout">
                <div class="workspace-main">
                    steps(locale: locale, current: Step::Input, failed: false)
                    if !tool.is_available() {
                        <p class="notice" role="status">(text.unavailable)</p>
                    } else {
                        match tool.queue.availability(client_ip(cx)) {
                            Err(rejection) => {
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
                            }
                            Ok(()) => {
                                editor(
                                    locale: locale,
                                    source: String::new(),
                                    model: Model::Shakkelha,
                                    retry: false
                                )
                            }
                        }
                    }
                </div>
                before_you_start(locale: locale)
            </div>
        </section>
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

/// One model choice serves both modes; a completed text job retains its original input.
#[component]
pub(super) async fn editor(
    locale: Locale,
    source: String,
    model: Model,
    retry: bool,
) -> Result<impl View> {
    let text = &locale.text().diacritization;
    let count = source.chars().count();
    Ok(view! {
        <form
            id="diacritize-form"
            class="diacritize-form"
            method="post"
            action=(href!(page))
            novalidate=(true)
            data-reserve=(href!(jobs::reserve))
            data-retry=(if retry { "true" } else { "false" })
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
            data-no-file=(text.no_file)
        >
            input_choices(locale: locale, model: model)
            <div id="diacritize-text-panel" class="diacritize-panel">
                <div class="diacritize-field-heading">
                    <label for="source-text">(text.source_label)</label>
                    <p id="text-count" class="diacritize-count">
                        (locale.diacritization_count(count))
                    </p>
                </div>
                <textarea
                    id="source-text"
                    class="diacritize-text"
                    name="text"
                    dir="auto"
                    rows="7"
                    placeholder=(text.placeholder)
                    aria-describedby="text-limit text-count"
                >
                    "\n"
                    (source)
                </textarea>
                <p id="text-limit" class="hint">(locale.diacritization_text_limit())</p>
                <div class="form-actions">
                    <button type="submit" class="button primary">
                        (text.submit_text)
                    </button>
                </div>
            </div>
            <div id="diacritize-document-panel" class="diacritize-panel" hidden=(true)>
                <div class="dropzone">
                    <input
                        id="document"
                        type="file"
                        name="file"
                        accept=".txt,.docx,text/plain,application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                        aria-label=(text.file_label)
                    >
                    <span class="dropzone-icon">
                        icon(data: icons::UPLOAD, size: 24)
                    </span>
                    <p class="dropzone-title">(text.choose_file)</p>
                    <p class="dropzone-hint">(locale.diacritization_limits())</p>
                    <p class="file-name" dir="auto">(text.no_file)</p>
                </div>
                <div class="form-actions">
                    <button type="submit" class="button primary">
                        (text.submit_document)
                    </button>
                </div>
            </div>
            <div class="upload-progress" hidden=(true)>
                <progress max="100" value="0" aria-label=(text.uploading)></progress>
                <p class="submission-status" role="status"></p>
                <button type="button" class="button secondary cancel-upload">
                    (text.cancel)
                </button>
            </div>
            <p class="error" role="alert" hidden=(true)></p>
            <a class="return-to-job" hidden=(true)>(text.back_to_job)</a>
        </form>
        <noscript><p class="notice">(text.needs_javascript)</p></noscript>
    })
}

#[component]
async fn input_choices(cx: &Cx, locale: Locale, model: Model) -> Result<impl View> {
    let text = &locale.text().diacritization;
    let tool = app_context::<Diacritization>(cx);
    let shakkelha = tool.model_available(Model::Shakkelha);
    let catt = tool.model_available(Model::Catt);
    Ok(view! {
        <fieldset class="diacritize-models">
            <legend>(text.model)</legend>
            <div class="diacritize-choices">
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
            </div>
            if !shakkelha || !catt {
                <p class="hint">(text.model_unavailable)</p>
            }
        </fieldset>
        <fieldset class="diacritize-modes">
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

#[component]
async fn before_you_start(locale: Locale) -> Result<impl View> {
    let text = &locale.text().diacritization;
    Ok(view! {
        <aside class="workspace-aside">
            <h2 class="aside-title">(text.before_you_start)</h2>
            <ul class="notes" role="list">
                <li>
                    icon(data: icons::INFO, size: 20)
                    <p>(text.replaces_marks)</p>
                </li>
                <li>
                    icon(data: icons::TRANSCRIPT, size: 20)
                    <p>(text.same_format)</p>
                </li>
                <li>
                    icon(data: icons::LOCK, size: 20)
                    <p>(locale.diacritization_privacy())</p>
                </li>
            </ul>
        </aside>
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Step {
    Input,
    Diacritize,
    Download,
}

#[component]
pub(super) async fn steps(locale: Locale, current: Step, failed: bool) -> Result<impl View> {
    let text = &locale.text().diacritization;
    let all = [
        (Step::Input, text.step_input),
        (Step::Diacritize, text.step_diacritize),
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
