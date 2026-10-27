import { readFile, mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { expect, test } from '@playwright/test';

// A focused WebKit run uses the same suite and servers without changing the default project.
test.use({ browserName: process.env.OCR_REVIEW_BROWSER || process.env.BROWSER || 'chromium', userAgent: undefined });
const fixture = (name) => fileURLToPath(new URL(`../fixtures/${name}`, import.meta.url));
const original = () => readFile(fixture('ocr-review.pdf'));
const first = 'الصفحة الأولى — كِتَاب 1\nFirst page: café 😀 <b> & نص مشترك';
const third = 'الصفحة الثالثة — مـدرسة 3\nThird page: آخر نص مشترك';
const texts = [first, '', third];
const resultRoot = (page) => page.locator('[data-result="ocr"]');

test.beforeEach(async ({ page }) => {
  page.on('pageerror', (error) => { throw error; });
});

function pdf(count, { largeLast = false } = {}) {
  const objects = ['<< /Type /Catalog /Pages 2 0 R >>', `<< /Type /Pages /Kids [${Array.from({ length: count }, (_, i) => `${i + 3} 0 R`).join(' ')}] /Count ${count} >>`];
  for (let i = 0; i < count; i++) {
    const side = largeLast && i === count - 1 ? 14400 : 600;
    objects.push(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${side} ${side}] /Resources << >> >>`);
  }
  let output = '%PDF-1.4\n';
  const offsets = [];
  objects.forEach((object, i) => { offsets.push(output.length); output += `${i + 1} 0 obj\n${object}\nendobj\n`; });
  const xref = output.length;
  output += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  offsets.forEach((offset) => { output += `${String(offset).padStart(10, '0')} 00000 n \n`; });
  return Buffer.from(`${output}trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`);
}

async function openResult(page, { locale = 'en', source = false, bytes, name = 'review.pdf', type = 'application/pdf' } = {}) {
  bytes ??= await original();
  await page.context().addCookies([{ name: 'lang', value: locale, url: 'http://127.0.0.1:3204' }]);
  const reservation = await page.request.post('/ocr/jobs');
  expect(reservation.ok()).toBeTruthy();
  const reserved = await reservation.json();
  const uploaded = await page.request.post(reserved.upload, { multipart: { file: { name, mimeType: type, buffer: bytes } } });
  expect(uploaded.ok()).toBeTruthy();
  const { next } = await uploaded.json();
  const jobId = next.split('/').pop();
  if (source) {
    await page.addInitScript(({ jobId, bytes, name, type }) => {
      window.arabiTools ??= {};
      window.arabiTools.sources = new Map([[jobId, new File([new Uint8Array(bytes)], name, { type })]]);
    }, { jobId, bytes: Array.from(bytes), name, type });
  }
  await page.goto(next);
  await expect(resultRoot(page)).toHaveAttribute('data-enhanced', 'true');
  return { jobId, next, bytes };
}

async function restore(page, bytes, name = 'review.pdf', mimeType = 'application/pdf') {
  const root = resultRoot(page);
  await root.locator('[data-ocr-source-file]').setInputFiles({ name, mimeType, buffer: bytes });
  await root.locator('[data-ocr-confirm]').check();
  await root.locator('[data-ocr-source-picker] button').click();
}

async function rendered(page, number) {
  await expect(resultRoot(page).locator('[data-ocr-source-viewport]')).toHaveAttribute('data-rendered-page', String(number));
  await expect(resultRoot(page)).toHaveAttribute('data-current-page', String(number));
}

async function colour(page) {
  return resultRoot(page).locator('canvas').evaluate((canvas) => {
    const [r, g, b] = canvas.getContext('2d').getImageData(Math.floor(canvas.width / 10), Math.floor(canvas.height / 2), 1, 1).data;
    return r - b > 60 ? 'red' : b - r > 60 ? 'blue' : r > 240 && g > 240 && b > 240 ? 'white' : 'other';
  });
}

async function screenshot(page, name) {
  if (!process.env.OCR_SCREENSHOT_DIR) return;
  await mkdir(process.env.OCR_SCREENSHOT_DIR, { recursive: true });
  await page.screenshot({ path: join(process.env.OCR_SCREENSHOT_DIR, `${name}.png`), fullPage: true });
}

for (const locale of ['ar', 'en']) {
  test(`OCR paired pages, adapter, zoom and copy preserve exports in ${locale}`, async ({ page, browser }) => {
    const errors = [];
    const hosts = new Set();
    page.on('pageerror', (error) => errors.push(error.message));
    page.on('request', (request) => { if (/^https?:/.test(request.url())) hosts.add(new URL(request.url()).host); });
    const { next } = await openResult(page, { locale, source: true });
    const root = resultRoot(page);
    await rendered(page, 1);
    expect(await colour(page)).toBe('red');
    await expect(root.locator('[data-ocr-page-text]')).toHaveText(first);
    await expect(root.locator('[data-ocr-previous]')).toBeDisabled();
    const baseline = {};
    for (const extension of ['txt', 'docx', 'json']) {
      const response = await page.request.get(`${next}/document.${extension}`);
      expect(response.ok()).toBeTruthy();
      baseline[extension] = await response.body();
    }
    expect(baseline.txt.toString()).toBe(texts.join('\n\nPAGE_SEPARATOR\n\n'));
    expect(JSON.parse(baseline.json)).toEqual(texts.map((content, index) => ({ page: index + 1, content })));
    expect(baseline.docx.subarray(0, 2).toString()).toBe('PK');
    await screenshot(page, `${locale}-desktop-ocr-result`);

    await root.locator('[data-ocr-zoom-in]').click();
    await rendered(page, 1);
    const zoom = await root.locator('[data-ocr-zoom-value]').textContent();
    const sourcePane = root.locator('[data-ocr-source-viewport]');
    await sourcePane.focus();
    const documentScroll = await page.evaluate(() => scrollY);
    await sourcePane.press('ArrowDown');
    await expect.poll(() => sourcePane.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
    expect(await page.evaluate(() => scrollY)).toBe(documentScroll);
    await root.locator('[data-ocr-next]').click();
    await rendered(page, 2);
    expect(await colour(page)).toBe('white');
    await expect(root.locator('[data-ocr-page-text]')).toHaveText('');
    await expect(root.locator('.ocr-no-text')).toBeVisible();
    await expect(root.locator('[data-ocr-zoom-value]')).toHaveText(zoom);
    await root.locator('[data-ocr-next]').click();
    await rendered(page, 3);
    expect(await colour(page)).toBe('blue');
    await expect(root.locator('[data-ocr-next]')).toBeDisabled();
    await expect(root.locator('[data-ocr-page-text]')).toHaveText(third);
    await screenshot(page, `${locale}-desktop-ocr-page-three`);
    await root.locator('[data-ocr-fit="page"]').click();
    await rendered(page, 3);

    // The page adapter preserves all text, blank-page indices and last-request-wins behavior.
    expect(await root.evaluate(async (root) => {
      const adapter = window.arabiTools.ocrReview.get(root);
      const saved = [...adapter.texts];
      await Promise.all([adapter.show(0), adapter.show(2), adapter.show(1), adapter.show(2)]);
      return { texts: saved, label: adapter.label(2), hidden: adapter.element(0), visible: adapter.element(2).textContent };
    })).toEqual({ texts, label: locale === 'ar' ? 'الصفحة 3' : 'Page 3', hidden: null, visible: third });
    await rendered(page, 3);
    expect(await colour(page)).toBe('blue');
    await expect(root.locator('[data-ocr-fit="page"]')).toHaveAttribute('aria-pressed', 'true');

    await root.locator('[data-ocr-page-number]').fill('0');
    await root.locator('[data-ocr-page-number]').press('Enter');
    await expect(root.locator('[data-ocr-page-number]')).toHaveAttribute('aria-invalid', 'true');
    await expect(root.locator('[data-ocr-page-error]')).toBeVisible();
    await expect(root).toHaveAttribute('data-current-page', '3');
    await root.locator('[data-ocr-page-number]').fill('٢');
    await root.locator('[data-ocr-page-number]').press('Enter');
    await rendered(page, 2);

    await page.evaluate(() => {
      window.copied = [];
      Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (text) => window.copied.push(text) } });
    });
    await root.locator('[data-ocr-copy-page]').click();
    await root.locator('[data-ocr-copy-all]').click();
    expect(await page.evaluate(() => window.copied)).toEqual(['', texts.join('\n\n')]);
    await expect(root.locator('[data-ocr-copy-all]')).toBeFocused();
    await expect(root.locator('[data-copy-status]')).toHaveText(locale === 'ar' ? 'نُسخ النص' : 'Text copied');
    for (const extension of ['txt', 'docx', 'json']) {
      const downloading = page.waitForEvent('download');
      await root.locator(`a[href$="document.${extension}"]`).click();
      const download = await downloading;
      expect(download.suggestedFilename()).toBe(`review.${extension}`);
      expect(await readFile(await download.path())).toEqual(baseline[extension]);
    }
    expect(hosts).toEqual(new Set(['127.0.0.1:3204']));
    expect(errors).toEqual([]);
    console.log(`OCR ${locale}: ${browser.browserType().name()} ${browser.version()}`);
  });

  test(`OCR missing source, local recovery and 320px keyboard views in ${locale}`, async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 780 });
    await openResult(page, { locale });
    const root = resultRoot(page);
    await expect(root.locator('[data-ocr-page-text]')).toHaveText(first);
    await expect(root.locator('.ocr-text-panel')).toBeVisible();
    const sourceRadio = root.locator('.ocr-view-choice input[value="source"]');
    await sourceRadio.focus();
    await sourceRadio.press('Space');
    await expect(root.locator('[data-ocr-source-status]')).toHaveText(locale === 'ar'
      ? 'اختر الملف الأصلي لاستعادة المعاينة. لن تُعاد عملية استخراج النص.'
      : 'Select the original file to restore the preview. Recognition will not run again.');
    await screenshot(page, `${locale}-320-ocr-source-missing`);
    const posts = [];
    page.on('request', (request) => { if (request.method() === 'POST' && /\/ocr\/jobs/.test(request.url())) posts.push(request.url()); });
    await root.locator('[data-ocr-source-picker] button').click();
    await expect(root.locator('[data-ocr-confirm]')).toHaveAttribute('aria-invalid', 'true');
    await restore(page, pdf(1));
    await expect(root.locator('[data-ocr-source-error]')).toContainText(locale === 'ar' ? 'النتيجة على 3' : 'result has 3');
    await screenshot(page, `${locale}-320-ocr-source-mismatch`);
    await expect(root.locator('canvas')).toHaveCount(0);
    await restore(page, await original());
    await rendered(page, 1);
    await expect(root.locator('[data-ocr-source-status]')).toHaveText(locale === 'ar' ? 'أصل مرفق محليًا' : 'Locally attached source');
    await screenshot(page, `${locale}-320-ocr-source-restored`);
    await root.locator('[data-ocr-next]').click();
    await rendered(page, 2);
    await root.locator('[data-ocr-next]').click();
    await rendered(page, 3);
    await root.locator('.ocr-view-choice input[value="text"]').check();
    await expect(root.locator('[data-ocr-page-text]')).toHaveText(third);
    await screenshot(page, `${locale}-320-ocr-text`);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBeTruthy();
    await page.evaluate(() => { document.documentElement.style.fontSize = '150%'; });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBeTruthy();
    const css = await root.locator('[data-ocr-page-text]').evaluate((node) => ({ whiteSpace: getComputedStyle(node).whiteSpace, bidi: getComputedStyle(node).unicodeBidi, line: parseFloat(getComputedStyle(node).lineHeight), size: parseFloat(getComputedStyle(node).fontSize) }));
    expect(css.whiteSpace).toBe('pre-wrap');
    expect(css.bidi).toBe('plaintext');
    expect(css.line).toBeGreaterThanOrEqual(css.size * 1.9);
    expect(posts).toEqual([]);
    // A direct reload has recognized text immediately and honestly loses the local source.
    await page.reload();
    await expect(resultRoot(page)).toHaveAttribute('data-enhanced', 'true');
    await expect(resultRoot(page).locator('canvas')).toHaveCount(0);
    await expect(resultRoot(page).locator('[data-ocr-page-text]')).toHaveText(first);
  });
}

test('OCR preview failures and denied clipboard keep text and exact copy scopes', async ({ page }) => {
  await openResult(page);
  const root = resultRoot(page);
  const posts = [];
  page.on('request', (request) => { if (request.method() === 'POST') posts.push(request.url()); });
  await restore(page, Buffer.from('GIF89a'), 'wrong.png', 'image/png');
  await expect(root.locator('[data-ocr-source-error]')).toHaveText('Choose exactly one PDF, JPG or PNG file.');
  await restore(page, Buffer.from('%PDF-not a readable document'), 'broken.pdf');
  await expect(root.locator('[data-ocr-source-error]')).toContainText('local preview could not be displayed');
  await screenshot(page, 'en-desktop-ocr-preview-failure');
  await expect(root.locator('[data-ocr-page-text]')).toHaveText(first);
  await page.route('**/pdf.worker-*.mjs', (route) => route.abort());
  await restore(page, await original());
  await expect(root.locator('[data-ocr-source-error]')).toContainText('local preview could not be displayed');
  await expect(root.locator('a[download]')).toHaveCount(3);
  await page.evaluate(() => Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async () => { throw new Error('unavailable'); } } }));
  await root.locator('[data-ocr-copy-all]').click();
  await expect(root.locator('textarea')).toHaveValue(texts.join('\n\n'));
  expect(await root.locator('textarea').evaluate((field) => field.value.slice(field.selectionStart, field.selectionEnd))).toBe(texts.join('\n\n'));
  await root.getByRole('button', { name: 'Close copy panel', exact: true }).click();
  await expect(root.locator('textarea')).toHaveCount(0);
  await expect(root.locator('[data-ocr-copy-all]')).toBeFocused();
  await root.locator('[data-ocr-next]').click();
  await root.locator('[data-ocr-copy-page]').click();
  await expect(root.locator('textarea')).toHaveValue('');
  expect(posts).toEqual([]);
});

test('OCR render failure recovers locally and an abandoned source load stays absent', async ({ page }) => {
  await page.addInitScript(() => {
    const context = HTMLCanvasElement.prototype.getContext;
    window.failNextOcrRender = true;
    HTMLCanvasElement.prototype.getContext = function (...args) {
      if (window.failNextOcrRender && this.closest('[data-ocr-source-viewport]')) {
        window.failNextOcrRender = false;
        throw new Error('Synthetic local render failure');
      }
      return context.apply(this, args);
    };
  });
  const { jobId, bytes } = await openResult(page, { source: true });
  const root = resultRoot(page);
  await expect(root.locator('[data-ocr-source-error]')).toContainText('local preview could not be displayed');
  await expect(root.locator('[data-ocr-page-text]')).toHaveText(first);
  await restore(page, bytes);
  await rendered(page, 1);
  await page.evaluate(({ jobId, bytes }) => {
    class DelayedFile extends File {
      async arrayBuffer() {
        await new Promise((resolve) => setTimeout(resolve, 100));
        return super.arrayBuffer();
      }
    }
    window.arabiTools.sources.set(jobId, new DelayedFile([new Uint8Array(bytes)], 'original.pdf'));
    document.dispatchEvent(new CustomEvent('arabi:source', { detail: { jobId } }));
    window.arabiTools.sources.delete(jobId);
    document.dispatchEvent(new CustomEvent('arabi:source', { detail: { jobId } }));
  }, { jobId, bytes: Array.from(bytes) });
  await expect(root.locator('[data-ocr-source-status]')).toHaveText('Select the original file to restore the preview. Recognition will not run again.');
  await root.locator('[data-ocr-next]').click();
  await expect(root.locator('[data-ocr-page-text]')).toHaveText('');
  await root.locator('[data-ocr-next]').click();
  await expect(root.locator('[data-ocr-page-text]')).toHaveText(third);
  await expect(root.locator('canvas')).toHaveCount(0);
});

test('OCR image source works, rejects unreadable images and releases object URLs', async ({ page }) => {
  await trackResources(page);
  const bytes = await readFile(fixture('ocr-review.png'));
  const { jobId } = await openResult(page, { bytes, name: 'page.png', type: 'image/png', source: true });
  const root = resultRoot(page);
  await rendered(page, 1);
  await expect(root.locator('img')).toHaveJSProperty('naturalWidth', 600);
  await expect(root.locator('[data-ocr-page-text]')).toHaveText(first);
  await expect(root.locator('[data-ocr-next]')).toBeDisabled();
  await page.evaluate(({ jobId }) => {
    window.arabiTools.sources.set(jobId, new File([new Uint8Array([137,80,78,71,13,10,26,10])], 'broken.png', { type: 'image/png' }));
    document.dispatchEvent(new CustomEvent('arabi:source', { detail: { jobId } }));
  }, { jobId });
  await expect(root.locator('[data-ocr-source-error]')).toContainText('local preview could not be displayed');
  await expect(root.locator('[data-ocr-page-text]')).toHaveText(first);
  await restore(page, await readFile(fixture('ocr-review.jpg')), 'different-name.jpg', 'image/jpeg');
  await rendered(page, 1);
  await expect(root.locator('img')).toHaveJSProperty('naturalWidth', 600);
  await expect.poll(() => page.evaluate(() => window.ocrResources.urls.size)).toBe(1);
  await root.evaluate((node) => node.remove());
  await expect.poll(() => page.evaluate(() => window.ocrResources.urls.size)).toBe(0);
});

test('OCR all-blank document keeps every page and never copies placeholder text', async ({ page }) => {
  await openResult(page, { bytes: pdf(3), source: true });
  const root = resultRoot(page);
  await expect(root.locator('.ocr-empty-document')).toHaveText('No text was found in this document. You can still review its pages.');
  await rendered(page, 1);
  await root.evaluate(async (node) => {
    const adapter = window.arabiTools.ocrReview.get(node);
    await adapter.show(2);
  });
  await rendered(page, 3);
  expect(await root.evaluate((node) => window.arabiTools.ocrReview.get(node).texts)).toEqual(['', '', '']);
  await page.evaluate(() => Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined }));
  await root.locator('[data-ocr-copy-all]').click();
  await expect(root.locator('textarea')).toHaveValue('\n\n\n\n');
});

test('OCR bundle modules, fonts and worker load from a deep route and dispose on removal', async ({ page }) => {
  await trackResources(page);
  await openResult(page);
  const root = resultRoot(page);
  for (const attribute of ['data-pdf-module', 'data-pdf-worker']) {
    const url = await root.getAttribute(attribute);
    expect(url.startsWith('/')).toBeTruthy();
    const response = await page.request.get(url);
    expect(response.ok()).toBeTruthy();
    expect(response.headers()['content-type']).toMatch(/(?:application|text)\/javascript/);
    expect((await response.body()).length).toBeGreaterThan(100_000);
  }
  const resources = JSON.parse(await root.getAttribute('data-pdf-resources'));
  for (const name of ['LiberationSans-Regular.ttf', 'Adobe-Japan1-UCS2.bcmap', 'jbig2.wasm', 'openjpeg.wasm']) {
    const response = await page.request.get(resources[name]);
    expect(response.ok()).toBeTruthy();
    expect((await response.body()).length).toBeGreaterThan(100);
  }
  await restore(page, await original());
  await rendered(page, 1);
  await expect.poll(() => page.evaluate(() => window.ocrResources.workers.size)).toBe(1);
  await root.evaluate((node) => node.remove());
  await expect.poll(() => page.evaluate(() => window.ocrResources.workers.size)).toBe(0);
  await expect.poll(() => page.evaluate(() => window.ocrResources.urls.size)).toBe(0);
});

async function trackResources(page) {
  await page.addInitScript(() => {
    window.ocrResources = { workers: new Set(), urls: new Set() };
    const create = URL.createObjectURL.bind(URL);
    const revoke = URL.revokeObjectURL.bind(URL);
    URL.createObjectURL = (blob) => { const url = create(blob); window.ocrResources.urls.add(url); return url; };
    URL.revokeObjectURL = (url) => { window.ocrResources.urls.delete(url); return revoke(url); };
    const OriginalWorker = window.Worker;
    window.Worker = class extends OriginalWorker {
      constructor(...args) { super(...args); window.ocrResources.workers.add(this); }
      terminate() { window.ocrResources.workers.delete(this); return super.terminate(); }
    };
  });
}

test('OCR 1000-page viewer bounds canvases and DOM while retaining every page text', async ({ page, browserName }, testInfo) => {
  test.setTimeout(120_000);
  await page.addInitScript(() => Object.defineProperty(window, 'devicePixelRatio', { value: 4 }));
  await openResult(page);
  // Exercise the result-root contract without asking the Drive mock to recognize 1000
  // pages. The PDF is real; only the completed text fixture is supplied at this boundary.
  const bytes = pdf(1000, { largeLast: true });
  await resultRoot(page).evaluate((oldRoot, bytes) => {
    const root = oldRoot.cloneNode(true);
    const pages = root.querySelector('[data-ocr-pages]');
    pages.replaceChildren();
    for (let index = 0; index < 1000; index++) {
      const text = document.createElement('p');
      text.dataset.ocrPageText = '';
      text.textContent = `Page ${index + 1}: ${'كِتَاب نص طويل 😀 '.repeat(50)}${index === 999 ? 'Final needle' : ''}`;
      pages.append(text);
    }
    root.dataset.jobId = 'large-fixture';
    delete root.dataset.enhanced;
    window.arabiTools.sources ??= new Map();
    window.arabiTools.sources.set('large-fixture', new File([new Uint8Array(bytes)], 'large.pdf', { type: 'application/pdf' }));
    oldRoot.replaceWith(root);
  }, Array.from(bytes));
  await rendered(page, 1);
  const root = resultRoot(page);
  const initialNodes = await root.locator('*').count();
  const searchStart = performance.now();
  await root.locator('[data-search-query]').fill('Final needle');
  await expect(root.locator('[data-search-count]')).toContainText('1');
  await rendered(page, 1000);
  await expect(root.locator('[data-ocr-page-text]')).toContainText('Final needle');
  const searchMs = Math.round(performance.now() - searchStart);
  await root.locator('[data-search-clear]').click();
  const times = [];
  for (const number of [500, 999, 1, 1000, 2, 1000]) {
    const start = performance.now();
    await root.evaluate((node, number) => window.arabiTools.ocrReview.get(node).show(number - 1), number);
    await rendered(page, number);
    times.push(Math.round(performance.now() - start));
  }
  expect(await root.locator('*').count()).toBe(initialNodes);
  expect(initialNodes).toBeLessThan(150);
  await expect(root.locator('canvas')).toHaveCount(1);
  await root.locator('[data-ocr-zoom-in]').click();
  await root.locator('[data-ocr-zoom-in]').click();
  await rendered(page, 1000);
  const bounds = await root.locator('canvas').evaluate((canvas) => ({ width: canvas.width, height: canvas.height }));
  expect(bounds.width).toBeLessThanOrEqual(4096);
  expect(bounds.height).toBeLessThanOrEqual(4096);
  expect(bounds.width * bounds.height).toBeLessThanOrEqual(8 * 1024 * 1024);
  expect(await root.evaluate((node) => {
    const adapter = window.arabiTools.ocrReview.get(node);
    return { count: adapter.texts.length, last: adapter.texts.findIndex((text) => text.includes('Final needle')) };
  })).toEqual({ count: 1000, last: 999 });
  await page.evaluate(() => Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
    writeText: async text => { window.largeOcrCopy = text; },
  } }));
  await root.locator('[data-ocr-copy-all]').click();
  expect(await root.evaluate(node => window.largeOcrCopy === window.arabiTools.ocrReview.get(node).texts.join('\n\n'))).toBe(true);
  let memory = null;
  if (browserName === 'chromium') {
    const cdp = await page.context().newCDPSession(page);
    await cdp.send('HeapProfiler.collectGarbage');
    memory = { ...(await cdp.send('Runtime.getHeapUsage')), ...(await cdp.send('Memory.getDOMCounters')) };
    await cdp.detach();
  }
  const measurement = { browser: browserName, searchMs, pageSwitchMs: times, resultElements: initialNodes, canvas: bounds, memory };
  console.log('OCR large-document measurement', JSON.stringify(measurement));
  await testInfo.attach('ocr-large-document.json', { body: JSON.stringify(measurement, null, 2), contentType: 'application/json' });
});
