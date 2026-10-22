//! The transcription tool's upload page.

mod jobs;

use topcoat::{
    Result,
    asset::{Asset, asset},
    context::{Cx, app_context},
    router::{href, page, request::client_ip},
    view::{View, view},
};

use crate::i18n::Locale;
use crate::tools::transcription::{EXTENSIONS, MAX_UPLOAD_BYTES, Transcription};

const UPLOAD_SCRIPT: Asset = asset!("./upload.js");

#[page]
pub async fn page(cx: &Cx) -> Result<impl View> {
    let locale = Locale::of(cx);
    let text = &locale.text().transcription;
    let transcription = app_context::<Transcription>(cx);
    let languages = transcription.languages();
    Ok(view! {
        <h1>(text.name)</h1>
        <p class="lead">(text.description)</p>
        match transcription.queue.availability(client_ip(cx)) {
            Err(rejection) => {
                <p class="notice">(locale.rejection(rejection))</p>
            }
            Ok(()) => {
                <form
                    id="upload-form"
                    class="card upload"
                    data-reserve=(href!(jobs::reserve))
                    data-extensions=(EXTENSIONS.join(","))
                    data-max-bytes=(MAX_UPLOAD_BYTES)
                    data-missing=(text.missing)
                    data-wrong-type=(text.wrong_type)
                    data-too-large=(locale.too_large())
                    data-failed=(text.upload_failed)
                >
                    // The input covers the drop zone, invisible, so clicking and dropping a
                    // file both reach it; the text around it follows the page's language.
                    <div class="dropzone">
                        <input
                            id="audio"
                            type="file"
                            name="file"
                            accept=".mp3,.m4a,audio/mpeg,audio/mp4,audio/x-m4a"
                            aria-label=(text.file_label)
                            required=(true)
                        >
                        <p class="dropzone-title">(text.choose_file)</p>
                        <p class="file-name">(text.no_file)</p>
                    </div>
                    <p class="hint">(locale.upload_limits())</p>
                    if languages.len() > 1 {
                        <label for="language">(text.language_label)</label>
                        <select id="language" name="language">
                            for language in languages {
                                <option value=(language.code.as_str())>
                                    (locale.language_name(&language.code))
                                </option>
                            }
                        </select>
                    }
                    <button type="submit">(text.submit)</button>
                    <div class="upload-progress" hidden=(true)>
                        <progress max="100" value="0"></progress>
                        <span>(text.uploading)</span>
                    </div>
                    <p class="error" role="alert" hidden=(true)></p>
                </form>
                <noscript><p class="notice">(text.needs_javascript)</p></noscript>
                <script src=(UPLOAD_SCRIPT) defer=(true)></script>
            }
        }
        <section class="notes">
            <p>(locale.compression_tip())</p>
            <p>(locale.privacy_note())</p>
        </section>
    })
}
