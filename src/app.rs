//! The site's pages. Each module under `app/` serves the URL of its module path.

mod health;
mod language;
mod transcribe;

use topcoat::{
    Result,
    asset::{Asset, AssetBundle, RouterBuilderAssetExt, asset},
    context::Cx,
    cookie::RouterBuilderCookieExt,
    font::{Font, UnicodeRange, UnicodeRanges, font},
    icon::icon,
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
use crate::icons;
use crate::tools::transcription::{MAX_UPLOAD_BYTES, Transcription};

const STYLES: Asset = asset!("./app/styles.css");

/// The site's source code.
const SOURCE_URL: &str = "https://github.com/ieasybooks/arlang.tools";

/// The initiative behind the site.
const INITIATIVE_URL: &str = "https://ieasybooks.com";

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

/// Fonts bundled from `app/fonts/`: Gilroy for Latin text, Noto Naskh Arabic UI for
/// Arabic text, Thmanyah Serif Display for Arabic headings and Kitab for transcripts. Each
/// covers only its own script, so the browser takes every character from the font that has it.
/// The characters taken from Gilroy.
const LATIN: UnicodeRanges = UnicodeRanges::new(&[
    UnicodeRange::from_u32(0x0000, 0x024F),
    UnicodeRange::from_u32(0x1E00, 0x1EFF),
    UnicodeRange::from_u32(0x2000, 0x206F),
    UnicodeRange::from_u32(0x20AC, 0x20AC),
    UnicodeRange::from_u32(0x2122, 0x2122),
    UnicodeRange::from_u32(0xFEFF, 0xFEFF),
    UnicodeRange::from_u32(0xFFFD, 0xFFFD),
]);
/// The characters taken from the Arabic fonts.
const ARABIC: UnicodeRanges = UnicodeRanges::new(&[
    UnicodeRange::from_u32(0x0600, 0x06FF),
    UnicodeRange::from_u32(0x0750, 0x077F),
    UnicodeRange::from_u32(0x08A0, 0x08FF),
    UnicodeRange::from_u32(0xFB50, 0xFDFF),
    UnicodeRange::from_u32(0xFE70, 0xFEFF),
]);
const LATIN_FONT: Font = font! {
    "Gilroy",
    @font-face {
        font-weight: 400;
        font-display: swap;
        src: url(asset!("./app/fonts/gilroy/Gilroy-Regular.woff2")) format("woff2");
        unicode-range: LATIN;
    }
    @font-face {
        font-weight: 500;
        font-display: swap;
        src: url(asset!("./app/fonts/gilroy/Gilroy-Medium.woff2")) format("woff2");
        unicode-range: LATIN;
    }
    @font-face {
        font-weight: 700;
        font-display: swap;
        src: url(asset!("./app/fonts/gilroy/Gilroy-Bold.woff2")) format("woff2");
        unicode-range: LATIN;
    }
};
const ARABIC_FONT: Font = font! {
    "Noto Naskh Arabic UI",
    @font-face {
        font-weight: 400 700;
        font-display: swap;
        src: url(asset!("./app/fonts/noto-naskh-arabic-ui/NotoNaskhArabicUI.woff2")
        ) format("woff2");
        unicode-range: ARABIC;
    }
};
const ARABIC_DISPLAY_FONT: Font = font! {
    "Thmanyah Serif Display",
    @font-face {
        font-weight: 500;
        font-display: swap;
        src: url(asset!("./app/fonts/thmanyah/ThmanyahSerifDisplay-Medium.woff2")
        ) format("woff2");
        unicode-range: ARABIC;
    }
};
const TRANSCRIPT_FONT: Font = font! {
    "Kitab",
    @font-face {
        font-weight: 400;
        font-display: swap;
        src: url(asset!("./app/fonts/kitab/Kitab-Regular.woff2")) format("woff2");
        unicode-range: ARABIC;
    }
};

#[layout]
async fn layout(cx: &Cx, slot: Slot<'_>) -> Result<impl View> {
    let locale = Locale::of(cx);
    let text = locale.text();
    Ok(view! {
        <!DOCTYPE html>
        <html lang=(locale.code()) dir=(locale.dir())>
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>(text.site_name)</title>
                topcoat::font::link(font: LATIN_FONT)
                topcoat::font::link(font: ARABIC_FONT)
                topcoat::font::link(font: ARABIC_DISPLAY_FONT)
                topcoat::font::link(font: TRANSCRIPT_FONT)
                <link rel="stylesheet" href=(STYLES)>
                topcoat::dev::script()
                topcoat::runtime::script()
            </head>
            <body>
                <header class="site-header">
                    <div class="container header-inner">
                        <a
                            class="brand"
                            href=(href!(home))
                            aria-label=(text.home_label)
                        >
                            <span class="brand-mark" aria-hidden="true">"ع"</span>
                            <span class="brand-name">(text.site_name)</span>
                        </a>
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
                                        icon(data: icons::TRANSLATE, size: 16)
                                        <span>(other.text().name)</span>
                                    </button>
                                }
                            }
                        </form>
                    </div>
                </header>
                <main class="page">
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
                site_footer(locale: locale)
            </body>
        </html>
    })
}

/// The initiative behind the site, its accounts, and the site's source code.
#[component]
async fn site_footer(locale: Locale) -> Result<impl View> {
    let text = locale.text();
    let social = &text.social;
    let links = [
        ("https://t.me/ieasybooks", icons::TELEGRAM, social.telegram),
        (
            "https://youtube.com/@ieasybooks",
            icons::YOUTUBE,
            social.youtube,
        ),
        ("https://x.com/iieasybooks", icons::X, social.x),
        (
            "https://fb.com/ieasybooks",
            icons::FACEBOOK,
            social.facebook,
        ),
        (
            "https://github.com/ieasybooks",
            icons::GITHUB,
            social.github,
        ),
        ("mailto:easybooksdev@gmail.com", icons::MAIL, social.email),
    ];
    Ok(view! {
        <footer class="site-footer">
            <div class="container footer-inner">
                <p>
                    <span class="brand-name">(text.site_name)</span>
                    (text.initiative_before)
                    <a href=(INITIATIVE_URL)>(text.initiative_name)</a>
                    (text.initiative_after)
                </p>
                <ul class="social" role="list" aria-label=(social.label)>
                    for (href, data, label) in links {
                        <li>
                            <a href=(href) title=(label) aria-label=(label)>
                                icon(data: data, size: 20)
                            </a>
                        </li>
                    }
                </ul>
                <a class="footer-link" href=(SOURCE_URL)>
                    icon(data: icons::CODE, size: 16)
                    <span>(text.source_code)</span>
                </a>
            </div>
        </footer>
    })
}

#[page]
async fn home(cx: &Cx) -> Result<impl View> {
    let text = Locale::of(cx).text();
    Ok(view! {
        <section class="hero">
            <div class="container">
                <h1>(text.tagline)</h1>
                <p class="lead">(text.lead)</p>
                <ul class="highlights" role="list">
                    <li>
                        icon(data: icons::FREE, size: 24)
                        <span>(text.free)</span>
                    </li>
                    <li>
                        icon(data: icons::PRIVATE, size: 24)
                        <span>(text.files_deleted)</span>
                    </li>
                    <li>
                        icon(data: icons::ARABIC, size: 24)
                        <span>(text.arabic_first)</span>
                    </li>
                </ul>
            </div>
        </section>
        <section class="section">
            <div class="container">
                <h2 class="section-title">(text.tools_title)</h2>
                <ul class="tools" role="list">
                    tool_card(
                        href: href!(transcribe::page).resolve(cx),
                        name: text.transcription.name,
                        description: text.transcription.description,
                        action: text.open_tool
                    )
                    <li class="tool-soon"><p>(text.more_tools)</p></li>
                </ul>
            </div>
        </section>
    })
}

#[component]
async fn tool_card(href: String, name: &str, description: &str, action: &str) -> Result<impl View> {
    Ok(view! {
        <li>
            <a class="tool" href=(href)>
                <h3 class="tool-name">(name)</h3>
                <p class="tool-description">(description)</p>
                <p class="tool-formats">
                    <span class="badge">"MP3 · M4A"</span>
                    <span class="forward">icon(data: icons::FORWARD, size: 16)</span>
                    <span class="badge">"TXT · SRT"</span>
                </p>
                <p class="tool-action">
                    <span>(action)</span>
                    <span class="forward">icon(data: icons::FORWARD, size: 16)</span>
                </p>
            </a>
        </li>
    })
}

#[component]
async fn missing_page(locale: Locale) -> Result<impl View> {
    let text = locale.text();
    Ok(view! {
        <section class="hero not-found">
            <div class="container">
                <p class="error-code" dir="ltr">"404"</p>
                <h1>(text.not_found_title)</h1>
                <p class="lead">(text.not_found)</p>
                <p><a class="button primary" href=(href!(home))>(text.back_home)</a></p>
            </div>
        </section>
    })
}
