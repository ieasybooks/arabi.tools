//! The site's pages. Each module under `app/` serves the URL of its module path.

mod health;
mod language;
mod transcribe;

use std::fmt::Write;

use serde_json::{Value, json};
use topcoat::{
    Result,
    asset::{Asset, AssetBundle, AssetConfig, RouterBuilderAssetExt, asset},
    context::{Cx, app_context},
    cookie::RouterBuilderCookieExt,
    font::{Font, UnicodeRange, UnicodeRanges, font},
    icon::icon,
    router::{
        Body, BodyLimit, ForwardedHeader, Router, RouterBuilderDiscoverExt, Slot, StatusCode,
        TrustedProxies,
        content::Json,
        error::NotFoundError,
        header::{CACHE_CONTROL, CONTENT_TYPE},
        href, layout, module_router, not_found, page,
        request::uri,
        response::{IntoResponse, Response},
        route,
    },
    runtime::RouterBuilderRuntimeExt,
    view::{Unescaped, View, component, error_boundary, view},
};

use crate::config::Config;
use crate::i18n::Locale;
use crate::icons;
use crate::meta::{PageMeta, origin};
use crate::tools::transcription::{MAX_UPLOAD_BYTES, Transcription};

const STYLES: Asset = asset!("./app/styles.css");

// The logo, and the images made from it by `scripts/render-images.mjs`.
const LOGO: Asset = asset!("./app/images/logo.svg");
const APPLE_TOUCH_ICON: Asset = asset!("./app/images/apple-touch-icon.png");
const ICON_192: Asset = asset!("./app/images/icon-192.png");
const ICON_512: Asset = asset!("./app/images/icon-512.png");
const FAVICON: &[u8] = include_bytes!("app/images/favicon.ico");
/// Link-preview cards, 1200 by 630 pixels, in each language.
const PREVIEW_AR: Asset = asset!("./app/images/og-ar.png");
const PREVIEW_EN: Asset = asset!("./app/images/og-en.png");

/// The page background, also the browser's toolbar color.
const THEME_COLOR: &str = "#f8f4ea";

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
                head_meta(locale: locale)
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
                            <img
                                class="brand-logo"
                                src=(LOGO)
                                alt=""
                                width="40"
                                height="40"
                            >
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

/// The page's title, description and indexing, link previews, icons and structured data.
#[component]
async fn head_meta(cx: &Cx, locale: Locale) -> Result<impl View> {
    let text = locale.text();
    let origin = origin(cx);
    let meta = page_meta(cx, locale, &origin);
    let url = format!("{origin}{}", uri(cx).path());
    let preview = match locale {
        Locale::Ar => PREVIEW_AR,
        Locale::En => PREVIEW_EN,
    };
    let preview = format!(
        "{origin}{}",
        app_context::<AssetConfig>(cx).resolve(preview)
    );
    // `<` is escaped so the data cannot end the script early.
    let structured_data = meta
        .structured_data
        .map(|data| Unescaped::new_unchecked(data.to_string().replace('<', "\\u003c")));
    Ok(view! {
        <title>(meta.title.as_str())</title>
        <meta name="description" content=(meta.description.as_str())>
        if meta.index {
            <link rel="canonical" href=(url.as_str())>
        } else {
            <meta name="robots" content="noindex">
        }
        <meta name="theme-color" content=(THEME_COLOR)>
        <link rel="icon" href=(href!(favicon)) sizes="48x48">
        <link rel="icon" href=(LOGO) type="image/svg+xml">
        <link rel="apple-touch-icon" href=(APPLE_TOUCH_ICON)>
        <link rel="manifest" href=(href!(manifest))>
        <meta property="og:type" content="website">
        <meta property="og:site_name" content=(text.site_name)>
        <meta property="og:title" content=(meta.title.as_str())>
        <meta property="og:description" content=(meta.description.as_str())>
        <meta property="og:url" content=(url.as_str())>
        <meta property="og:image" content=(preview.as_str())>
        <meta property="og:image:width" content="1200">
        <meta property="og:image:height" content="630">
        <meta property="og:image:alt" content=(text.preview_alt)>
        <meta property="og:locale" content=(locale.og_locale())>
        for other in Locale::ALL {
            if other != locale {
                <meta property="og:locale:alternate" content=(other.og_locale())>
            }
        }
        <meta name="twitter:card" content="summary_large_image">
        if let Some(structured_data) = structured_data {
            <script type="application/ld+json">(structured_data)</script>
        }
    })
}

/// What the requested page tells search engines: the home page, a tool's page, or a page that
/// does not exist, which stays out of search results.
fn page_meta(cx: &Cx, locale: Locale, origin: &str) -> PageMeta {
    let text = locale.text();
    if uri(cx).path() == href!(home).resolve(cx) {
        return PageMeta {
            title: format!("{} — {}", text.site_name, text.tagline),
            description: text.description.to_owned(),
            index: true,
            structured_data: Some(site_data(locale, origin)),
        };
    }
    transcribe::meta(cx, locale, origin).unwrap_or_else(|| {
        PageMeta::private(
            locale.page_title(text.not_found_title),
            text.description.to_owned(),
        )
    })
}

/// The site and the initiative behind it, as schema.org data.
fn site_data(locale: Locale, origin: &str) -> Value {
    let text = locale.text();
    json!({
        "@context": "https://schema.org",
        "@type": "WebSite",
        "name": text.site_name,
        "alternateName": Locale::ALL
            .iter()
            .filter(|other| **other != locale)
            .map(|other| other.text().site_name)
            .collect::<Vec<_>>(),
        "url": format!("{origin}/"),
        "description": text.description,
        "inLanguage": locale.code(),
        "publisher": {
            "@type": "Organization",
            "name": text.initiative_name,
            "url": INITIATIVE_URL,
        },
    })
}

/// Lets crawlers read the whole site and points them at the sitemap.
#[route(GET "./robots.txt")]
async fn robots(cx: &Cx) -> Result<String> {
    Ok(format!(
        "User-agent: *\nAllow: /\n\nSitemap: {}{}\n",
        origin(cx),
        href!(sitemap).resolve(cx)
    ))
}

/// The pages search engines should know about: the home page and each tool's page.
#[route(GET "./sitemap.xml")]
async fn sitemap(cx: &Cx) -> Result<Typed> {
    let origin = origin(cx);
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for path in [href!(home).resolve(cx), href!(transcribe::page).resolve(cx)] {
        let _ = writeln!(xml, "  <url><loc>{origin}{path}</loc></url>");
    }
    xml.push_str("</urlset>\n");
    Ok(Typed::new("application/xml; charset=utf-8", xml))
}

/// The icon browsers ask for at the site's root.
#[route(GET "./favicon.ico")]
async fn favicon() -> Result<Typed> {
    Ok(Typed::new("image/x-icon", FAVICON))
}

/// The site's name, colors and icons, for adding it to a phone's home screen.
#[route(GET "./site.webmanifest")]
async fn manifest(cx: &Cx) -> Result<Json<Value>> {
    let locale = Locale::of(cx);
    let text = locale.text();
    let assets = app_context::<AssetConfig>(cx);
    Ok(Json(json!({
        "name": text.site_name,
        "short_name": text.site_name,
        "description": text.description,
        "lang": locale.code(),
        "dir": locale.dir(),
        "start_url": href!(home).resolve(cx),
        "display": "standalone",
        "background_color": THEME_COLOR,
        "theme_color": THEME_COLOR,
        "icons": [
            {"src": assets.resolve(ICON_192), "sizes": "192x192", "type": "image/png"},
            {"src": assets.resolve(ICON_512), "sizes": "512x512", "type": "image/png"},
        ],
    })))
}

/// A response with its own `Content-Type`, cached for a day.
struct Typed {
    content_type: &'static str,
    body: Body,
}

impl Typed {
    fn new(content_type: &'static str, body: impl Into<Body>) -> Self {
        Self {
            content_type,
            body: body.into(),
        }
    }
}

impl IntoResponse for Typed {
    fn into_response(self, _cx: &Cx) -> Result<Response> {
        Ok(Response::builder()
            .header(CONTENT_TYPE, self.content_type)
            .header(CACHE_CONTROL, "public, max-age=86400")
            .body(self.body)?)
    }
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
