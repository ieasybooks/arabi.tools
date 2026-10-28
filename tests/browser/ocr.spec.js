import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "@playwright/test";

function pdf(pages) {
  const objects = ["<< /Type /Catalog /Pages 2 0 R >>", `<< /Type /Pages /Kids [${Array.from({ length: pages }, (_, i) => `${i + 3} 0 R`).join(" ")}] /Count ${pages} >>`];
  for (let i = 0; i < pages; i++) objects.push("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 72 72] /Resources << >> >>");
  let output = "%PDF-1.4\n";
  const offsets = [];
  objects.forEach((object, i) => { offsets.push(output.length); output += `${i + 1} 0 obj\n${object}\nendobj\n`; });
  const xref = output.length;
  output += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  offsets.forEach((offset) => { output += `${String(offset).padStart(10, "0")} 00000 n \n`; });
  output += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(output);
}

const text = "مرحبا بالعالم\nهذا نص من صفحة كتاب.";
for (const locale of [
  { code: "ar", dir: "rtl", name: "استخراج النص من الصور", file: "ملف PDF أو صورة", submit: "ابدأ استخراج النص", done: "اكتمل استخراج النص.", downloads: ["تنزيل النص (TXT)", "تنزيل مستند Word (DOCX)", "تنزيل بيانات الصفحات (JSON)"], invalid: "نقبل ملف PDF واحدًا أو صورة JPG أو PNG واحدة فقط.", unreadable: "لم نتمكن من قراءة الملف.", pageCap: "الملف يتجاوز الحد الأقصى البالغ 1000 صفحة.", unavailable: "أداة استخراج النص غير متاحة مؤقتًا." },
  { code: "en", dir: "ltr", name: "Text recognition (OCR)", file: "PDF or image file", submit: "Extract text", done: "Text recognition complete.", downloads: ["Download text (.txt)", "Download Word document (.docx)", "Download pages (.json)"], invalid: "Choose exactly one PDF, JPG or PNG file.", unreadable: "We could not read this file.", pageCap: "The file exceeds the 1000-page limit.", unavailable: "Text recognition is temporarily unavailable." },
]) {
  async function open(page, url = "/ocr") {
    await page.goto(url);
    if (locale.code === "en") await page.getByRole("button", { name: "English", exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("lang", locale.code);
    await expect(page.locator("html")).toHaveAttribute("dir", locale.dir);
  }

  test(`OCR upload, live progress, preview and downloads in ${locale.code}`, async ({ page }) => {
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await open(page);
    await expect(page.getByRole("heading", { name: locale.name, exact: true })).toBeVisible();
    await expect(page.getByText(/Google Drive/)).toBeVisible();
    if (process.env.OCR_SCREENSHOT_DIR) await page.screenshot({ path: join(process.env.OCR_SCREENSHOT_DIR, `ocr-upload-${locale.code}.png`), fullPage: true });
    await page.evaluate(() => { window.__sentinel = performance.timeOrigin; window.__fileInput = document.querySelector("[name=file]"); });
    await page.getByLabel(locale.file, { exact: true }).setInputFiles({ name: "book.pdf", mimeType: "application/pdf", buffer: pdf(3) });
    await page.getByRole("button", { name: locale.submit, exact: true }).click();
    await expect(page.locator(".workspace-recovery")).toHaveAttribute("href", /\/ocr\/jobs\/[a-f0-9]{32}$/);
    await expect(page.getByRole("progressbar")).toBeVisible();
    await expect(page.locator(".submission-status")).toHaveText(locale.done, { timeout: 30_000 });
    expect(await page.evaluate(() => window.__sentinel === performance.timeOrigin && window.__fileInput === document.querySelector("[name=file]"))).toBe(true);
    await expect(page.locator("[data-ocr-position]")).toContainText("3");
    await expect(page.locator(".ocr-pages .ocr-page")).toHaveCount(1);
    await expect(page.locator("[data-ocr-page-text]").first()).toHaveText(text);
    if (process.env.OCR_SCREENSHOT_DIR) await page.screenshot({ path: join(process.env.OCR_SCREENSHOT_DIR, `ocr-result-${locale.code}.png`), fullPage: true });
    for (const [i, extension] of ["txt", "docx", "json"].entries()) {
      // Each download is listed in the Download menu, which closes after a choice.
      await page.locator(".download-menu summary").click();
      const link = page.getByRole("link", { name: locale.downloads[i], exact: true });
      const started = page.waitForEvent("download");
      await link.click();
      const download = await started;
      expect(download.suggestedFilename()).toBe(`book.${extension}`);
      expect(await download.failure()).toBeNull();
      const body = await readFile(await download.path());
      if (extension === "docx") expect(body.subarray(0, 2).toString()).toBe("PK");
      else if (extension === "json") expect(JSON.parse(body)).toEqual([1, 2, 3].map((page) => ({ page, content: text })));
      else expect(body.toString()).toBe([text, text, text].join("\n\nPAGE_SEPARATOR\n\n"));
    }
    if (process.env.OCR_SCREENSHOT_DIR) {
      await page.setViewportSize({ width: 390, height: 844 });
      await page.screenshot({ path: join(process.env.OCR_SCREENSHOT_DIR, `ocr-result-mobile-${locale.code}.png`), fullPage: true });
    }
    expect(errors).toEqual([]);
  });

  test(`OCR upload errors in ${locale.code}`, async ({ page }) => {
    await open(page);
    const file = page.getByLabel(locale.file, { exact: true });
    await file.setInputFiles({ name: "bad.gif", mimeType: "image/gif", buffer: Buffer.from("bad") });
    // An unsupported file is refused as it is chosen: the drop zone stays and nothing can be sent.
    await expect(page.getByRole("alert")).toHaveText(locale.invalid);
    await expect(page.locator(".file-row")).toBeHidden();
    await expect(page.locator("#upload-form [type=submit]")).toBeHidden();
    await expect(page.locator("#upload-form [type=submit]")).toBeDisabled();
    await file.setInputFiles({ name: "broken.pdf", mimeType: "application/pdf", buffer: Buffer.from("broken") });
    await page.getByRole("button", { name: locale.submit, exact: true }).click();
    await expect(page.getByRole("alert")).toContainText(locale.unreadable);
    await file.setInputFiles({ name: "too-many.pdf", mimeType: "application/pdf", buffer: pdf(1001) });
    await page.getByRole("button", { name: locale.submit, exact: true }).click();
    await expect(page.getByRole("alert")).toHaveText(locale.pageCap);
  });

  test(`OCR unavailable in ${locale.code}, transcription still available`, async ({ page }) => {
    await open(page, "http://127.0.0.1:3205/ocr");
    await expect(page.getByRole("status")).toContainText(locale.unavailable);
    await expect(page.locator("#upload-form")).toHaveCount(0);
    await page.goto("http://127.0.0.1:3205/transcribe");
    await expect(page.locator("#upload-form")).toBeVisible();
  });
}
