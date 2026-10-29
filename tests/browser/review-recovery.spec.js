import { mkdir, readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';

const fixture = name => fileURLToPath(new URL(`../fixtures/${name}`, import.meta.url));
const pendingUploads = new Set();
const source = async tool => tool === 'transcribe'
  ? { name: 'speech.mp3', mimeType: 'audio/mpeg', buffer: await readFile(fixture('speech.mp3')) }
  : { name: 'ocr-review.pdf', mimeType: 'application/pdf', buffer: await readFile(fixture('ocr-review.pdf')) };

async function open(page, baseURL, tool, language) {
  await page.context().addCookies([{ name: 'lang', value: language, url: baseURL }]);
  await page.goto(`/${tool}`);
}

test.beforeEach(async ({ page }) => {
  page.on('pageerror', error => { throw error; });
});
test.afterEach(async ({ request }) => {
  // A failing pre-repair test must not leave a 30-minute reservation blocking
  // later cases. An invalid upload releases only an unused reservation.
  for (const upload of pendingUploads) {
    await request.post(upload, { multipart: { file: { name: 'cleanup.txt', mimeType: 'text/plain', buffer: Buffer.from('invalid') } } });
  }
  pendingUploads.clear();
});

for (const tool of ['transcribe', 'ocr']) {
  for (const failure of ['request', 'response', 'timeout']) {
    test(`${tool} retries recovery after a ${failure} failure`, async ({ page, baseURL }) => {
      await open(page, baseURL, tool, 'en');
      const file = await source(tool);
      let recoveries = 0;
      let receiver;
      const forwarded = [];
      if (failure === 'timeout') {
        // WebKit pauses its network timer inside Playwright interception. A real
        // HTTP receiver must accept the request and withhold the response instead.
        receiver = createServer((request, response) => {
          request.resume();
          const headers = { 'Access-Control-Allow-Origin': baseURL, 'Access-Control-Allow-Methods': 'POST, OPTIONS', 'Access-Control-Allow-Headers': 'Content-Type' };
          if (request.method === 'OPTIONS') { response.writeHead(204, headers); response.end(); return; }
          recoveries++;
          if (recoveries === 1) return;
          forwarded.push((async () => {
            const reply = await fetch(`${baseURL}${request.url}`, { method: 'POST' });
            response.writeHead(reply.status, { ...headers, 'Content-Type': 'application/json' });
            response.end(await reply.text());
          })());
        });
        await new Promise(resolve => receiver.listen(0, '127.0.0.1', resolve));
        await page.route(`**/${tool}/jobs`, async route => {
          const response = await route.fetch();
          const reply = await response.json();
          await route.fulfill({ response, json: { ...reply, recover: `http://127.0.0.1:${receiver.address().port}${reply.recover}` } });
        });
      } else {
        await page.route(`**/${tool}/jobs/*/recover`, async route => {
          recoveries++;
          if (recoveries === 1) {
            await expect(page.locator('.workspace-observer [data-phase=reserved]')).toHaveCount(1);
            if (failure === 'response') await route.fetch();
            await route.abort();
          } else await route.continue();
        });
      }
      const uploadPath = `**/${tool}/jobs/*/${tool === 'ocr' ? 'document' : 'audio'}`;
      await page.route(uploadPath, route => {
        pendingUploads.add(new URL(route.request().url()).pathname);
        return route.abort();
      });
      const input = page.locator('#upload-form [name=file]');
      const submit = page.locator('#upload-form [type=submit]');
      try {
        await input.setInputFiles(file);
        await submit.click();
        await expect.poll(() => recoveries, { timeout: 25_000 }).toBeGreaterThanOrEqual(2);
        await expect(input).toBeEnabled();
        await expect(submit).toHaveAttribute('aria-disabled', 'false');
        await page.unroute(uploadPath);
        await submit.click();
        await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
      } finally {
        if (receiver) {
          receiver.closeAllConnections();
          await new Promise(resolve => receiver.close(resolve));
          await Promise.all(forwarded);
        }
      }
    });
  }
  test(`${tool} delayed recovery preserves focus and the previous result's source`, async ({ page, baseURL }) => {
    await open(page, baseURL, tool, 'en');
    const file = await source(tool);
    const input = page.locator('#upload-form [name=file]');
    const submit = page.locator('#upload-form [type=submit]');
    await input.setInputFiles(file);
    await submit.click();
    await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
    const oldId = await page.locator('.workspace-result [data-result]').getAttribute('data-job-id');
    await page.evaluate(id => {
      window.reviewPreviousFile = window.arabiTools.sources.get(id);
      window.reviewPreviousResult = document.querySelector('.workspace-result [data-result]');
      window.reviewPreviousDownloads = [...window.reviewPreviousResult.querySelectorAll('a[download]')].map(link => link.href);
    }, oldId);
    const uploadPath = `**/${tool}/jobs/*/${tool === 'ocr' ? 'document' : 'audio'}`;
    await page.route(uploadPath, route => {
      pendingUploads.add(new URL(route.request().url()).pathname);
      return route.abort();
    });
    for (const link of ['.workspace-recovery', '.return-to-job']) {
      let release;
      const held = new Promise(resolve => { release = resolve; });
      const recoveryPath = `**/${tool}/jobs/*/recover`;
      await page.route(recoveryPath, async route => { await held; await route.continue(); });
      try {
        await input.setInputFiles({ ...file, name: `replacement-${file.name}` });
        await submit.click();
        await expect(page.locator('.workspace-observer [data-phase=reserved]')).toHaveCount(1);
        const focused = page.locator(link);
        await expect(focused).toBeVisible();
        await focused.focus();
        release();
        await expect(input).toBeEnabled();
        await expect(submit).toBeFocused();
        expect(await page.evaluate(id => {
          const result = document.querySelector('.workspace-result [data-result]');
          return result === window.reviewPreviousResult
            && window.arabiTools.sources.get(id) === window.reviewPreviousFile
            && JSON.stringify([...result.querySelectorAll('a[download]')].map(link => link.href)) === JSON.stringify(window.reviewPreviousDownloads);
        }, oldId)).toBe(true);
      } finally {
        release();
        await page.unroute(recoveryPath);
      }
    }
    await page.unroute(uploadPath);
    await submit.click();
    await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
    await expect(page.locator('.workspace-result [data-result]')).not.toHaveAttribute('data-job-id', oldId);
  });
  for (const language of ['ar', 'en']) {
    test(`${tool} recovers an upload that never reaches the application in ${language}`, async ({ page, baseURL }) => {
      await open(page, baseURL, tool, language);
      const file = await source(tool);
      const input = page.locator('#upload-form [name=file]');
      const submit = page.locator('#upload-form [type=submit]');
      const reservations = [];
      await page.route(`**/${tool}/jobs`, async route => {
        const response = await route.fetch();
        const reservation = await response.json();
        reservations.push(reservation);
        if (reservation.upload) pendingUploads.add(reservation.upload);
        await route.fulfill({ response });
      });
      let uploads = 0;
      const uploadPath = `**/${tool}/jobs/*/${tool === 'ocr' ? 'document' : 'audio'}`;
      await page.route(uploadPath, async route => {
        uploads++;
        // Do not forward the first upload: only its reservation exists on the server.
        if (uploads === 1) await route.abort('failed');
        else await route.continue();
      });
      await input.setInputFiles(file);
      await page.evaluate(() => {
        window.reviewInput = document.querySelector('[name=file]');
        window.reviewFile = window.reviewInput.files[0];
        window.reviewDocument = performance.timeOrigin;
      });
      await submit.click();
      await expect(page.getByRole('alert')).toBeVisible();
      await expect(input).toBeEnabled();
      await expect(submit).toHaveAttribute('aria-disabled', 'false');
      await expect(page.locator('.remove-file')).toBeEnabled();
      expect(reservations).toHaveLength(1);
      // Recovery revokes the old upload capability before enabling Retry. A late
      // request cannot fill it, even if it arrives after a new reservation starts.
      const late = await page.request.post(reservations[0].upload, { multipart: { file } });
      expect(late.status()).toBe(409);
      await submit.click();
      await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
      expect(reservations).toHaveLength(2);
      expect(uploads).toBe(2);
      expect(reservations[1].job_id).not.toBe(reservations[0].job_id);
      await expect(page.locator('.workspace-result [data-result]')).toHaveAttribute('data-job-id', reservations[1].job_id);
      expect(await page.evaluate(() => window.reviewInput === document.querySelector('[name=file]')
        && window.reviewFile === window.reviewInput.files[0]
        && window.reviewDocument === performance.timeOrigin)).toBe(true);
      await expect(page.getByRole('alert')).toBeHidden();
    });

    test(`${tool} observes an accepted upload after a lost response in ${language}`, async ({ page, baseURL }) => {
      await open(page, baseURL, tool, language);
      const file = await source(tool);
      let reservations = 0;
      let uploads = 0;
      page.on('request', request => {
        if (request.method() === 'POST' && new URL(request.url()).pathname === `/${tool}/jobs`) reservations++;
      });
      await page.route(`**/${tool}/jobs/*/${tool === 'ocr' ? 'document' : 'audio'}`, async route => {
        uploads++;
        // Send real bytes in WebKit too, then lose only the browser's response.
        const accepted = await page.request.post(route.request().url(), { multipart: { file } });
        expect(accepted.status()).toBe(200);
        await route.abort();
      });
      await page.locator('#upload-form [name=file]').setInputFiles(file);
      await page.locator('#upload-form [type=submit]').click();
      await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
      await expect(page.getByRole('alert')).toBeHidden();
      expect(reservations).toBe(1);
      expect(uploads).toBe(1);
    });

    test(`${tool} Clear returns keyboard focus to search in ${language}`, async ({ page, baseURL }) => {
      await open(page, baseURL, tool, language);
      await page.locator('#upload-form [name=file]').setInputFiles(await source(tool));
      await page.locator('#upload-form [type=submit]').click();
      await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
      const field = page.locator('[data-search-query]');
      const clear = page.locator('[data-search-clear]');
      await field.fill('ا');
      await expect(page.locator('[data-search-next]')).toBeEnabled();
      await field.press('Tab');
      await page.keyboard.press('Tab');
      await page.keyboard.press('Tab');
      await expect(clear).toBeFocused();
      await page.keyboard.press('Enter');
      await expect(field).toBeFocused();
      // ASCII uses real key events instead of Playwright's non-Latin insertText,
      // which can target the last editable field even after native focus is lost.
      await page.keyboard.type('next');
      await expect(field).toHaveValue('next');
    });
  }
}

const warnings = {
  transcribe: {
    ar: 'أبقِ هذه الصفحة مفتوحة حتى ينتهي التفريغ، فإغلاقها يلغيه.',
    en: 'Keep this page open until the transcription is done. Closing it cancels it.',
  },
  ocr: {
    ar: 'أبقِ هذه الصفحة مفتوحة حتى ينتهي استخراج النص، فإغلاقها يلغيه.',
    en: 'Keep this page open until text recognition finishes. Closing it cancels the job.',
  },
  diacritize: {
    ar: 'أبقِ هذه الصفحة مفتوحة حتى ينتهي التشكيل، فإغلاقها يلغي المهمة.',
    en: 'Keep this page open until diacritization finishes. Closing it cancels the job.',
  },
};

for (const tool of ['transcribe', 'ocr', 'diacritize']) {
  for (const language of ['ar', 'en']) {
    test(`${tool} shows the keep-open warning during queued and running work in ${language}`, async ({ page, baseURL }) => {
      await open(page, baseURL, tool, language);
      // Hold observation so fast fixture jobs cannot skip the queued/running UI.
      // Submission still uses the real application and its mounted form.
      await page.route(`**/${tool}/observe`, route => route.abort());
      if (tool === 'diacritize') {
        await page.locator('#source-text').fill('اللغة العربية جميلة.');
        await page.locator('#diacritize-form [type=submit][data-mode=text]').click();
      } else {
        await page.locator('#upload-form [name=file]').setInputFiles(await source(tool));
        await page.locator('#upload-form [type=submit]').click();
      }
      await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'observing');
      const jobPath = await page.locator('.workspace-recovery').getAttribute('href');
      const job = jobPath.split('/').pop();
      // Finish the real fixture job before injecting presentation states, so
      // the following case cannot inherit an active-job admission refusal.
      await expect.poll(async () => (await page.request.get(jobPath)).text(), { timeout: 60_000 }).toContain('data-phase="completed"');
      const warning = page.locator('.upload-progress .keep-open');
      await expect(warning).toBeHidden();
      for (const phase of ['queued', 'processing', 'incomplete', 'completed']) {
        await page.evaluate(({ job, phase }) => {
          const update = document.createElement('div');
          update.dataset.jobId = job;
          update.dataset.phase = phase === 'incomplete' ? 'completed' : phase;
          const status = document.createElement('p');
          status.className = 'status';
          status.textContent = phase;
          update.append(status);
          if (phase === 'completed') {
            const result = document.createElement('div');
            result.dataset.result = 'test';
            update.append(result);
          }
          document.querySelector('.workspace-observer').replaceChildren(update);
        }, { job, phase });
        await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', phase === 'incomplete' ? 'processing' : phase);
        if (phase === 'completed' || phase === 'incomplete') await expect(warning).toBeHidden();
        else {
          await expect(warning).toBeVisible();
          await expect(warning).toHaveText(warnings[tool][language]);
          // Check the same warning at a phone width without horizontal scrolling.
          await page.setViewportSize({ width: 320, height: 740 });
          await expect(warning).toBeVisible();
          expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
          if (phase === 'processing' && process.env.RECOVERY_SCREENSHOT_DIR) {
            await mkdir(process.env.RECOVERY_SCREENSHOT_DIR, { recursive: true });
            await page.screenshot({ path: join(process.env.RECOVERY_SCREENSHOT_DIR, `${tool}-${language}-processing.png`), fullPage: true });
          }
        }
      }
    });
  }
}
