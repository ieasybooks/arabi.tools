//! The site's pages. Each module under `app/` serves the URL of its module path.

mod health;
mod language;
mod transcribe;

use topcoat::{
    Result,
    asset::{Asset, AssetBundle, RouterBuilderAssetExt, asset},
    context::Cx,
    cookie::RouterBuilderCookieExt,
    font::{Font, font},
    router::{
        BodyLimit, ForwardedHeader, Router, RouterBuilderDiscoverExt, Slot, StatusCode,
        TrustedProxies, error::NotFoundError, href, layout, module_router, not_found, page,
        request::uri,
    },
    runtime::RouterBuilderRuntimeExt,
    view::{View, component, error_boundary, view},
};

use crate::config::Config;
use crate::i18n::Locale;
use crate::tools::transcription::{MAX_UPLOAD_BYTES, Transcription};

const STYLES: Asset = asset!("./app/styles.css");

/// Room for the form fields and boundaries around an uploaded file.
const FORM_OVERHEAD: usize = 64 * 1024;

/// The site's router. `assets` must be bundled from the same build.
pub fn router(
    config: &Config,
    transcription: Transcription,
    assets: AssetBundle,
) -> anyhow::Result<Router> {
    let upload_limit = usize::try_from(MAX_UPLOAD_BYTES)? + FORM_OVERHEAD;
    let mut router = module_router!()
        .discover()
        .assets(assets)
        .cookies()
        .app_context(transcription)
        .layer(BodyLimit::max(upload_limit).at("/transcribe/jobs"));
    if let Some(header) = &config.client_ip_header {
        // The proxy in front of the app is the only way in, so trust its header.
        router = router.trusted_proxies(
            TrustedProxies::new()
                .nearest(1)
                .header(ForwardedHeader::Single(header.clone())),
        );
    }
    Ok(router.runtime().build())
}

not_found!();

/// Inter for Latin text and IBM Plex Sans Arabic for Arabic, bundled from `app/fonts/`. Each
/// file holds only its own script, and the stylesheet lists Inter first, so the browser takes
/// every character from the font that has it.
const LATIN_FONT: Font = font! {
    "Inter",
    @font-face {
        font-weight: 400;
        font-display: swap;
        src: url(asset!("./app/fonts/inter/inter-latin-400-normal.woff2")) format(
            "woff2");
    }
    @font-face {
        font-weight: 500;
        font-display: swap;
        src: url(asset!("./app/fonts/inter/inter-latin-500-normal.woff2")) format(
            "woff2");
    }
    @font-face {
        font-weight: 600;
        font-display: swap;
        src: url(asset!("./app/fonts/inter/inter-latin-600-normal.woff2")) format(
            "woff2");
    }
};
const ARABIC_FONT: Font = font! {
    "IBM Plex Sans Arabic",
    @font-face {
        font-weight: 400;
        font-display: swap;
        src: url(asset!(
                "./app/fonts/ibm-plex-sans-arabic/ibm-plex-sans-arabic-arabic-400-normal.woff2",
            )) format("woff2");
    }
    @font-face {
        font-weight: 500;
        font-display: swap;
        src: url(asset!(
                "./app/fonts/ibm-plex-sans-arabic/ibm-plex-sans-arabic-arabic-500-normal.woff2",
            )) format("woff2");
    }
    @font-face {
        font-weight: 600;
        font-display: swap;
        src: url(asset!(
                "./app/fonts/ibm-plex-sans-arabic/ibm-plex-sans-arabic-arabic-600-normal.woff2",
            )) format("woff2");
    }
};

#[layout]
async fn layout(cx: &Cx, slot: Slot<'_>) -> Result<impl View> {
    let locale = Locale::of(cx);
    Ok(view! {
        <!DOCTYPE html>
        <html lang=(locale.code()) dir=(locale.dir())>
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"arlang.tools"</title>
                topcoat::font::link(font: LATIN_FONT)
                topcoat::font::link(font: ARABIC_FONT)
                <link rel="stylesheet" href=(STYLES)>
                topcoat::dev::script()
                topcoat::runtime::script()
            </head>
            <body>
                <header class="site-header">
                    <div class="container">
                        <a class="brand" href=(href!(home))>"arlang.tools"</a>
                        <form
                            class="language-switch"
                            method="post"
                            action=(href!(language::switch))
                        >
                            <input type="hidden" name="back" value=(uri(cx).path())>
                            for other in Locale::ALL {
                                if other != locale {
                                    <button
                                        type="submit"
                                        name="lang"
                                        value=(other.code())
                                        lang=(other.code())
                                    >
                                        (other.text().name)
                                    </button>
                                }
                            }
                        </form>
                    </div>
                </header>
                <main class="container">
                    error_boundary(
                        fallback: move |error| {
                            if error.downcast_ref::<NotFoundError>().is_none() {
                                return Err(error);
                            }
                            Ok(
                                view! {
                                    (StatusCode::NOT_FOUND)
                                    missing_page(locale: locale)
                                },
                            )
                        },
                        (slot)
                    )
                </main>
            </body>
        </html>
    })
}

#[page]
async fn home(cx: &Cx) -> Result<impl View> {
    let text = Locale::of(cx).text();
    Ok(view! {
        <h1>(text.tagline)</h1>
        <p class="lead">(text.choose_tool)</p>
        <ul class="tools" role="list">
            tool_card(
                href: href!(transcribe::page).resolve(cx),
                name: text.transcription.name,
                description: text.transcription.description
            )
        </ul>
        <p class="hint">(text.more_tools)</p>
    })
}

#[component]
async fn tool_card(href: String, name: &str, description: &str) -> Result<impl View> {
    Ok(view! {
        <li>
            <a class="tool" href=(href)>
                <h2>(name)</h2>
                <p>(description)</p>
            </a>
        </li>
    })
}

#[component]
async fn missing_page(locale: Locale) -> Result<impl View> {
    let text = locale.text();
    Ok(view! {
        <p class="notice">(text.not_found)</p>
        <p><a href=(href!(home))>(text.back_home)</a></p>
    })
}
