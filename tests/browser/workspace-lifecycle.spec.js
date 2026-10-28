import { createServer } from 'node:http';
import { execFileSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';
import { allowClipboard } from './review-helpers.js';

const speech = fileURLToPath(new URL('../fixtures/speech.mp3', import.meta.url));
const image = execFileSync('python3', ['-c', `
import struct, sys, zlib
def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
sys.stdout.buffer.write(b'\\x89PNG\\r\\n\\x1a\\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 32, 32, 8, 0, 0, 0, 0)) + chunk(b'IDAT', zlib.compress((b'\\0' + b'\\xff' * 32) * 32)) + chunk(b'IEND', b''))
`]);
const pending = () => {
  let release;
  const promise = new Promise(resolve => { release = resolve; });
  return { promise, release };
};

async function locale(page, tool, language) {
  await page.goto(`/${tool}`);
  if (language === 'en') await page.getByRole('button', { name: 'English', exact: true }).click();
}

async function fits(page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
}

for (const tool of ['transcribe', 'ocr']) {
  for (const language of ['ar', 'en']) {
    test(`${tool} keeps its input and source ownership across retry in ${language}, desktop and 320px`, async ({ page }) => {
      await locale(page, tool, language);
      const file = tool === 'transcribe' ? { name: 'speech.mp3', mimeType: 'audio/mpeg', buffer: await readFile(speech) }
        : { name: 'page.png', mimeType: 'image/png', buffer: image };
      const input = page.locator('#upload-form [name=file]');
      const submit = page.locator('#upload-form [type=submit]');
      await input.setInputFiles(file);
      await page.evaluate(() => {
        window.__sourceInput = document.querySelector('[name=file]');
        window.__file = window.__sourceInput.files[0];
        window.__sentinel = performance.timeOrigin;
        window.__sources = [];
        document.addEventListener('arabi:source', event => {
          window.__sources.push({ id: event.detail.jobId, same: window.arabiTools.sources.get(event.detail.jobId) === window.__file, hasResult: !!document.querySelector('.workspace-result .result') });
        });
      });
      await expect(page.locator('.file-name')).toHaveText(file.name);
      await expect(page.locator('.file-size')).toContainText(tool === 'transcribe' ? 'MP3' : 'PNG');
      await submit.click();
      await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
      const firstJob = (await page.locator('.workspace-recovery').getAttribute('href')).split('/').pop();
      expect(await page.evaluate(id => window.arabiTools.sources.get(id) === window.__file, firstJob)).toBe(true);
      expect(await page.evaluate(() => window.__sources[0])).toEqual({ id: firstJob, same: true, hasResult: false });
      const recognized = page.locator('.workspace-result').locator(tool === 'ocr' ? '[data-ocr-page-text]' : '.cue-text');
      const originalResult = await recognized.allTextContents();
      const downloads = await page.locator('.workspace-result a[download]').evaluateAll(links => links.map(link => link.href));
      await page.evaluate(() => { window.__result = document.querySelector('.workspace-result .result'); });
      await fits(page);
      await page.setViewportSize({ width: 320, height: 740 });
      await fits(page);
      // A failed new reservation retains the previous result's original file.
      await input.setInputFiles({ ...file, name: `next-${file.name}` });
      expect(await page.evaluate(id => window.arabiTools.sources.get(id) === window.__file, firstJob)).toBe(true);
      await page.route(`**/${tool}/jobs`, route => route.fulfill({ status: 503, json: { error: language === 'ar' ? 'الطابور ممتلئ الآن.' : 'The queue is full.' } }));
      await submit.click();
      await expect(page.getByRole('alert')).toBeVisible();
      await expect(page.locator('.previous-result')).toBeVisible();
      // Responsive Source/Text controls change visibility; recognized text and
      // its download targets must remain tied to the previous result.
      expect(await recognized.allTextContents()).toEqual(originalResult);
      expect(await page.locator('.workspace-result a[download]').evaluateAll(links => links.map(link => link.href))).toEqual(downloads);
      expect(await page.evaluate(() => document.querySelector('.workspace-result .result') === window.__result)).toBe(true);
      await page.unroute(`**/${tool}/jobs`);
      await page.evaluate(() => { window.__file = document.querySelector('[name=file]').files[0]; });
      await submit.click();
      await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
      const nextJob = (await page.locator('.workspace-recovery').getAttribute('href')).split('/').pop();
      expect(nextJob).not.toBe(firstJob);
      expect(await page.evaluate(({ firstJob, nextJob }) => ({
        old: window.arabiTools.sources.has(firstJob), current: window.arabiTools.sources.get(nextJob) === window.__file,
        source: document.querySelector('[name=file]') === window.__sourceInput, document: performance.timeOrigin === window.__sentinel,
        size: window.arabiTools.sources.size,
      }), { firstJob, nextJob })).toEqual({ old: false, current: true, source: true, document: true, size: 1 });
      await fits(page);
      await expect(submit).toBeFocused();
      await page.locator('.remove-file').click();
      await expect(input).toHaveJSProperty('value', '');
      await expect(page.locator('.file-name')).not.toContainText(file.name);
      await expect(page.locator('.file-size')).toBeEmpty();
      // Removing the next input does not detach the displayed result's original.
      expect(await page.evaluate(id => window.arabiTools.sources.has(id), nextJob)).toBe(true);
      await page.locator('[data-workspace]').evaluate(root => root.remove());
      await expect.poll(() => page.evaluate(() => window.arabiTools.sources.size)).toBe(0);
    });
  }
}

for (const tool of ['transcribe', 'ocr']) {
  test(`${tool} folds its input beside a current result, never above the view, with one privacy line`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 700 });
    await page.goto(`/${tool}`);
    const form = page.locator('#upload-form');
    const input = form.locator('[name=file]');
    const submit = form.locator('[type=submit]');
    const inputNote = page.locator('[data-workspace] > .privacy-note');
    const resultNote = page.locator('.workspace-result .privacy-note');
    const file = tool === 'transcribe' ? { name: 'speech.mp3', mimeType: 'audio/mpeg', buffer: await readFile(speech) }
      : { name: 'page.png', mimeType: 'image/png', buffer: image };
    await input.setInputFiles(file);
    await expect(inputNote).toBeVisible();
    await expect(submit).toHaveClass(/\bprimary\b/);
    await submit.click();
    await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
    // Result first: one bar holding the file and a secondary run button, which keeps its focus.
    await expect(form).toHaveAttribute('data-collapsed', '');
    await expect(submit).toHaveClass(/\bsecondary\b/);
    await expect(submit).not.toHaveClass(/\bprimary\b/);
    await expect(submit).toBeFocused();
    await expect(page.locator('.file-name')).toHaveText(file.name);
    expect(await form.evaluate(node => node.getBoundingClientRect().height)).toBeLessThan(120);
    // The privacy line moves to the foot of the result; the header has no retention line.
    await expect(inputNote).toBeHidden();
    await expect(resultNote).toBeVisible();
    await expect(page.locator('.workspace-result .result-meta')).toBeHidden();
    // Another file unfolds the input with the filled run button.
    await input.setInputFiles({ ...file, name: `other-${file.name}` });
    await expect(form).not.toHaveAttribute('data-collapsed');
    await expect(submit).toHaveClass(/\bprimary\b/);
    // Scrolled into the previous result, the input keeps its size when the next result arrives,
    // and folds once it is back in view.
    const reservation = pending();
    await page.route(`**/${tool}/jobs`, async route => { await reservation.promise; await route.continue(); });
    await submit.click();
    await page.evaluate(() => {
      const spacer = document.createElement('div');
      spacer.style.blockSize = '2000px';
      document.querySelector('main').append(spacer);
      window.scrollTo(0, document.querySelector('.workspace-result').getBoundingClientRect().top + window.scrollY + 40);
    });
    expect(await form.evaluate(node => node.getBoundingClientRect().top)).toBeLessThan(0);
    reservation.release();
    await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
    await expect(page.locator('.workspace-result [data-result]')).toBeVisible();
    await expect(form).not.toHaveAttribute('data-collapsed');
    await expect(submit).toHaveClass(/\bprimary\b/);
    await page.evaluate(() => window.scrollTo(0, 0));
    await expect(form).toHaveAttribute('data-collapsed', '');
    await expect(submit).toBeFocused();
    // Removing the file unfolds the input to the drop zone, and nothing can be sent.
    await page.locator('.remove-file').click();
    await expect(form).not.toHaveAttribute('data-collapsed');
    await expect(submit).toHaveClass(/\bprimary\b/);
    await expect(submit).toBeDisabled();
    // A direct job page has the one privacy line too, at the foot of its result.
    const job = await page.locator('.workspace-recovery').getAttribute('href');
    await page.goto(job);
    await expect(page.locator('.privacy-note')).toHaveCount(1);
    await expect(page.locator('[data-result] .privacy-note')).toBeVisible();
  });
}

test('diacritization says what it replaces in the empty output and keeps one privacy line', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await locale(page, 'diacritize', 'en');
  await expect(page.locator('.result-placeholder')).toHaveText('The diacritized text appears here, with new diacritics replacing any common ones in your text; review it before use.');
  await expect(page.locator('.privacy-note')).toHaveCount(1);
  await expect(page.locator('[data-workspace] > .privacy-note')).toBeVisible();
  await page.locator('#source-text').fill('اللغة العربية جميلة');
  await page.getByRole('button', { name: 'Diacritize text', exact: true }).click();
  await expect(page.locator('.workspace-result .result')).toBeVisible();
  await expect(page.locator('.result-placeholder')).toBeHidden();
  await expect(page.locator('[data-workspace] > .privacy-note')).toBeHidden();
  await expect(page.locator('.workspace-result .privacy-note')).toBeVisible();
  await expect(page.locator('.workspace-result .result-meta')).not.toContainText('30');
});

test('upload stages distinguish reservation, transfer and validation and prevent double submit', async ({ page }) => {
  await page.goto('/transcribe');
  const reserve = pending();
  const upload = pending();
  let reserved = 0;
  let checked = false;
  let backendUpload;
  const requests = [];
  const origin = new URL(page.url()).origin;
  // A real local HTTP receiver lets the browser finish transferring bytes while
  // its response is held. Playwright request interception holds the transfer itself.
  const receiver = createServer((request, response) => {
    const handle = async () => {
      const headers = { 'Access-Control-Allow-Origin': origin, 'Access-Control-Allow-Methods': 'POST, OPTIONS', 'Access-Control-Allow-Headers': 'Content-Type' };
      if (request.method === 'OPTIONS') { response.writeHead(204, headers); response.end(); return; }
      const chunks = [];
      for await (const chunk of request) chunks.push(chunk);
      checked = true;
      const accepted = await fetch(`${origin}${backendUpload}`, { method: 'POST', headers: { 'Content-Type': request.headers['content-type'] }, body: Buffer.concat(chunks) });
      const body = await accepted.text();
      await upload.promise;
      response.writeHead(accepted.status, { ...headers, 'Content-Type': 'application/json' });
      response.end(body);
    };
    requests.push(handle());
  });
  await new Promise(resolve => receiver.listen(0, '127.0.0.1', resolve));
  await page.route('**/transcribe/jobs', async route => {
    reserved++;
    await reserve.promise;
    const response = await route.fetch();
    const reply = await response.json();
    backendUpload = reply.upload;
    await route.fulfill({ response, json: { ...reply, upload: `http://127.0.0.1:${receiver.address().port}/audio` } });
  });
  try {
    await page.locator('[name=file]').setInputFiles(speech);
    await page.locator('[type=submit].primary').click();
    await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'reserving');
    await expect(page.locator('.submission-status')).toHaveText('جارٍ طلب مكان في الطابور…');
    await expect(page.locator('.upload-progress progress')).not.toHaveAttribute('value');
    // Without a percentage the bar sweeps and the status turns a spinner, unless motion is reduced.
    await expect(page.locator('.upload-progress progress')).toHaveCSS('animation-name', 'progress-sweep');
    expect(await page.locator('.submission-status').evaluate(node => getComputedStyle(node, '::before').animationName)).toBe('spin');
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await expect(page.locator('.upload-progress progress')).toHaveCSS('animation-name', 'none');
    await page.emulateMedia({ reducedMotion: 'no-preference' });
    // The file row offers no way to change the file while a job runs.
    await expect(page.locator('[name=file]')).toBeDisabled();
    await expect(page.locator('.remove-file')).toBeDisabled();
    await page.locator('#upload-form').evaluate(form => form.requestSubmit());
    expect(reserved).toBe(1);
    reserve.release();
    await expect.poll(() => checked).toBe(true);
    // These are native XHR upload-completion events, with the response held.
    await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'validating');
    await expect(page.locator('.submission-status')).toHaveText('اكتمل الرفع. جارٍ التحقق من الملف…');
    await expect(page.locator('.submission-percent')).toBeEmpty();
    await expect(page.locator('.upload-progress progress')).not.toHaveAttribute('value');
    upload.release();
    await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
  } finally {
    reserve.release();
    upload.release();
    await Promise.all(requests);
    receiver.closeAllConnections();
    await new Promise(resolve => receiver.close(resolve));
  }
});

test('language reload explains draft loss, returns to the accepted job and never resubmits', async ({ page }) => {
  await page.goto('/diacritize');
  const source = page.locator('#source-text');
  await source.fill('اللغة العربية جميلة');
  let reservations = 0;
  page.on('request', request => { if (request.method() === 'POST' && new URL(request.url()).pathname === '/diacritize/jobs') reservations++; });
  await page.getByRole('button', { name: 'شكِّل النص', exact: true }).click();
  await expect(page.locator('.workspace-result .result')).toBeVisible();
  const job = await page.locator('.workspace-recovery').getAttribute('href');
  const text = await page.locator('#diacritized-text').inputValue();
  await source.fill('مسودة غير مرسلة');
  await expect(page.locator('.language-switch [name=back]')).toHaveValue(job);
  let explanation;
  page.once('dialog', async dialog => { explanation = dialog.message(); await dialog.dismiss(); });
  await page.getByRole('button', { name: 'English', exact: true }).click();
  expect(explanation).toBe('يؤدي تغيير اللغة إلى إعادة تحميل الصفحة. قد تفقد التعديلات غير المرسلة والمعاينة المحلية.');
  await expect(source).toHaveValue('مسودة غير مرسلة');
  page.once('dialog', dialog => dialog.accept());
  await page.getByRole('button', { name: 'English', exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`${job}$`));
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await expect(page.locator('#diacritized-text')).toHaveValue(text);
  await expect(source).toHaveValue('اللغة العربية جميلة');
  await page.reload();
  await expect(page.locator('#diacritized-text')).toHaveValue(text);
  await page.goto('/');
  await page.goBack();
  await expect(page.locator('#diacritized-text')).toHaveValue(text);
  await page.goForward();
  await expect(page).toHaveURL(/\/$/);
  expect(reservations).toBe(1);
});

test('an active transcription follows language reload to its job without its local file', async ({ page }) => {
  await page.goto('/transcribe');
  await page.locator('[name=file]').setInputFiles(speech);
  await page.locator('#upload-form [type=submit]').click();
  await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', /observing|processing|completed/);
  const job = await page.locator('.workspace-recovery').getAttribute('href');
  await expect(page.locator('.language-switch [name=back]')).toHaveValue(job);
  page.once('dialog', dialog => dialog.accept());
  await page.getByRole('button', { name: 'English', exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`${job}$`));
  await expect(page.locator('#job-status')).toHaveText('Transcription complete.');
  expect(await page.evaluate(() => window.arabiTools.sources.size)).toBe(0);
});

test('displayed diacritization stays copyable after downloads become unavailable', async ({ page, context, browserName }) => {
  await allowClipboard(context, browserName);
  await page.clock.install();
  await page.goto('/diacritize');
  await page.locator('#source-text').fill('اللغة العربية جميلة');
  await page.getByRole('button', { name: 'شكِّل النص', exact: true }).click();
  await expect(page.locator('.workspace-result .result')).toBeVisible();
  const text = await page.locator('#diacritized-text').inputValue();
  const job = await page.locator('.workspace-recovery').getAttribute('href');
  await expect(page.locator('.workspace-bridge')).toHaveAttribute('data-ack', '');
  await page.route(`**${job}/result`, route => route.fulfill({ status: 404 }));
  await page.clock.fastForward(31 * 60_000);
  expect(await page.evaluate(async job => (await fetch(`${job}/result`)).status, job)).toBe(404);
  await page.getByRole('button', { name: 'نسخ النص كاملًا', exact: true }).click();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(text);
  await expect(page.locator('#source-text')).toHaveValue('اللغة العربية جميلة');
  await expect(page.locator('#diacritized-text')).toHaveValue(text);
});

test('missing jobs keep the mounted source and expose the tool for a new attempt', async ({ page }) => {
  await page.goto('/diacritize');
  await page.locator('#source-text').fill('نص لا ينبغي أن يختفي');
  const id = '00000000000000000000000000000001';
  await page.route('**/diacritize/jobs', route => route.fulfill({ json: { job_id: id, next: `/diacritize/jobs/${id}`, text: `/diacritize/jobs/${id}/text` } }));
  await page.route('**/diacritize/jobs/*/text', route => route.fulfill({ json: { job_id: id, next: `/diacritize/jobs/${id}` } }));
  await page.getByRole('button', { name: 'شكِّل النص', exact: true }).click();
  await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'missing');
  await expect(page.locator('.submission-status')).toContainText('غير متاحة أو انتهت صلاحيتها');
  await expect(page.locator('#source-text')).toHaveValue('نص لا ينبغي أن يختفي');
  await expect(page.getByRole('button', { name: 'شكِّل النص', exact: true })).toBeEnabled();
  const response = await page.goto(`/diacritize/jobs/${id}`);
  expect(response.status()).toBe(404);
  await expect(page.locator('.notice')).toContainText('غير متاحة أو انتهت صلاحيتها');
  await expect(page.locator('main a[href="/diacritize"]')).toBeVisible();
});

test('cancellation before a reservation reply never uploads or loses the source', async ({ page }) => {
  await page.goto('/diacritize');
  await page.locator('#source-text').fill('اللغة العربية جميلة');
  const reservation = pending();
  let created = false;
  let uploads = 0;
  await page.route('**/diacritize/jobs', async route => {
    const response = await route.fetch();
    created = true;
    await reservation.promise;
    await route.fulfill({ response });
  });
  page.on('request', request => { if (/\/diacritize\/jobs\/[^/]+\/text$/.test(new URL(request.url()).pathname)) uploads++; });
  try {
    await page.getByRole('button', { name: 'شكِّل النص', exact: true }).click();
    await expect.poll(() => created).toBe(true);
    await page.getByRole('button', { name: 'إلغاء المهمة', exact: true }).click();
    await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'cancelling');
    reservation.release();
    await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'cancelled');
    await expect(page.locator('#source-text')).toHaveValue('اللغة العربية جميلة');
    await expect(page.getByRole('alert')).toBeHidden();
    expect(uploads).toBe(0);
  } finally { reservation.release(); }
});

test('clipboard fallback selects the full diacritized result without moving the page', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: () => Promise.reject(new Error('unavailable')) } });
  });
  await locale(page, 'diacritize', 'en');
  await page.locator('#source-text').fill('اللغة العربية جميلة');
  await page.getByRole('button', { name: 'Diacritize text', exact: true }).click();
  await expect(page.locator('.workspace-result .result')).toBeVisible();
  await page.getByRole('button', { name: 'Copy all text', exact: true }).scrollIntoViewIfNeeded();
  const scroll = await page.evaluate(() => window.scrollY);
  await page.getByRole('button', { name: 'Copy all text', exact: true }).click();
  await expect(page.locator('.copy-status')).toHaveText('Copying was unavailable. Select the text and copy it manually.');
  expect(await page.locator('#diacritized-text').evaluate(input => input.selectionStart === 0 && input.selectionEnd === input.value.length)).toBe(true);
  expect(await page.evaluate(saved => Math.abs(window.scrollY - saved), scroll)).toBeLessThanOrEqual(1);
});

test('a lost reservation response retries the same attempt without a second upload', async ({ page }) => {
  await page.goto('/transcribe');
  const attempts = [];
  let uploads = 0;
  await page.route('**/transcribe/jobs', async route => {
    attempts.push(new URLSearchParams(route.request().postData()).get('attempt'));
    if (attempts.length === 1) { await route.fetch(); await route.abort(); }
    else await route.continue();
  });
  page.on('request', request => { if (/\/transcribe\/jobs\/[^/]+\/audio$/.test(new URL(request.url()).pathname)) uploads++; });
  await page.locator('[name=file]').setInputFiles(speech);
  await page.locator('#upload-form [type=submit]').click();
  await expect(page.getByRole('alert')).toBeVisible();
  await expect(page.locator('[name=file]')).toBeEnabled();
  await page.locator('#upload-form [type=submit]').click();
  await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
  expect(attempts).toHaveLength(2);
  expect(attempts[0]).toBe(attempts[1]);
  expect(uploads).toBe(1);
});

test('accepted uploads with lost replies reconcile their existing job', async ({ page }) => {
  await page.goto('/ocr');
  let reservations = 0;
  page.on('request', request => { if (new URL(request.url()).pathname === '/ocr/jobs') reservations++; });
  await page.route('**/ocr/jobs/*/document', async route => {
    // WebKit's intercepted multipart body omits file bytes. Send the actual
    // fixture with the same context before dropping the browser's reply.
    const response = await page.request.post(route.request().url(), {
      multipart: { file: { name: 'reply-lost.png', mimeType: 'image/png', buffer: image } },
    });
    expect(response.status()).toBe(200);
    expect((await response.json()).job_id).toBe(new URL(route.request().url()).pathname.split('/')[3]);
    await route.abort();
  });
  await page.locator('[name=file]').setInputFiles({ name: 'reply-lost.png', mimeType: 'image/png', buffer: image });
  await page.locator('#upload-form [type=submit]').click();
  await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
  await expect(page.getByRole('alert')).toBeHidden();
  expect(reservations).toBe(1);
  const id = (await page.locator('.workspace-recovery').getAttribute('href')).split('/').pop();
  expect(await page.evaluate(id => window.arabiTools.sources.get(id)?.name, id)).toBe('reply-lost.png');
});

test('all tool controllers work with blocked browser storage', async ({ page }) => {
  await page.addInitScript(() => {
    for (const method of ['getItem', 'setItem', 'removeItem']) Storage.prototype[method] = () => { throw new Error('Storage unavailable'); };
  });
  for (const tool of ['diacritize', 'transcribe', 'ocr']) {
    await page.goto(`/${tool}`);
    if (tool === 'diacritize') await page.locator('#source-text').fill('اللغة العربية جميلة');
    else await page.locator('[name=file]').setInputFiles(tool === 'transcribe' ? speech : { name: 'page.png', mimeType: 'image/png', buffer: image });
    await page.locator('form .primary[type=submit]:visible').click();
    await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
  }
});

test('upload attempt storage is scoped to its own tool', async ({ page }) => {
  const foreign = 'abcdef0123456789abcdef0123456789';
  await page.addInitScript(value => sessionStorage.setItem('transcribe:attempt', value), foreign);
  await page.goto('/ocr');
  let used;
  await page.route('**/ocr/jobs', async route => {
    used = new URLSearchParams(route.request().postData()).get('attempt');
    await route.continue();
  });
  await page.locator('[name=file]').setInputFiles({ name: 'page.png', mimeType: 'image/png', buffer: image });
  await page.locator('#upload-form [type=submit]').click();
  await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
  expect(used).not.toBe(foreign);
  expect(await page.evaluate(() => sessionStorage.getItem('transcribe:attempt'))).toBe(foreign);
  expect(await page.evaluate(() => sessionStorage.getItem('ocr:attempt'))).toBeNull();
});


test('a missing job resolves uncertain cancellation while keeping the source and previous result', async ({ page }) => {
  await page.clock.install();
  await page.goto('/diacritize');
  await page.locator('#source-text').fill('اللغة العربية جميلة');
  await page.getByRole('button', { name: 'شكِّل النص', exact: true }).click();
  await expect(page.locator('.workspace-result .result')).toBeVisible();
  const previous = await page.locator('#diacritized-text').inputValue();
  const id = '00000000000000000000000000000002';
  const job = `/diacritize/jobs/${id}`;
  await page.route('**/diacritize/jobs', route => route.fulfill({ json: { job_id: id, next: job, text: `${job}/text`, cancel: `${job}/cancel` } }));
  await page.route(`**${job}/text`, route => route.fulfill({ json: { job_id: id, next: job } }));
  await page.route(`**${job}/cancel`, route => route.fulfill({ status: 503, json: {} }));
  let observations = 0;
  await page.route('**/diacritize/observe', route => { observations++; return route.abort(); });
  await page.getByRole('button', { name: 'شكِّل النص', exact: true }).click();
  await expect.poll(() => observations).toBeGreaterThan(0);
  await page.getByRole('button', { name: 'إلغاء المهمة', exact: true }).click();
  await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'cancel-unconfirmed');
  await page.clock.fastForward(46_000);
  await page.unroute('**/diacritize/observe');
  await page.locator('.retry-observation').click();
  await page.clock.runFor(500);
  await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'missing');
  await expect(page.locator('#source-text')).toHaveValue('اللغة العربية جميلة');
  await expect(page.locator('#diacritized-text')).toHaveValue(previous);
  await expect(page.getByRole('button', { name: 'شكِّل النص', exact: true })).toBeEnabled();
  await expect(page.getByRole('alert')).toBeHidden();
});
