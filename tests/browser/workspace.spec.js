import { expect, test } from '@playwright/test';

test('diacritization proof keeps the document and source through two jobs', async ({ page }) => {
  await page.goto('/diacritize');
  await page.getByRole('button', { name: 'English', exact: true }).click();
  const source = page.locator('#source-text');
  await source.fill('  اللغة العربية جميلة.\n'.repeat(20));
  await page.evaluate(() => {
    window.__source = document.querySelector('#source-text');
    window.__sentinel = performance.timeOrigin;
    window.__source.setSelectionRange(15, 30);
    window.__source.scrollTop = 45;
    document.querySelector('#diacritize-form').addEventListener('submit', () => {
      window.__scroll = window.scrollY;
      window.__focus = document.activeElement;
    }, { capture: true });
  });
  let jobs = 0;
  page.on('request', request => {
    if (request.method() === 'POST' && new URL(request.url()).pathname === '/diacritize/jobs') jobs++;
  });
  for (const model of ['shakkelha', 'catt']) {
    await page.locator(`input[name=model][value=${model}]`).check();
    await page.getByRole('button', { name: 'Diacritize text', exact: true }).click();
    await expect(page.locator('.workspace-result [data-result]')).toHaveAttribute('data-model', model === 'catt' ? 'CATT' : 'شكِّلها', { timeout: 60_000 });
    expect(await page.evaluate(() => ({
      same: window.__source === document.querySelector('#source-text'),
      document: window.__sentinel === performance.timeOrigin,
      selection: [window.__source.selectionStart, window.__source.selectionEnd],
      scroll: window.__source.scrollTop,
    }))).toEqual({ same: true, document: true, selection: [15, 30], scroll: 45 });
    await expect(page).toHaveURL(/\/diacritize$/);
    expect(await page.evaluate(() => Math.abs(window.scrollY - window.__scroll))).toBeLessThanOrEqual(1);
    expect(await page.evaluate(() => document.activeElement === window.__focus)).toBe(true);
  }
  expect(jobs).toBe(2);
});

test('diacritization proof reconciles a submission before hydration', async ({ page }) => {
  let release;
  const held = new Promise(resolve => { release = resolve; });
  await page.route(/\/topcoat-[^/]+\.js$/, async route => { await held; await route.continue(); });
  await page.goto('/diacritize', { waitUntil: 'commit' });
  await page.locator('#source-text').fill('اللغة العربية جميلة');
  await page.evaluate(() => { window.__sentinel = performance.timeOrigin; });
  await page.getByRole('button', { name: 'شكِّل النص', exact: true }).click();
  await expect(page.locator('.workspace-recovery')).toHaveAttribute('href', /\/diacritize\/jobs\/[a-f0-9]{32}$/);
  await expect(page.locator('.workspace-bridge')).toHaveAttribute('data-ack', '');
  release();
  await expect(page.locator('.workspace-result [data-result]')).toBeVisible({ timeout: 60_000 });
  expect(await page.evaluate(() => window.__sentinel === performance.timeOrigin)).toBe(true);
  await expect(page).toHaveURL(/\/diacritize$/);
});

test('diacritization proof keeps previous output through cancellation of the next job', async ({ page }) => {
  await page.goto('/diacritize');
  await page.locator('#source-text').fill('اللغة العربية جميلة');
  await page.getByRole('button', { name: 'شكِّل النص', exact: true }).click();
  await expect(page.locator('.workspace-result [data-result]')).toBeVisible({ timeout: 60_000 });
  const previous = await page.locator('#diacritized-text').inputValue();
  const previousJob = await page.locator('.workspace-recovery').getAttribute('href');
  let release;
  const held = new Promise(resolve => { release = resolve; });
  let reserved;
  await page.route('**/diacritize/jobs/*/text', async route => {
    reserved = route.request().url().replace(/\/text$/, '');
    await held;
    await route.abort().catch(() => {});
  });
  await page.getByRole('button', { name: 'شكِّل النص', exact: true }).click();
  await expect.poll(() => reserved).toBeTruthy();
  expect(new URL(reserved).pathname).not.toBe(previousJob);
  expect((await page.request.post(`${previousJob}/cancel`)).status()).toBe(404);
  await expect(page.locator('.previous-result')).toContainText('شكِّلها');
  await page.getByRole('button', { name: 'إلغاء المهمة', exact: true }).click();
  await expect(page.locator('.submission-status')).toHaveText('أُلغيت المهمة بطلبك.');
  await expect(page.locator('#diacritized-text')).toHaveValue(previous);
  await expect(page.locator('#source-text')).toHaveValue('اللغة العربية جميلة');
  release();
});


test('diacritization proof exposes delayed and incomplete rendering, ignores stale jobs and recovers', async ({ page }) => {
  await page.clock.install();
  await page.goto('/diacritize');
  await page.locator('#source-text').fill('اللغة العربية جميلة');
  await page.getByRole('button', { name: 'شكِّل النص', exact: true }).click();
  const output = page.locator('.workspace-result .result');
  await expect(output).toBeVisible();
  const oldId = await output.getAttribute('data-job-id');
  const oldText = await page.locator('#diacritized-text').inputValue();
  let observations = 0;
  await page.route('**/diacritize/observe', route => { observations++; return route.abort(); });
  await page.getByRole('radio', { name: 'CATT', exact: true }).check();
  await page.getByRole('button', { name: 'شكِّل النص', exact: true }).click();
  await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'observing');
  await expect.poll(() => observations).toBeGreaterThan(0);
  const job = (await page.locator('.workspace-recovery').getAttribute('href')).split('/').pop();
  expect(job).not.toBe(oldId);
  await page.evaluate(({ oldId, job }) => {
    const observer = document.querySelector('.workspace-observer');
    const stale = document.createElement('div');
    stale.dataset.phase = 'completed';
    stale.dataset.jobId = oldId;
    const result = document.createElement('div');
    result.className = 'result';
    result.textContent = 'stale';
    stale.append(result);
    stale.dataset.testUpdate = 'true';
    observer.append(stale);
    window.__currentJob = job;
  }, { oldId, job });
  await expect(page.locator('#diacritized-text')).toHaveValue(oldText);
  await page.evaluate(() => {
    const incomplete = document.createElement('div');
    incomplete.dataset.phase = 'completed';
    incomplete.dataset.jobId = window.__currentJob;
    document.querySelector('[data-test-update]').remove();
    incomplete.dataset.testUpdate = 'true';
    document.querySelector('.workspace-observer').append(incomplete);
  });
  await page.clock.fastForward(46_000);
  await expect(page.locator('.observation-status')).toBeVisible();
  await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'observing');
  await expect(page.locator('.workspace-recovery')).toHaveAttribute('href', `/diacritize/jobs/${job}`);
  await page.evaluate(() => document.querySelector('[data-test-update]').remove());
  await page.unroute('**/diacritize/observe');
  await page.locator('.retry-observation').click();
  await page.clock.runFor(500);
  await expect(output).toHaveAttribute('data-job-id', job);
  await expect(output).toHaveAttribute('data-model', 'CATT');
  await expect(page.locator('.observation-status')).toBeHidden();
  await expect(page.locator('#source-text')).toHaveValue('اللغة العربية جميلة');
});

test('diacritization proof reconnects the same running job without another submission', async ({ page }) => {
  await page.addInitScript(() => {
    const NativeSocket = window.WebSocket;
    window.__sockets = [];
    window.WebSocket = class extends NativeSocket {
      constructor(...args) { super(...args); window.__sockets.push(this); }
    };
  });
  let reservations = 0;
  page.on('request', request => {
    if (request.method() === 'POST' && new URL(request.url()).pathname === '/diacritize/jobs') reservations++;
  });
  await page.goto('/diacritize');
  await page.getByRole('radio', { name: 'مستند', exact: true }).check();
  await page.locator('[name=file]').setInputFiles({ name: 'reconnect.txt', mimeType: 'text/plain', buffer: Buffer.from('اللغة العربية جميلة.\n'.repeat(1000)) });
  await page.getByRole('button', { name: 'شكِّل المستند', exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__sockets.some(socket => socket.readyState === 1))).toBe(true);
  const job = await page.locator('.workspace-recovery').getAttribute('href');
  await page.evaluate(() => { for (const socket of window.__sockets) socket.close(); });
  await expect(page.locator('.workspace-result .result')).toBeVisible({ timeout: 60_000 });
  await expect(page.locator('.workspace-recovery')).toHaveAttribute('href', job);
  expect(reservations).toBe(1);
  expect(await page.evaluate(() => window.__sockets.length)).toBeGreaterThanOrEqual(2);
});
