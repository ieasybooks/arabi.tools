import { fileURLToPath } from "node:url";
import { expect } from "@playwright/test";

export const speech = fileURLToPath(new URL("../fixtures/speech.mp3", import.meta.url));
export const baseURL = "http://127.0.0.1:3204";

// Playwright cannot grant clipboard permissions to WebKit. Keep the real
// Chromium clipboard check and use exact-payload readback for that engine.
export async function allowClipboard(context, browserName) {
  if (browserName === "chromium") {
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  } else {
    await context.addInitScript(() => {
      let copied = "";
      Object.defineProperty(navigator, "clipboard", { configurable: true, value: {
        writeText: async text => { copied = text; },
        readText: async () => copied,
      } });
    });
  }
}

export async function completedTranscript(browser) {
  const page = await browser.newPage();
  try {
    await page.goto(`${baseURL}/transcribe`);
    await page.locator('input[type="file"]').setInputFiles(speech);
    await page.getByRole("button", { name: "ابدأ التفريغ", exact: true }).click();
    const root = page.locator('[data-result="transcript"]');
    await expect(root).toHaveAttribute("data-transcript-ready", "true", { timeout: 60_000 });
    return `/transcribe/jobs/${await root.getAttribute("data-job-id")}`;
  } finally {
    await page.close();
  }
}

export async function openTranscript(page, path, locale = "en") {
  await page.context().addCookies([{ name: "lang", value: locale, url: baseURL }]);
  await page.goto(`${baseURL}${path}`);
  await expect(page.locator('[data-result="transcript"]')).toHaveAttribute("data-transcript-ready", "true");
  await page.evaluate(() => document.fonts.ready);
}

export const unisolate = (value) => value.replace(/[\u2066-\u2069]/g, "");

export async function countText(page) {
  return unisolate(await page.locator("[data-search-count]").textContent());
}
