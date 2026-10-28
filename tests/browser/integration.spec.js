import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';

const fixture = name => fileURLToPath(new URL(`../fixtures/${name}`, import.meta.url));
const first = 'الصفحة الأولى — كِتَاب 1\nFirst page: café 😀 <b> & نص مشترك';
const third = 'الصفحة الثالثة — مـدرسة 3\nThird page: آخر نص مشترك';
const unisolate = value => value.replace(/[\u2066-\u2069]/g, '');

test.beforeEach(async ({ page }) => {
  page.on('pageerror', error => { throw error; });
});

async function open(page, baseURL, locale, tool) {
  await page.context().addCookies([{ name: 'lang', value: locale, url: baseURL }]);
  await page.goto(`/${tool}`);
  await page.evaluate(() => {
    window.integrationDocument = performance.timeOrigin;
    window.integrationInput = document.querySelector('[name=file]');
  });
}

async function submit(page, file, type) {
  await page.locator('#upload-form [name=file]').setInputFiles(file);
  await page.evaluate(() => { window.integrationFile = document.querySelector('#upload-form [name=file]').files[0]; });
  const existing = page.locator(`.workspace-result [data-result="${type}"]`);
  const previous = await existing.count() ? await existing.getAttribute('data-job-id') : null;
  await page.locator('#upload-form [type=submit]').click();
  const root = page.locator(`.workspace-result [data-result="${type}"]`);
  await expect(root).toHaveAttribute('data-job-id', /[a-f0-9]{32}/);
  if (previous) await expect(root).not.toHaveAttribute('data-job-id', previous);
  await expect(root).toHaveAttribute(type === 'ocr' ? 'data-enhanced' : 'data-transcript-ready', 'true');
  expect(await page.evaluate(id => performance.timeOrigin === window.integrationDocument
    && document.querySelector('#upload-form [name=file]') === window.integrationInput
    && window.arabiTools.sources.get(id) === window.integrationFile, await root.getAttribute('data-job-id'))).toBe(true);
  return root;
}

async function rendered(root, number, colour) {
  await expect(root).toHaveAttribute('data-current-page', String(number));
  await expect(root.locator('[data-ocr-source-viewport]')).toHaveAttribute('data-rendered-page', String(number));
  expect(await root.locator('canvas').evaluate(canvas => {
    const [r, g, b] = canvas.getContext('2d').getImageData(Math.floor(canvas.width / 10), Math.floor(canvas.height / 2), 1, 1).data;
    return r - b > 60 ? 'red' : b - r > 60 ? 'blue' : r > 240 && g > 240 && b > 240 ? 'white' : 'other';
  })).toBe(colour);
}

async function trackCopy(page) {
  await page.evaluate(() => {
    window.integrationCopies = [];
    window.integrationClipboard = [];
    const copy = window.arabiTools.copyText;
    window.arabiTools.copyText = (text, options) => {
      window.integrationCopies.push(text);
      return copy(text, options);
    };
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      writeText: async text => window.integrationClipboard.push(text),
    } });
  });
}

for (const locale of ['ar', 'en']) {
  test(`integrated OCR source, search, replacement and reload in ${locale}`, async ({ page, baseURL }) => {
    await open(page, baseURL, locale, 'ocr');
    const root = await submit(page, fixture('ocr-review.pdf'), 'ocr');
    await rendered(root, 1, 'red');
    await expect(root.locator('[data-ocr-page-text]')).toHaveText(first);
    await expect(root.locator('[data-ocr-source-picker]')).toBeHidden();
    await trackCopy(page);
    await root.locator('.zoom-menu > summary').click();
    await root.locator('[data-ocr-zoom-in]').click();
    await rendered(root, 1, 'red');
    const zoom = await root.locator('[data-ocr-zoom-value]').textContent();
    const query = root.locator('[data-search-query]');
    await query.fill('نص مشترك');
    await expect(root.locator('[data-search-count]')).toContainText('2');
    await query.press('Enter');
    await rendered(root, 3, 'blue');
    await expect(query).toBeFocused();
    await expect(root.locator('[data-ocr-page-text]')).toHaveText(third);
    await expect(root.locator('[data-ocr-zoom-value]')).toHaveText(zoom);
    await root.locator('[data-ocr-previous]').click();
    await rendered(root, 2, 'white');
    await expect(root.locator('[data-ocr-page-text]')).toHaveText('');
    await expect(query).toHaveValue('نص مشترك');
    // Both copy scopes are chosen from the header's Copy menu.
    for (const scope of ['page', 'all']) {
      await root.locator('.copy-menu > summary').click();
      await root.locator(`[data-ocr-copy-${scope}]`).click();
    }
    expect(await page.evaluate(() => window.integrationCopies)).toEqual(['', [first, '', third].join('\n\n')]);
    expect(await page.evaluate(() => window.integrationClipboard)).toEqual(['', [first, '', third].join('\n\n')]);

    const oldId = await root.getAttribute('data-job-id');
    await root.evaluate(node => { window.integrationOldRoot = node; });
    // A new selection and a refused retry keep the old source paired with its old text.
    await page.locator('#upload-form [name=file]').setInputFiles(fixture('ocr-review.png'));
    await rendered(root, 2, 'white');
    await page.route('**/ocr/jobs', route => route.fulfill({ status: 503, json: { error: 'Queue full for this test.' } }));
    await page.locator('#upload-form [type=submit]').click();
    await expect(page.locator('#upload-error')).toBeVisible();
    await rendered(root, 2, 'white');
    await page.unroute('**/ocr/jobs');
    const next = await submit(page, fixture('ocr-review.png'), 'ocr');
    const newId = await next.getAttribute('data-job-id');
    await expect(next.locator('img')).toHaveJSProperty('naturalWidth', 600);
    await expect(next.locator('[data-ocr-page-text]')).toHaveText(first);
    await expect(next.locator('[data-search-query]')).toHaveValue('');
    await expect(next.locator('[data-ocr-next]')).toBeDisabled();
    expect(await page.evaluate(oldId => !window.integrationOldRoot.isConnected
      && !window.arabiTools.sources.has(oldId)
      && window.arabiTools.ocrReview.get(window.integrationOldRoot) === undefined
      && window.arabiTools.sources.size === 1, oldId)).toBe(true);
    for (const link of await next.locator('a[download]').all()) await expect(link).toHaveAttribute('href', new RegExp(`/ocr/jobs/${newId}/`));

    const job = await page.locator('.workspace-recovery').getAttribute('href');
    await expect(page.locator('.language-switch [name=back]')).toHaveValue(job);
    let explanation;
    page.once('dialog', async dialog => { explanation = dialog.message(); await dialog.accept(); });
    await page.locator('.language-switch button').click();
    await expect(page).toHaveURL(new RegExp(`${job}$`));
    expect(explanation).toContain(locale === 'ar' ? 'إعادة تحميل' : 'reloads');
    const direct = page.locator('[data-result="ocr"]');
    await expect(direct).toHaveAttribute('data-enhanced', 'true');
    await expect(direct.locator('img')).toHaveCount(0);
    await expect(direct.locator('[data-ocr-source-picker]')).toBeVisible();
    await expect(direct.locator('[data-ocr-page-text]')).toHaveText(first);
    const posts = [];
    page.on('request', request => { if (request.method() === 'POST' && request.url().includes('/ocr/jobs')) posts.push(request.url()); });
    await direct.locator('[data-ocr-source-file]').setInputFiles(fixture('ocr-review.png'));
    await direct.locator('[data-ocr-confirm]').check();
    await direct.locator('[data-ocr-source-picker] button').click();
    await expect(direct.locator('img')).toHaveJSProperty('naturalWidth', 600);
    // The picker clears after attaching, but the locally held preview still needs
    // the same reload explanation as an uploaded source.
    explanation = null;
    page.once('dialog', async dialog => { explanation = dialog.message(); await dialog.accept(); });
    await page.locator('.language-switch button').click();
    await expect(page).toHaveURL(new RegExp(`${job}$`));
    expect(explanation).toContain(locale === 'ar' ? 'reloads' : 'إعادة تحميل');
    await expect(direct).toHaveAttribute('data-enhanced', 'true');
    await expect(direct.locator('img')).toHaveCount(0);
    await page.reload();
    await expect(direct).toHaveAttribute('data-enhanced', 'true');
    await expect(direct.locator('img')).toHaveCount(0);
    await expect(direct.locator('[data-ocr-page-text]')).toHaveText(first);
    expect(posts).toEqual([]);
  });

  test(`integrated transcript search, shared copy and consecutive results in ${locale}`, async ({ page, baseURL }) => {
    await open(page, baseURL, locale, 'transcribe');
    const root = await submit(page, fixture('speech.mp3'), 'transcript');
    await trackCopy(page);
    await root.locator('[data-search-query]').fill('كتاب');
    const total = await root.evaluate(node => window.arabiTools.search.find(
      [...node.querySelectorAll('.cue-text')].map(cue => cue.textContent), 'كتاب', { ignoreMarks: true, ignoreTatweel: true }).length);
    await expect.poll(async () => unisolate(await root.locator('[data-search-count]').textContent())).toContain(String(total));
    await root.locator('[data-copy-all]').click();
    const download = await page.request.get(await root.locator('a[href$="transcript.txt"]').getAttribute('href'));
    expect(await page.evaluate(() => window.integrationCopies)).toEqual([await download.text()]);
    const oldId = await root.getAttribute('data-job-id');
    await root.evaluate(node => { window.integrationOldRoot = node; });
    const next = await submit(page, fixture('speech.m4a'), 'transcript');
    const newId = await next.getAttribute('data-job-id');
    await expect(next.locator('[data-search-query]')).toHaveValue('');
    expect(await page.evaluate(oldId => !window.integrationOldRoot.isConnected
      && !window.arabiTools.sources.has(oldId) && window.arabiTools.sources.size === 1, oldId)).toBe(true);
    for (const link of await next.locator('a[download]').all()) await expect(link).toHaveAttribute('href', new RegExp(`/transcribe/jobs/${newId}/`));
    const job = await page.locator('.workspace-recovery').getAttribute('href');
    await expect(page.locator('.language-switch [name=back]')).toHaveValue(job);
    page.once('dialog', dialog => dialog.accept());
    await page.locator('.language-switch button').click();
    await expect(page).toHaveURL(new RegExp(`${job}$`));
    await expect(page.locator('[data-result="transcript"]')).toHaveAttribute('data-transcript-ready', 'true');
  });
}

test('integrated diacritization copy uses shared feedback and selects the displayed output on denial', async ({ page, baseURL }) => {
  await open(page, baseURL, 'en', 'diacritize');
  await page.locator('#source-text').fill('هذا كتاب عربي مفيد.');
  await page.locator('.diacritize-form [type=submit]:visible').click();
  const root = page.locator('.workspace-result [data-result="diacritization"]');
  await expect(root).toBeVisible();
  await trackCopy(page);
  const output = root.locator('[name=result]');
  const text = await output.inputValue();
  await root.locator('[data-copy]').click();
  expect(await page.evaluate(() => window.integrationCopies)).toEqual([text]);
  await expect(root.locator('[data-copy-status]')).toHaveText('Text copied');
  await page.evaluate(() => Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined }));
  await root.locator('[data-copy]').click();
  expect(await output.evaluate(field => field.value.slice(field.selectionStart, field.selectionEnd))).toBe(text);
  await expect(root.locator('[data-copy-status]')).toHaveText('Copying was unavailable. Select the text and copy it manually.');
});
