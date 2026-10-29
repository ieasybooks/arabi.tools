import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';

const address = 'http://127.0.0.1:3204';
const fixture = name => fileURLToPath(new URL(`../fixtures/${name}`, import.meta.url));

test.beforeEach(async ({ page }) => {
  page.on('pageerror', error => { throw error; });
});

for (const language of ['ar', 'en']) {
  test(`completion preserves the focused recovery link until focus leaves in ${language}`, async ({ page }) => {
    await page.context().addCookies([{ name: 'lang', value: language, url: address }]);
    await page.goto('/transcribe');
    let release;
    const upload = new Promise(resolve => { release = resolve; });
    await page.route('**/transcribe/jobs/*/audio', async route => {
      await upload;
      await route.continue();
    });
    try {
      await page.locator('[name=file]').setInputFiles(fixture('speech.mp3'));
      await page.locator('#upload-form [type=submit]').click();
      const recovery = page.locator('.workspace-recovery');
      await expect(recovery).toBeVisible();
      await recovery.focus();
      await expect(recovery).toBeFocused();
      release();
      await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
      await expect(recovery).toBeFocused();
      await expect(recovery).toBeVisible();
      expect(await page.locator('.upload-progress').evaluate(node => node.getBoundingClientRect().height)).toBeGreaterThan(10);
      await page.keyboard.press('Tab');
      await expect(recovery).not.toBeFocused();
      await expect(recovery).toBeHidden();
    } finally {
      release();
    }
  });

  test(`direct diacritization results keep their actions beside the output without JavaScript in ${language}`, async ({ page, browser }) => {
    await page.context().addCookies([{ name: 'lang', value: language, url: address }]);
    await page.goto('/diacritize');
    await page.locator('#source-text').fill('اللغة العربية جميلة، وهذا نص للتشكيل.');
    await page.locator('#diacritize-form [type=submit]:visible').click();
    await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
    const output = await page.locator('#diacritized-text').inputValue();
    const job = await page.locator('.workspace-recovery').getAttribute('href');
    const context = await browser.newContext({ javaScriptEnabled: false });
    try {
      await context.addCookies([{ name: 'lang', value: language, url: address }]);
      const fallback = await context.newPage();
      await fallback.goto(`${address}${job}`);
      await expect(fallback.locator('#diacritized-text')).toHaveValue(output);
      for (const width of [1280, 320]) {
        await fallback.setViewportSize({ width, height: 900 });
        await fallback.evaluate(() => document.fonts.ready);
        const layout = await fallback.locator('.diacritize-result').evaluate(node => {
          const result = node.getBoundingClientRect();
          const head = node.querySelector('.diacritize-output-head').getBoundingClientRect();
          const text = node.querySelector('#diacritized-text').getBoundingClientRect();
          return { resultTop: result.top, headTop: head.top, headBottom: head.bottom, textTop: text.top };
        });
        expect(layout.headTop).toBeGreaterThanOrEqual(layout.resultTop);
        expect(layout.headBottom).toBeLessThanOrEqual(layout.textTop);
        expect(await fallback.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      }
      const downloading = fallback.waitForEvent('download');
      await fallback.locator('.diacritize-result a[download]').click();
      const download = await downloading;
      expect(download.suggestedFilename()).toBe('text.txt');
      expect((await readFile(await download.path())).toString()).toBe(output);
    } finally {
      await context.close();
    }
  });

  test(`direct OCR results keep visible page headings without JavaScript in ${language}`, async ({ page, browser }) => {
    await page.context().addCookies([{ name: 'lang', value: language, url: address }]);
    await page.goto('/ocr');
    await page.locator('[name=file]').setInputFiles(fixture('ocr-review.pdf'));
    await page.locator('#upload-form [type=submit]').click();
    await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
    const job = await page.locator('.workspace-recovery').getAttribute('href');
    const context = await browser.newContext({ javaScriptEnabled: false });
    try {
      await context.addCookies([{ name: 'lang', value: language, url: address }]);
      const fallback = await context.newPage();
      await fallback.goto(`${address}${job}`);
      const root = fallback.locator('[data-result=ocr]');
      await expect(root.locator('[data-ocr-controls]')).toBeHidden();
      await expect(root.locator('.ocr-page h3')).toHaveText([1, 2, 3].map(number => `${language === 'ar' ? 'الصفحة' : 'Page'} ${number}`));
      for (const width of [1280, 320]) {
        await fallback.setViewportSize({ width, height: 900 });
        await fallback.evaluate(() => document.fonts.ready);
        for (const heading of await root.locator('.ocr-page h3').all()) {
          const box = await heading.boundingBox();
          expect(box.width).toBeGreaterThan(20);
          expect(box.height).toBeGreaterThan(10);
        }
        await expect(root.locator('.ocr-text-panel .ocr-panel-title')).toBeVisible();
        expect(await fallback.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      }
      await expect(root.locator('.ocr-page').nth(1).locator('.ocr-no-text')).toBeVisible();
      await root.locator('.download-menu > summary').click();
      await expect(root.locator('a[download]')).toHaveCount(3);
      for (const link of await root.locator('a[download]').all()) await expect(link).toBeVisible();
    } finally {
      await context.close();
    }
  });
}
