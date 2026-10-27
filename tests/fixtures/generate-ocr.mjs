// Regenerate the original OCR review fixtures using the already installed browser test tools.
import { chromium } from '../browser/node_modules/playwright/index.mjs';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
const font = await readFile(new URL('../../src/app/fonts/kitab/Kitab-Regular.woff2', import.meta.url));
const browser = await chromium.launch();
try {
  const page = await browser.newPage({ viewport: { width: 600, height: 800 } });
  await page.setContent(`<style>
    @font-face { font-family: Kitab; src: url(data:font/woff2;base64,${font.toString('base64')}); }
    @page { size: 600px 800px; margin: 0; }
    * { box-sizing: border-box; } body { margin: 0; }
    section { width:600px; height:800px; break-after:page; padding:50px; text-align:center; font:36px Kitab; }
    .one { background:#eeaaaa; } .three { background:#2222bb; }
    b { display:block; font:200px sans-serif; } p { margin-top:90px; }
  </style><section class="one"><b>1</b><p lang="ar" dir="rtl">الصفحة الأولى — كِتَاب</p><p>First page</p></section><section></section><section class="three"><b>3</b><p lang="ar" dir="rtl">الصفحة الثالثة — مـدرسة</p><p>Third page</p></section>`);
  await page.evaluate(() => document.fonts.ready);
  await page.pdf({ path: fileURLToPath(new URL('ocr-review.pdf', import.meta.url)), printBackground: true, preferCSSPageSize: true });
  await page.screenshot({ path: fileURLToPath(new URL('ocr-review.png', import.meta.url)) });
  await page.screenshot({ path: fileURLToPath(new URL('ocr-review.jpg', import.meta.url)), type: 'jpeg', quality: 85 });
} finally { await browser.close(); }
