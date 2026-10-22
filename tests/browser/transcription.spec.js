import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";

const speech = fileURLToPath(new URL("../fixtures/speech.mp3", import.meta.url));
const transcript = "مرحبا بالعالم";

for (const locale of [
  {
    code: "ar",
    direction: "rtl",
    file: "الملف الصوتي",
    submit: "ابدأ التفريغ",
    done: "اكتمل التفريغ.",
    text: "تنزيل النص (TXT)",
    subtitles: "تنزيل الترجمة (SRT)",
  },
  {
    code: "en",
    direction: "ltr",
    file: "Audio file",
    submit: "Start transcription",
    done: "Transcription complete.",
    text: "Download text (.txt)",
    subtitles: "Download subtitles (.srt)",
  },
]) {
  test(`transcription updates live and downloads files in ${locale.code}`, async ({ page }) => {
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.goto("/transcribe");
    await expect(page.locator("html")).toHaveAttribute("lang", "ar");
    if (locale.code === "en") {
      await page.getByRole("button", { name: "English", exact: true }).click();
    }
    await expect(page.locator("html")).toHaveAttribute("lang", locale.code);
    await expect(page.locator("html")).toHaveAttribute("dir", locale.direction);

    const jobNavigations = [];
    page.on("framenavigated", (frame) => {
      if (frame === page.mainFrame() && /\/transcribe\/jobs\/[a-f0-9]{32}$/.test(frame.url())) {
        jobNavigations.push(frame.url());
      }
    });
    await page.getByLabel(locale.file, { exact: true }).setInputFiles(speech);
    await page.getByRole("button", { name: locale.submit, exact: true }).click();
    await page.waitForURL(/\/transcribe\/jobs\/[a-f0-9]{32}$/);
    await expect(page.getByRole("progressbar")).toBeVisible();
    await expect(page.getByText(locale.done, { exact: true })).not.toBeVisible();

    // The same document must change from running to done, without refresh or polling navigation.
    await expect(page.getByText(locale.done, { exact: true })).toBeVisible({ timeout: 60_000 });
    await expect(page.getByText(transcript, { exact: true }).first()).toBeVisible();
    expect(jobNavigations).toHaveLength(1);
    const jobPath = new URL(page.url()).pathname;

    for (const [extension, label, contentType] of [
      ["txt", locale.text, "text/plain; charset=utf-8"],
      ["srt", locale.subtitles, "application/x-subrip; charset=utf-8"],
    ]) {
      const link = page.getByRole("link", { name: label, exact: true });
      const href = await link.getAttribute("href");
      expect(href).toBe(`${jobPath}/transcript.${extension}`);
      const response = await page.request.get(href);
      expect(response.status()).toBe(200);
      expect(response.headers()["content-type"]).toBe(contentType);
      expect(response.headers()["content-disposition"]).toBe(
        `attachment; filename="speech.${extension}"; filename*=UTF-8''speech.${extension}`,
      );
      const body = await response.text();
      if (extension === "txt") {
        const lines = body.trim().split(/\r?\n/);
        expect(lines).toContain(transcript);
        expect(lines.every((line) => line === transcript)).toBe(true);
      } else {
        expect(body).toMatch(/^1\r?\n\d{2}:\d{2}:\d{2},\d{3} --> \d{2}:\d{2}:\d{2},\d{3}\r?\nمرحبا بالعالم/);
      }
      const downloadStarted = page.waitForEvent("download");
      await link.click();
      const download = await downloadStarted;
      expect(download.suggestedFilename()).toBe(`speech.${extension}`);
      expect(await download.failure()).toBeNull();
      expect(await readFile(await download.path(), "utf8")).toBe(body);
    }
    expect(errors).toEqual([]);
  });
}

test("the form stays locked while the file is being sent", async ({ page }) => {
  await page.goto("/transcribe");
  // Hold the reservation request to keep the form in the middle of sending.
  let release;
  const held = new Promise((resolve) => {
    release = resolve;
  });
  let reservations = 0;
  await page.route("**/transcribe/jobs", async (route) => {
    reservations += 1;
    await held;
    await route.continue();
  });

  const file = page.getByLabel("الملف الصوتي", { exact: true });
  const submit = page.getByRole("button", { name: "ابدأ التفريغ", exact: true });
  await file.setInputFiles(speech);
  await submit.click();
  await expect(submit).toBeDisabled();
  await expect(file).toBeDisabled();
  // A second click while sending must not send a second request.
  await submit.click({ force: true });

  release();
  expect(reservations).toBe(1);
  await page.waitForURL(/\/transcribe\/jobs\/[a-f0-9]{32}$/);
  await expect(page.getByRole("status")).toHaveText("اكتمل التفريغ.", { timeout: 60_000 });
});

// Chooses the recording on the Arabic upload page and sends it.
const send = async (tab) => {
  await tab.getByLabel("الملف الصوتي", { exact: true }).setInputFiles(speech);
  await tab.getByRole("button", { name: "ابدأ التفريغ", exact: true }).click();
};

test("a reloaded tab replaces the reservation it lost", async ({ page }) => {
  // The upload never reaches the site, so the tab's reservation stays unused.
  await page.route("**/audio", (route) => route.abort());
  await page.goto("/transcribe");
  await send(page);
  await expect(page.getByText("تعذّر رفع الملف. تحقّق من اتصالك وحاول مرة أخرى.")).toBeVisible();

  // The tab reloads and sends again from the same attempt.
  await page.unroute("**/audio");
  await page.reload();
  await send(page);
  await page.waitForURL(/\/transcribe\/jobs\/[a-f0-9]{32}$/);
  await expect(page.getByRole("status")).toHaveText("اكتمل التفريغ.", { timeout: 60_000 });
});

test("a tab opened from the upload page cannot replace its reservation", async ({ page }) => {
  // Hold the upload on its way, so the page's reservation stays unused.
  let uploading;
  const sent = new Promise((resolve) => {
    uploading = resolve;
  });
  let release;
  const held = new Promise((resolve) => {
    release = resolve;
  });
  await page.route("**/audio", async (route) => {
    uploading();
    await held;
    await route.continue();
  });
  await page.goto("/transcribe");
  await send(page);
  await sent;

  // Like a duplicated tab, the opened tab starts with a copy of the page's session storage.
  const [opened] = await Promise.all([
    page.waitForEvent("popup"),
    page.evaluate(() => {
      window.open("/transcribe");
    }),
  ]);
  await send(opened);
  await expect(
    opened.getByText("يوجد تفريغ قيد التنفيذ من اتصالك بالإنترنت. انتظر حتى ينتهي، ثم ارفع ملفًا آخر."),
  ).toBeVisible();
  await opened.close();

  // The held upload still arrives and its job runs.
  release();
  await page.waitForURL(/\/transcribe\/jobs\/[a-f0-9]{32}$/);
  await expect(page.getByRole("status")).toHaveText("اكتمل التفريغ.", { timeout: 60_000 });
});
