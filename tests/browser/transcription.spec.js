import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";

const speech = fileURLToPath(new URL("../fixtures/speech.mp3", import.meta.url));
const samples = [
  "مرحبا بالعالم كِتَاب مُدَرِّس كتاب",
  "مـدرسة عـَلَم أمل إمل آمل كتاب",
  '😀 كتاب Arabic 123 <b> & "نص" [a.*] café',
];

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
    await page.evaluate(() => { window.__sentinel = performance.timeOrigin; window.__fileInput = document.querySelector("[name=file]"); });
    await page.getByLabel(locale.file, { exact: true }).setInputFiles(speech);
    await page.getByRole("button", { name: locale.submit, exact: true }).click();
    await expect(page.locator(".workspace-recovery")).toHaveAttribute("href", /\/transcribe\/jobs\/[a-f0-9]{32}$/);
    await expect(page.getByRole("progressbar")).toBeVisible();
    await expect(page.locator(".submission-status")).not.toHaveText(locale.done);

    // The same document must change from running to done, without refresh or polling navigation.
    await expect(page.locator(".submission-status")).toHaveText(locale.done, { timeout: 60_000 });
    await expect(page.locator(".cue-text").first()).toBeVisible();
