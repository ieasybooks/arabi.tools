// Renders the site's raster images from src/app/images/logo.svg and the bundled fonts: the
// PNG icons, favicon.ico, and the link-preview cards in Arabic and English. Run it from the
// repository root after `npm ci --prefix tests/browser`: node scripts/render-images.mjs

import { readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const { chromium } = createRequire(new URL("../tests/browser/package.json", import.meta.url))(
  "playwright",
);
const images = new URL("../src/app/images/", import.meta.url);
const fonts = new URL("../src/app/fonts/", import.meta.url);

// Pages rendered from strings cannot load local files, so the logo and fonts go in as data URLs.
const dataUrl = async (url, type) =>
  `data:${type};base64,${(await readFile(url)).toString("base64")}`;
const logo = await dataUrl(new URL("logo.svg", images), "image/svg+xml");
const font = (path) => dataUrl(new URL(path, fonts), "font/woff2");
const gilroyMedium = await font("gilroy/Gilroy-Medium.woff2");
const gilroyBold = await font("gilroy/Gilroy-Bold.woff2");
const naskh = await font("noto-naskh-arabic-ui/NotoNaskhArabicUI.woff2");
const thmanyah = await font("thmanyah/ThmanyahSerifDisplay-Medium.woff2");

// The text of each card, in the site's two languages.
const cards = {
  ar: {
    dir: "rtl",
    title: "أدوات العربية",
    tagline: "أدوات مجانية للغة العربية",
    tool: "تفريغ الصوت إلى نص مكتوب وملف ترجمة SRT",
    initiative: "مشروع من مبادرة الكتب الميسّرة",
  },
  en: {
    dir: "ltr",
    title: "arabi.tools",
    tagline: "Free tools for the Arabic language",
    tool: "Turn audio into text and SRT subtitles",
    initiative: "A project of the iEasyBooks initiative",
  },
};

const ornament =
  "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 144 12'%3E%3Cpath d='M0 6h60M84 6h60' stroke='%23a4824a'/%3E%3Cpath d='M72 1l5 5-5 5-5-5z' fill='%23a4824a'/%3E%3Ccircle cx='63' cy='6' r='1' fill='%23a4824a'/%3E%3Ccircle cx='81' cy='6' r='1' fill='%23a4824a'/%3E%3C/svg%3E";

const card = ({ dir, title, tagline, tool, initiative }, lang) => `<!doctype html>
<html lang="${lang}" dir="${dir}">
<style>
  @font-face { font-family: Gilroy; font-weight: 500; src: url(${gilroyMedium}); }
  @font-face { font-family: Gilroy; font-weight: 700; src: url(${gilroyBold}); }
  @font-face { font-family: Naskh; font-weight: 400 700; src: url(${naskh}); }
  @font-face { font-family: Thmanyah; src: url(${thmanyah}); }
  body { margin: 0; }
  main {
    box-sizing: border-box; width: 1200px; height: 630px; padding: 28px;
    background: #f8f4ea; color: #2b2119; font-family: Gilroy, Naskh, sans-serif;
  }
  .frame {
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 18px;
    height: 100%; border: 1.5px solid #a4824a; border-radius: 28px; text-align: center;
    box-shadow: inset 0 0 0 6px #f8f4ea, inset 0 0 0 7px rgb(164 130 74 / 0.45);
  }
  img { width: 128px; height: 128px; }
  h1 { margin: 6px 0 0; font: 500 104px/1.1 Gilroy, Thmanyah, serif; }
  :lang(en) h1 { font-weight: 700; font-size: 96px; letter-spacing: -0.02em; }
  .ornament { width: 180px; height: 15px; background: url("${ornament}") center / contain no-repeat; }
  .tagline { margin: 0; font-size: 42px; font-weight: 500; color: #5f5245; }
  .tool { margin: 0; font-size: 30px; color: #7d6e5e; }
  .initiative { margin: 10px 0 0; font-size: 24px; color: #a4824a; font-weight: 500; }
</style>
<main><div class="frame">
  <img src="${logo}" alt="">
  <h1>${title}</h1>
  <div class="ornament"></div>
  <p class="tagline">${tagline}</p>
  <p class="tool">${tool}</p>
  <p class="initiative">${initiative}</p>
</div></main>
</html>`;

// An ICO file holding the given PNGs, one directory entry each.
const ico = (pngs) => {
  const header = Buffer.alloc(6 + 16 * pngs.length);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(pngs.length, 4);
  let offset = header.length;
  pngs.forEach(({ size, data }, index) => {
    const entry = 6 + 16 * index;
    header.writeUInt8(size % 256, entry);
    header.writeUInt8(size % 256, entry + 1);
    header.writeUInt16LE(1, entry + 4);
    header.writeUInt16LE(32, entry + 6);
    header.writeUInt32LE(data.length, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    offset += data.length;
  });
  return Buffer.concat([header, ...pngs.map(({ data }) => data)]);
};

const browser = await chromium.launch();
const page = await browser.newPage();

// The logo at `size` pixels; `square` fills the rounded corners, as iOS rounds icons itself.
const icon = async (size, square = false) => {
  await page.setViewportSize({ width: size, height: size });
  await page.setContent(
    `<body style="margin:0"><div style="width:${size}px;height:${size}px;background:${square ? "#1f5c49" : "none"}"><img src="${logo}" width="${size}" height="${size}" style="display:block"></div></body>`,
  );
  await page.locator("img").evaluate((img) => img.decode());
  return page.locator("div").screenshot({ omitBackground: true });
};

await writeFile(new URL("icon-192.png", images), await icon(192));
await writeFile(new URL("icon-512.png", images), await icon(512));
await writeFile(new URL("apple-touch-icon.png", images), await icon(180, true));
const favicons = [];
for (const size of [16, 32, 48]) favicons.push({ size, data: await icon(size) });
await writeFile(new URL("favicon.ico", images), ico(favicons));

await page.setViewportSize({ width: 1200, height: 630 });
for (const [lang, text] of Object.entries(cards)) {
  await page.setContent(card(text, lang));
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: fileURLToPath(new URL(`og-${lang}.png`, images)) });
}
await browser.close();
console.log(`rendered ${(await readFile(new URL("og-ar.png", images))).length} bytes for og-ar.png`);
