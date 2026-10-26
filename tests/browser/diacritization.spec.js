import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "@playwright/test";

const source = "\n  اللُّغة العربية جميلة، API 2026.\nهذا نص للتشكيل.  ";
const stripMarks = (text) => text.replace(/[\u064b-\u0652]/gu, "");

// A real Word package with a bold run, a second run, and an untouched styles part.
const docx = execFileSync("python3", ["-c", `
import io, sys, zipfile
files = {
    '[Content_Types].xml': '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>',
    '_rels/.rels': '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
    'word/document.xml': '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">  اللغة العربية </w:t></w:r><w:r><w:t>جميلة، API 2026.</w:t></w:r></w:p></w:body></w:document>',
    'word/styles.xml': '<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Normal"/></w:styles>',
}
output = io.BytesIO()
with zipfile.ZipFile(output, 'w') as archive:
    for name, content in files.items(): archive.writestr(name, content)
sys.stdout.buffer.write(output.getvalue())
`]);

function docxPart(bytes, name) {
  return execFileSync("python3", ["-c", "import io, sys, zipfile; sys.stdout.buffer.write(zipfile.ZipFile(io.BytesIO(sys.stdin.buffer.read())).read(sys.argv[1]))", name], { input: bytes }).toString();
}

async function download(page, label) {
  const started = page.waitForEvent("download");
  await page.getByRole("link", { name: label, exact: true }).click();
  const result = await started;
  expect(await result.failure()).toBeNull();
  return { name: result.suggestedFilename(), bytes: await readFile(await result.path()) };
}

async function fitsViewport(page) {
  const dimensions = await page.evaluate(() => ({ width: document.documentElement.clientWidth, scroll: document.documentElement.scrollWidth }));
  expect(dimensions.scroll).toBeLessThanOrEqual(dimensions.width);
}

for (const locale of [
  {
    code: "ar", dir: "rtl", name: "تشكيل النص العربي", text: "نص", document: "مستند",
    original: "النص الأصلي", result: "النص المشكول", submit: "شكِّل النص", upload: "شكِّل المستند",
    file: "ملف TXT أو DOCX", done: "اكتمل التشكيل.", copy: "نسخ النص", copied: "نُسخ النص.",
    txt: "تنزيل النص (TXT)", docx: "تنزيل مستند Word (DOCX)", originalFile: "تنزيل الملف الأصلي",
    limit: "أدخل نصًا لا يتجاوز 1000 حرفًا، بما فيها الحركات والمسافات.",
    wrongType: "نقبل ملف TXT أو DOCX واحدًا فقط. ملفات DOC غير مدعومة.",
    invalidUtf8: "تعذّر قراءة ترميز النص. احفظ ملف TXT بترميز UTF-8 ثم حاول مرة أخرى.",
    invalidDocx: "تعذّر قراءة مستند Word. تأكد أنه ملف DOCX سليم وغير محمي بكلمة مرور.",
    tooLarge: "حجم الملف أكبر من 99 ميغابايت.",
    cancel: "إلغاء المهمة", cancelled: "أُلغيت المهمة بطلبك.", back: "العودة إلى التشكيل",
    unavailable: "أداة التشكيل غير متاحة مؤقتًا. حاول مرة أخرى لاحقًا.",
  },
  {
    code: "en", dir: "ltr", name: "Arabic diacritization", text: "Text", document: "Document",
    original: "Original text", result: "Diacritized text", submit: "Diacritize text", upload: "Diacritize document",
    file: "TXT or DOCX file", done: "Diacritization complete.", copy: "Copy text", copied: "Text copied.",
    txt: "Download text (.txt)", docx: "Download Word document (.docx)", originalFile: "Download original file",
    limit: "Enter up to 1000 characters, including diacritics and spaces.",
    wrongType: "Choose exactly one TXT or DOCX file. DOC files are not supported.",
    invalidUtf8: "We could not read the text encoding. Save the TXT file as UTF-8 and try again.",
    invalidDocx: "We could not read this Word document. Check that it is a valid DOCX file and is not password protected.",
    tooLarge: "The file is larger than 99 MB.",
    cancel: "Cancel job", cancelled: "You cancelled this job.", back: "Back to your diacritization",
    unavailable: "Arabic diacritization is temporarily unavailable. Please try again later.",
  },
]) {
  async function open(page, url = "/diacritize") {
    await page.goto(url);
    if (locale.code === "en") await page.getByRole("button", { name: "English", exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("lang", locale.code);
    await expect(page.locator("html")).toHaveAttribute("dir", locale.dir);
  }

  test(`Diacritization retains input, copies, downloads and retries both models in ${locale.code}`, async ({ page, context }) => {
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);
    await open(page, "/");
    await fitsViewport(page);
    if (process.env.DIACRITIZATION_SCREENSHOT_DIR) {
      await page.screenshot({ path: join(process.env.DIACRITIZATION_SCREENSHOT_DIR, `catalog-${locale.code}.png`), fullPage: true });
    }
    await page.setViewportSize({ width: 390, height: 844 });
    await fitsViewport(page);
    if (process.env.DIACRITIZATION_SCREENSHOT_DIR) {
      await page.screenshot({ path: join(process.env.DIACRITIZATION_SCREENSHOT_DIR, `catalog-mobile-${locale.code}.png`), fullPage: true });
    }
    await page.locator('a[href="/diacritize"]').click();
    await expect(page.getByRole("heading", { name: locale.name, exact: true })).toBeVisible();
    await fitsViewport(page);
    if (process.env.DIACRITIZATION_SCREENSHOT_DIR) {
      await page.screenshot({ path: join(process.env.DIACRITIZATION_SCREENSHOT_DIR, `diacritization-input-mobile-${locale.code}.png`), fullPage: true });
    }
    await page.setViewportSize({ width: 1280, height: 900 });
    if (process.env.DIACRITIZATION_SCREENSHOT_DIR) {
      await page.screenshot({ path: join(process.env.DIACRITIZATION_SCREENSHOT_DIR, `diacritization-input-${locale.code}.png`), fullPage: true });
    }
    await expect(page.getByRole("radio", { name: "شكِّلها", exact: true })).toBeChecked();
    await expect(page.locator('bdi[lang="en"]')).toHaveText("CATT");
    await expect(page.locator('bdi[lang="en"]')).toHaveAttribute("dir", "ltr");
    await page.getByLabel(locale.original, { exact: true }).fill(source);
    for (const model of ["شكِّلها", "CATT"]) {
      const previousUrl = page.url();
      await page.getByRole("radio", { name: model, exact: true }).check();
      await page.getByRole("button", { name: locale.submit, exact: true }).click();
      await page.waitForURL((url) => /\/diacritize\/jobs\/[a-f0-9]{32}$/.test(url.pathname) && url.href !== previousUrl);
      await expect(page.locator("#job-status")).toHaveText(locale.done, { timeout: 60_000 });
      const output = await page.getByLabel(locale.result, { exact: true }).inputValue();
      expect(stripMarks(output)).toBe(stripMarks(source));
      expect(output).toMatch(/[\u064b-\u0652]/u);
      await expect(page.getByLabel(locale.original, { exact: true })).toHaveValue(source);
      await expect(page.getByRole("radio", { name: model, exact: true })).toBeChecked();
      await page.getByRole("button", { name: locale.copy, exact: true }).click();
      await expect(page.locator(".copy-status")).toHaveText(locale.copied);
      expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(output);
      const result = await download(page, locale.txt);
      expect(result.name).toBe("text.txt");
      expect(result.bytes.toString()).toBe(output);
      await expect(page.locator('meta[name="robots"]')).toHaveAttribute("content", /noindex/);
      await fitsViewport(page);
      // A saved result retains its own model and text mode even after this tab
      // chooses different defaults on the tool page.
      const completedUrl = page.url();
      await page.goto("/diacritize");
      await page.getByRole("radio", { name: model === "CATT" ? "شكِّلها" : "CATT", exact: true }).check();
      await page.getByRole("radio", { name: locale.document, exact: true }).check();
      await page.goto(completedUrl);
      await expect(page.getByLabel(locale.original, { exact: true })).toBeVisible();
      await expect(page.getByLabel(locale.original, { exact: true })).toHaveValue(source);
      await expect(page.getByRole("radio", { name: model, exact: true })).toBeChecked();
    }
    await page.setViewportSize({ width: 390, height: 844 });
    await fitsViewport(page);
    await expect(page.getByLabel(locale.original, { exact: true })).toBeVisible();
    if (process.env.DIACRITIZATION_SCREENSHOT_DIR) {
      await page.screenshot({ path: join(process.env.DIACRITIZATION_SCREENSHOT_DIR, `diacritization-result-mobile-${locale.code}.png`), fullPage: true });
      await page.setViewportSize({ width: 1280, height: 900 });
      await page.screenshot({ path: join(process.env.DIACRITIZATION_SCREENSHOT_DIR, `diacritization-result-${locale.code}.png`), fullPage: true });
    }
    expect(errors).toEqual([]);
  });

  test(`Diacritization keeps source text out of URLs without JavaScript in ${locale.code}`, async ({ browser }) => {
    const context = await browser.newContext({ javaScriptEnabled: false });
    try {
      const address = "http://127.0.0.1:3204";
      await context.addCookies([{ name: "lang", value: locale.code, url: address }]);
      const page = await context.newPage();
      await page.goto(`${address}/diacritize`);
      await page.getByLabel(locale.original, { exact: true }).fill(source);
      const [request] = await Promise.all([
        page.waitForRequest((request) => request.isNavigationRequest()),
        page.getByRole("button", { name: locale.submit, exact: true }).click(),
      ]);
      expect(request.method()).toBe("POST");
      expect(new URL(request.url()).search).toBe("");
    } finally {
      await context.close();
    }
  });

  test(`Diacritization counts Unicode scalars and accepts exactly 1000 in ${locale.code}`, async ({ page }) => {
    await open(page);
    const input = page.getByLabel(locale.original, { exact: true });
    expect(await input.getAttribute("maxlength")).toBeNull();
    await input.fill("🙂".repeat(1001));
    await expect(page.locator("#text-count")).toContainText("1001 / 1000");
    await expect(page.getByRole("alert")).toHaveText(locale.limit);
    await page.getByRole("button", { name: locale.submit, exact: true }).click();
    await expect(page).toHaveURL(/\/diacritize$/);
    const boundary = `${"🙂".repeat(999)}ا`;
    await input.fill(boundary);
    await expect(page.locator("#text-count")).toContainText("1000 / 1000");
    await expect(page.getByRole("alert")).toBeHidden();
    await page.getByRole("button", { name: locale.submit, exact: true }).click();
    await page.waitForURL(/\/diacritize\/jobs\/[a-f0-9]{32}$/);
    await expect(page.locator("#job-status")).toHaveText(locale.done, { timeout: 60_000 });
    await expect(page.getByLabel(locale.original, { exact: true })).toHaveValue(boundary);
  });

  test(`Diacritization keeps TXT and DOCX formats and model choice in ${locale.code}`, async ({ page }) => {
    const txt = Buffer.from("\ufeff  اللغة العربية جميلة، API 2026.\r\nهذا نص.\t ");
    for (const [extension, bytes, model] of [["txt", txt, "شكِّلها"], ["docx", docx, "CATT"]]) {
      await page.goto("/diacritize");
      if (locale.code === "en" && await page.locator("html").getAttribute("lang") !== "en") {
        await page.getByRole("button", { name: "English", exact: true }).click();
      }
      await page.getByRole("radio", { name: model, exact: true }).check();
      await page.getByRole("radio", { name: locale.document, exact: true }).check();
      await expect(page.getByRole("radio", { name: model, exact: true })).toBeChecked();
      await page.getByRole("radio", { name: locale.text, exact: true }).check();
      await expect(page.getByRole("radio", { name: model, exact: true })).toBeChecked();
      await page.getByRole("radio", { name: locale.document, exact: true }).check();
      await page.getByLabel(locale.file, { exact: true }).setInputFiles({ name: `مستند.${extension}`, mimeType: extension === "txt" ? "text/plain" : "application/vnd.openxmlformats-officedocument.wordprocessingml.document", buffer: bytes });
      await page.getByRole("button", { name: locale.upload, exact: true }).click();
      await page.waitForURL(/\/diacritize\/jobs\/[a-f0-9]{32}$/);
      await expect(page.locator("#job-status")).toHaveText(locale.done, { timeout: 60_000 });
      await expect(page.locator(".result-heading bdi")).toHaveText(model);
      const result = await download(page, locale[extension]);
      expect(result.name).toBe(`مستند.${extension}`);
      if (extension === "txt") {
        expect(stripMarks(result.bytes.toString())).toBe(txt.toString());
        expect(result.bytes.toString()).toMatch(/[\u064b-\u0652]/u);
      } else {
        const before = docxPart(docx, "word/document.xml");
        const after = docxPart(result.bytes, "word/document.xml");
        expect(after).not.toBe(before);
        expect(stripMarks(after)).toBe(before);
        expect(docxPart(result.bytes, "word/styles.xml")).toBe(docxPart(docx, "word/styles.xml"));
      }
      const original = await download(page, locale.originalFile);
      expect(original.bytes).toEqual(bytes);
      await expect(page.locator("#diacritized-text")).toHaveCount(0);
      await fitsViewport(page);
    }
  });

  test(`Diacritization rejects DOC, oversized files and invalid documents in ${locale.code}`, async ({ page }) => {
    await open(page);
    await page.getByRole("radio", { name: locale.document, exact: true }).check();
    const input = page.getByLabel(locale.file, { exact: true });
    await input.setInputFiles({ name: "legacy.doc", mimeType: "application/msword", buffer: Buffer.from("legacy") });
    await expect(page.getByRole("alert")).toHaveText(locale.wrongType);
    await input.setInputFiles({ name: "large.txt", mimeType: "text/plain", buffer: Buffer.from("test") });
    // Exercise the browser's boundary without allocating a 99 MB fixture for this UI check.
    await input.evaluate((element) => Object.defineProperty(element.files[0], "size", { value: 99_000_001 }));
    await page.getByRole("button", { name: locale.upload, exact: true }).click();
    await expect(page.getByRole("alert")).toHaveText(locale.tooLarge);
    for (const [name, bytes, message] of [["invalid.txt", Buffer.from([0xff]), locale.invalidUtf8], ["invalid.docx", Buffer.from("broken"), locale.invalidDocx]]) {
      await input.setInputFiles({ name, mimeType: "application/octet-stream", buffer: bytes });
      await page.getByRole("button", { name: locale.upload, exact: true }).click();
      await expect(page.getByRole("alert")).toHaveText(message);
      await expect(input).toBeEnabled();
    }
  });

  test(`Diacritization cancels uploads without exposing another browser's job in ${locale.code}`, async ({ page, browser, context }) => {
    await open(page);
    await page.getByLabel(locale.original, { exact: true }).fill(source);
    await page.getByRole("radio", { name: locale.document, exact: true }).check();
    await page.getByLabel(locale.file, { exact: true }).setInputFiles({ name: "cancel.txt", mimeType: "text/plain", buffer: Buffer.from(source) });
    let releaseUpload;
    const held = new Promise((resolve) => { releaseUpload = resolve; });
    let uploadUrl;
    await page.route("**/diacritize/jobs/*/document", async (route) => {
      uploadUrl = route.request().url();
      await held;
      await route.abort().catch(() => {});
    });
    await page.getByRole("button", { name: locale.upload, exact: true }).click();
    await expect.poll(() => uploadUrl).toBeTruthy();
    await expect(page.locator(".upload-progress progress")).toBeVisible();
    await expect(page.getByLabel(locale.file, { exact: true })).toBeDisabled();
    const jobCookie = (await context.cookies()).find((cookie) => cookie.name === "diacritize_job");
    expect(jobCookie.httpOnly).toBe(true);
    expect(jobCookie.sameSite).toBe("Lax");
    const outsider = await browser.newContext();
    try {
      const other = await outsider.newPage();
      await other.goto(new URL("/diacritize", uploadUrl).href);
      await expect(other.getByRole("link", { name: /العودة إلى التشكيل|Back to your diacritization/ })).toHaveCount(0);
      const denied = await outsider.request.post(uploadUrl.replace(/\/document$/, "/cancel"));
      expect(denied.status()).toBe(404);
      const forged = await outsider.request.post(uploadUrl.replace(/\/document$/, "/cancel"), {
        headers: { Cookie: `diacritize_job=${new URL(uploadUrl).pathname.split("/")[3]}` },
      });
      expect(forged.status()).toBe(404);
      await page.getByRole("button", { name: locale.cancel, exact: true }).click();
      await expect(page.locator(".submission-status")).toHaveText(locale.cancelled);
      await expect(page.getByLabel(locale.file, { exact: true })).toBeEnabled();
      await page.getByRole("radio", { name: locale.text, exact: true }).check();
      await expect(page.getByLabel(locale.original, { exact: true })).toHaveValue(source);
    } finally {
      releaseUpload();
      await outsider.close();
    }
  });

  test(`Diacritization cancels a running job in ${locale.code}`, async ({ page }) => {
    await open(page);
    await page.getByRole("radio", { name: locale.document, exact: true }).check();
    await page.getByLabel(locale.file, { exact: true }).setInputFiles({ name: "long.txt", mimeType: "text/plain", buffer: Buffer.from("اللغة العربية جميلة وهذا نص طويل للتشكيل.\n".repeat(10_000)) });
    await page.getByRole("button", { name: locale.upload, exact: true }).click();
    await page.waitForURL(/\/diacritize\/jobs\/[a-f0-9]{32}$/);
    await expect(page.getByRole("progressbar")).toBeVisible();
    await page.getByRole("button", { name: locale.cancel, exact: true }).click();
    await expect(page.locator("#job-status")).toHaveText(locale.cancelled);
    await expect(page.getByRole("link", { name: locale.txt, exact: true })).toHaveCount(0);
  });

  test(`Diacritization is unavailable without models in ${locale.code}`, async ({ page }) => {
    await open(page, "http://127.0.0.1:3205/diacritize");
    await expect(page.getByRole("status")).toHaveText(locale.unavailable);
    await expect(page.locator("#diacritize-form")).toHaveCount(0);
    await page.goto("http://127.0.0.1:3205/transcribe");
    await expect(page.locator("#upload-form")).toBeVisible();
  });
}
