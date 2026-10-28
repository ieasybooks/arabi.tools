import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';

const fixture = name => fileURLToPath(new URL(`../fixtures/${name}`, import.meta.url));
const unusualName = "تقرير $& $' $`.gif";
const refused = (locale, name, formats) => locale === 'ar'
  ? `الملف \u2068${name}\u2069 ليس بصيغة ${formats.ar}.`
  : `\u2068${name}\u2069 is not ${formats.en} file.`;

async function open(page, tool, locale) {
  await page.context().addCookies([{ name: 'lang', value: locale, url: 'http://127.0.0.1:3204' }]);
  await page.goto(`/${tool}`);
}

async function finish(page, tool, locale) {
  await open(page, tool, locale);
  const extension = tool === 'ocr' ? 'pdf' : 'mp3';
  await page.locator('#upload-form [name=file]').setInputFiles({
    name: `original.${extension}`,
    mimeType: tool === 'ocr' ? 'application/pdf' : 'audio/mpeg',
    buffer: await readFile(fixture(tool === 'ocr' ? 'ocr-review.pdf' : 'speech.mp3')),
  });
  await page.locator('#upload-form [type=submit]').click();
  await expect(page.locator('[data-workspace]')).toHaveAttribute('data-phase', 'completed');
  await expect(page.locator('.workspace-result [data-result]')).toBeVisible();
}

for (const locale of ['ar', 'en']) {
  test(`rejected filenames stay literal in every tool in ${locale}`, async ({ page }) => {
    for (const [tool, formats] of [
      ['transcribe', { ar: 'MP3 أو M4A', en: 'an MP3 or M4A' }],
      ['ocr', { ar: 'PDF أو JPG أو PNG', en: 'a PDF, JPG or PNG' }],
      ['diacritize', { ar: 'TXT أو DOCX', en: 'a TXT or DOCX' }],
    ]) {
      await open(page, tool, locale);
      if (tool === 'diacritize') await page.getByRole('radio', { name: locale === 'ar' ? 'مستند' : 'Document', exact: true }).check();
      await page.locator('form [name=file]').setInputFiles({ name: unusualName, mimeType: 'image/gif', buffer: Buffer.from('GIF89a') });
      await expect(page.getByRole('alert')).toHaveText(refused(locale, unusualName, formats));
    }
  });

  test(`OCR source rejection keeps the literal filename in ${locale}`, async ({ page }) => {
    await finish(page, 'ocr', locale);
    await page.goto(await page.locator('.workspace-recovery').getAttribute('href'));
    const root = page.locator('[data-result="ocr"]');
    await expect(root).toHaveAttribute('data-enhanced', 'true');
    // A supported extension with unsupported content exercises the local preview's own check.
    const name = unusualName.replace('.gif', '.png');
    await root.locator('[data-ocr-source-file]').setInputFiles({ name, mimeType: 'image/png', buffer: Buffer.from('GIF89a') });
    await root.locator('[data-ocr-confirm]').check();
    await root.locator('[data-ocr-source-picker] button').click();
    await expect(root.locator('[data-ocr-source-error]')).toHaveText(refused(locale, name, { ar: 'PDF أو JPG أو PNG', en: 'a PDF, JPG or PNG' }));
  });

  for (const tool of ['transcribe', 'ocr']) {
    test(`${tool} keeps the result named when its input is changed or removed in ${locale}`, async ({ page }) => {
      await finish(page, tool, locale);
      const name = page.locator('.workspace-result .result-name');
      await expect(name).toBeHidden();
      const result = page.locator('.workspace-result [data-result]');
      const job = await result.getAttribute('data-job-id');
      const file = await page.locator('#upload-form [name=file]').evaluate(input => ({ name: input.files[0].name, type: input.files[0].type }));
      await page.locator('#upload-form [name=file]').setInputFiles({
        name: `next-${file.name}`, mimeType: file.type,
        buffer: await readFile(fixture(tool === 'ocr' ? 'ocr-review.pdf' : 'speech.mp3')),
      });
      await expect(name).toBeVisible();
      await expect(name).toHaveText('original');
      await expect(result).toHaveAttribute('data-job-id', job);
      await page.locator('.remove-file').click();
      await expect(name).toBeVisible();
      await expect(name).toHaveText('original');
      await expect(result).toHaveAttribute('data-job-id', job);
    });
  }
}
