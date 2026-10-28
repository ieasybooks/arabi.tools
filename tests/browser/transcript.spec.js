import { execFileSync } from "node:child_process";
import { chromium, expect, test, webkit } from "@playwright/test";
import { completedTranscript, openTranscript, countText, speech } from "./review-helpers.js";

let jobPath;
test.beforeAll(async ({ browser }) => { jobPath = await completedTranscript(browser); });

for (const locale of ["ar", "en"]) {
  for (const width of [1280, 320]) {
    test(`transcript search, reading and copy in ${locale} at ${width}px`, async ({ page }) => {
      const errors = [];
      page.on("pageerror", (error) => errors.push(error.message));
      await page.setViewportSize({ width, height: 900 });
      await page.addInitScript(() => {
        window.copies = [];
        Object.defineProperty(navigator, "clipboard", { value: { writeText: async (text) => window.copies.push(text) } });
      });
      await openTranscript(page, jobPath, locale);
      const texts = await page.locator(".cue-text").allTextContents();
      expect(texts.length).toBeGreaterThan(1);
      const bytes = {};
      for (const extension of ["txt", "srt"]) {
        const response = await page.request.get(`${jobPath}/transcript.${extension}`);
        expect(response.status()).toBe(200);
        expect(response.headers()["content-disposition"]).toContain(`filename="speech.${extension}"`);
        bytes[extension] = await response.body();
      }
      expect(bytes.txt.toString()).toBe(texts.map((text) => `${text}\n`).join(""));

      // The field is named for what it searches.
      const field = page.getByRole("searchbox", { name: locale === "ar" ? "ابحث في التفريغ" : "Search transcript", exact: true });
      const originalURL = page.url();
      const storageBefore = await page.evaluate(() => [Object.entries(localStorage), Object.entries(sessionStorage)]);
      const searchRequests = [];
      page.on("request", (request) => searchRequests.push(request.url()));
      await field.fill("كتاب");
      const expected = await page.evaluate((texts) => arabiTools.search.find(texts, "كتاب", {}).length, texts);
      await expect.poll(() => countText(page)).toBe(`1/${expected}`);
      await field.press("Shift+Enter");
      await expect(field).toBeFocused();
      await expect(page.locator("[data-cue-select]")).toHaveValue(String(texts.length));
      const next = page.locator("[data-search-next]");
      await next.click();
      await expect(next).toBeFocused();
      await expect(page.locator("[data-cue-select]")).toHaveValue("1");
      // One short time format: the row, the cue picker and the active match agree; the precise
      // SRT time stays in the download.
      const firstTime = await page.locator("[data-cue-time]").first().textContent();
      expect(firstTime).toMatch(/^\d+:\d{2}(:\d{2})?$/);
      await expect(page.locator("[data-search-active]")).toContainText(firstTime);
      await expect(page.locator("[data-cue-select] option").first()).toContainText(firstTime);
      expect((await page.locator("[data-cue-select] option").allTextContents()).join("")).not.toContain(",");
      expect(page.url()).toBe(originalURL);
      expect(await page.evaluate(() => [Object.entries(localStorage), Object.entries(sessionStorage)])).toEqual(storageBefore);
      expect(searchRequests).toEqual([]);

      const source = await page.locator(".cue-text").allTextContents();
      expect(source).toEqual(texts);
      await page.locator("[data-show-timestamps]").uncheck();
      await expect(page.locator(".cue-time").first()).toBeHidden();
      await page.locator("[data-cue-select]").focus();
      await page.locator("[data-cue-select]").selectOption(String(texts.length));
      await expect(page.locator("[data-cue-select]")).toBeFocused();
      await page.locator("[data-copy-all]").click();
      await expect(page.locator("[data-copy-all]")).toBeFocused();
      await expect(page.locator("[data-copy-status]")).toHaveText(locale === "ar" ? "نُسخ النص" : "Text copied");
      expect(await page.evaluate(() => window.copies.at(-1))).toBe(bytes.txt.toString());
      // Each cue's Copy shows a short label and is named in full.
      const copyCue = page.getByRole("button", { name: locale === "ar" ? "نسخ هذا المقطع" : "Copy this cue", exact: true });
      await expect(copyCue).toHaveCount(texts.length);
      await expect(copyCue.last()).toHaveText(locale === "ar" ? "نسخ" : "Copy");
      await copyCue.last().click();
      expect(await page.evaluate(() => window.copies.at(-1))).toBe(texts.at(-1));
      expect(await page.locator(".copy-panel").count()).toBe(0);

      for (const extension of ["txt", "srt"]) {
        expect(await (await page.request.get(`${jobPath}/transcript.${extension}`)).body()).toEqual(bytes[extension]);
      }
      const geometry = await page.evaluate(() => ({
        viewport: document.documentElement.clientWidth,
        content: document.documentElement.scrollWidth,
        direction: getComputedStyle(document.querySelector(".cue-text")).direction,
        lineHeight: parseFloat(getComputedStyle(document.querySelector(".cue-text")).lineHeight),
        fontSize: parseFloat(getComputedStyle(document.querySelector(".cue-text")).fontSize),
      }));
      expect(geometry.content).toBeLessThanOrEqual(geometry.viewport);
      if (width === 1280) {
        // The header keeps one line: the name, Copy all text and Download share a row.
        const rows = await page.locator(".result-heading").evaluate((heading) => [...heading.querySelectorAll(".result-name, .result-actions > *")]
          .map((element) => Math.round(element.getBoundingClientRect().top + element.getBoundingClientRect().height / 2)));
        expect(Math.max(...rows) - Math.min(...rows)).toBeLessThanOrEqual(2);
        expect(await page.locator("[data-copy-all]").evaluate((button) => button.offsetHeight)).toBeLessThan(48);
        // One toolbar row: the reading controls at the start, the compact search at the end.
        const toolbar = await page.evaluate(() => {
          const box = (selector) => document.querySelector(selector).getBoundingClientRect();
          return { controls: box(".transcript-actions"), field: box(".search-field"), list: box(".cues") };
        });
        expect(Math.abs(toolbar.controls.top - toolbar.field.top)).toBeLessThanOrEqual(4);
        expect(toolbar.field.width).toBeLessThan(toolbar.list.width / 2);
      }
      expect(geometry.direction).toBe("rtl");
      expect(geometry.lineHeight / geometry.fontSize).toBeGreaterThanOrEqual(1.8);
      // Enter the scroll region by keyboard: pointer focus need not show
      // :focus-visible, but tabbing from the preceding control (the toolbar's
      // last, the search's second option) must do so.
      await page.locator("[data-search-tatweel]").focus();
      await page.keyboard.press("Tab");
      await expect(page.locator(".cues")).toBeFocused();
      expect(await page.locator(".cues").evaluate((element) => getComputedStyle(element).outlineStyle)).not.toBe("none");
      if (width === 320) {
        await page.evaluate(() => { document.documentElement.style.fontSize = "200%"; });
        for (const selector of ['[data-result="transcript"]', '.cues']) {
          const size = await page.locator(selector).evaluate((element) => [element.scrollWidth, element.clientWidth]);
          expect(size[0]).toBeLessThanOrEqual(size[1]);
        }
      }
      expect(errors).toEqual([]);
    });
  }
}

test("clipboard denied or missing offers the exact requested scope and removes the panel", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async () => { throw new Error("denied"); } } });
  });
  await openTranscript(page, jobPath);
  const original = await (await page.request.get(`${jobPath}/transcript.txt`)).text();
  await page.locator("[data-copy-all]").click();
  const field = page.getByRole("textbox", { name: "Text to copy" });
  await expect(field).toHaveValue(original);
  await expect(field).toBeInViewport({ ratio: 1 });
  await expect(page.locator("[data-copy-status]")).toHaveText("Copying was unavailable. Select the text and copy it manually.");
  expect(await field.evaluate((element) => [element.selectionStart, element.selectionEnd])).toEqual([0, original.length]);
  await page.getByRole("button", { name: "Close copy panel" }).click();
  await expect(field).toHaveCount(0);
  await expect(page.locator("[data-copy-all]")).toBeFocused();
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", { value: undefined }));
  const cue = await page.locator(".cue-text").last().textContent();
  await page.locator("[data-copy-cue]").last().click();
  await expect(field).toHaveValue(cue);
  await expect(field).toBeInViewport({ ratio: 1 });
  await field.press("Escape");
  await expect(field).toHaveCount(0);
  await expect(page.locator("[data-copy-cue]").last()).toBeFocused();
});

test("copy uses exact original TXT when cue parsing folded a newline", async ({ page }) => {
  await openTranscript(page, jobPath);
  await page.evaluate(() => {
    const old = document.querySelector('[data-result="transcript"]');
    const root = old.cloneNode(true);
    root.dataset.copyText = JSON.stringify("أصل\r\nبلا تغيير\n");
    delete root.dataset.transcriptReady;
    old.replaceWith(root);
    Object.defineProperty(navigator, "clipboard", { value: { writeText: async (text) => { window.copied = text; } } });
  });
  await page.locator("[data-copy-all]").click();
  expect(await page.evaluate(() => window.copied)).toBe("أصل\r\nبلا تغيير\n");
});

test("live roots initialize once, release highlights and ignore late clipboard replies", async ({ page }) => {
  await openTranscript(page, jobPath);
  await page.evaluate(() => {
    window.originalRoot = document.querySelector('[data-result="transcript"]');
    window.newRoot = originalRoot.cloneNode(true);
    originalRoot.remove();
  });
  await page.evaluate(() => document.querySelector("main").append(window.newRoot));
  await expect(page.locator('[data-result="transcript"]')).toHaveAttribute("data-transcript-ready", "true");
  await page.getByRole("searchbox").fill("كتاب");
  await expect.poll(async () => (await countText(page)).startsWith("1/")).toBe(true);
  await page.evaluate(() => {
    Object.defineProperty(navigator, "clipboard", { value: { writeText: () => new Promise((resolve) => { window.finishCopy = resolve; }) } });
  });
  await page.locator("[data-copy-all]").click();
  await page.evaluate(() => window.newRoot.remove());
  await page.evaluate(() => window.finishCopy());
  expect(await page.evaluate(() => CSS.highlights.get("arabi-search-match").size)).toBe(0);
  expect(await page.evaluate(() => window.newRoot.querySelector("[data-copy-status]").textContent)).toBe("");
  await page.evaluate(() => document.querySelector("main").append(window.originalRoot));
  await expect(page.locator('[data-result="transcript"]')).toHaveAttribute("data-transcript-ready", "true");
  await page.getByRole("searchbox").fill("كتاب");
  await expect.poll(async () => (await countText(page)).startsWith("1/")).toBe(true);
});

for (const engine of ["chromium", "webkit"]) {
  for (const fallback of [false, true]) {
    test(`${engine} ${fallback ? "background" : "custom"} highlights preserve Arabic joining and selection`, async ({ page, browserName }) => {
      const extra = engine !== browserName ? await ({ chromium, webkit })[engine].launch() : null;
      const tab = extra ? await extra.newPage() : page;
      const errors = [];
      tab.on("pageerror", (error) => errors.push(error.message));
      try {
        if (fallback) await tab.addInitScript(() => { window.Highlight = undefined; });
        await openTranscript(tab, jobPath);
        // A partial word checks shaping at both highlight boundaries.
        const before = await tab.locator(".cue-text").evaluateAll((elements) => elements.map((element) => {
          const range = new Range();
          range.selectNodeContents(element);
          const box = element.getBoundingClientRect();
          return { text: element.textContent, rects: [...range.getClientRects()].map((rect) => ({ x: rect.x - box.x, y: rect.y - box.y, width: rect.width, height: rect.height })) };
        }));
        await tab.getByRole("searchbox").fill("تاب");
        await expect.poll(async () => (await countText(tab)).startsWith("1/")).toBe(true);
        const after = await tab.locator(".cue-text").allTextContents();
        expect(after).toEqual(before.map((value) => value.text));
        const highlightedRects = await tab.locator(".cue-text").evaluateAll((elements) => elements.map((element) => {
          const range = new Range();
          range.selectNodeContents(element);
          const box = element.getBoundingClientRect();
          return [...range.getClientRects()].map((rect) => ({ x: rect.x - box.x, y: rect.y - box.y, width: rect.width, height: rect.height }));
        }));
        expect(highlightedRects).toEqual(before.map((entry) => entry.rects));
        expect(await tab.locator(".cue-text b, .cue-text script").count()).toBe(0);
        const selected = await tab.locator(".cue-text").first().evaluate((element) => {
          const selection = window.getSelection();
          const range = new Range();
          range.selectNodeContents(element);
          selection.removeAllRanges();
          selection.addRange(range);
          return selection.toString();
        });
        expect(selected).toBe(before[0].text);
        if (fallback) {
          expect(await tab.locator(".search-painted").count()).toBeGreaterThan(0);
          expect(await tab.locator(".cue-text").first().evaluate((element) => getComputedStyle(element).backgroundImage)).not.toBe("none");
          expect(await tab.locator("mark").count()).toBe(0);
          const viewport = tab.viewportSize();
          const positions = () => tab.locator(".cue-text").first().evaluate((element) => getComputedStyle(element).backgroundPosition);
          const widePositions = await positions();
          await tab.setViewportSize({ width: 320, height: 900 });
          await expect.poll(positions).not.toBe(widePositions);
          expect(await tab.locator(".cue-text").allTextContents()).toEqual(after);
          await tab.setViewportSize(viewport);
        } else {
          expect(await tab.evaluate(() => CSS.highlights.get("arabi-search-active").size)).toBe(1);
        }
        await tab.getByRole("button", { name: "Clear search" }).click();
        const restored = await tab.locator(".cue-text").evaluateAll((elements) => elements.map((element) => {
          const range = new Range();
          range.selectNodeContents(element);
          return { text: element.textContent, rects: [...range.getClientRects()].map((rect) => ({ x: rect.x, y: rect.y, width: rect.width, height: rect.height })) };
        }));
        // Text shaping and layout return exactly; marks never remain in output.
        expect(restored.map((entry) => entry.rects.map((rect) => rect.width))).toEqual(before.map((entry) => entry.rects.map((rect) => rect.width)));
        expect(await tab.locator("mark").count()).toBe(0);
        expect(await tab.locator(".search-painted").count()).toBe(0);
        expect(errors).toEqual([]);
      } finally {
        await extra?.close();
      }
    });
  }
}

test("missing Intl.Segmenter leaves reading, copy and downloads available", async ({ page }) => {
  await page.addInitScript(() => { Intl.Segmenter = undefined; });
  await openTranscript(page, jobPath);
  await expect(page.getByRole("searchbox")).toBeDisabled();
  await expect(page.locator("[data-search-count]")).toHaveText("Search needs a newer browser. You can still select, copy, and download the text.");
  await expect(page.locator(".cue-text").first()).toBeVisible();
  await expect(page.locator("[data-copy-all]")).toBeEnabled();
  expect(await (await page.request.get(`${jobPath}/transcript.txt`)).text()).not.toBe("");
});

test("three-hour transcript keeps all cues searchable, selectable and copyable", async ({ page }, testInfo) => {
  await openTranscript(page, jobPath);
  const measurement = await page.evaluate(async () => {
    const started = performance.now();
    const old = document.querySelector('[data-result="transcript"]');
    const root = old.cloneNode(true);
    delete root.dataset.transcriptReady;
    delete root.dataset.copyText;
    const prototype = root.querySelector("[data-cue]").cloneNode(true);
    const region = root.querySelector(".cues");
    const select = root.querySelector("[data-cue-select]");
    region.replaceChildren();
    select.replaceChildren();
    // 2160 five-second utterances: 3 hours, about 26,000 words. Silero's
    // default maximum speech region is 15 s; ordinary pauses make shorter cues.
    window.longTexts = Array.from({ length: 2160 }, (_, index) =>
      `في هذا الدرس نقرأ كِتَاب اللغة العربية ونتعلم معًا أصول الكتابة والتعبير ${index === 2159 ? "نهايةالمحاضرة" : index + 1}`);
    for (const [index, text] of window.longTexts.entries()) {
      const cue = prototype.cloneNode(true);
      cue.dataset.cue = String(index + 1);
      cue.id = `long-cue-${index + 1}`;
      cue.querySelector(".cue-text").textContent = text;
      cue.querySelector(".cue-number").textContent = String(index + 1);
      const seconds = index * 5;
      const time = [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60].map((value) => String(value).padStart(2, "0")).join(":");
      cue.querySelector("[data-cue-time]").textContent = time;
      region.append(cue);
      select.add(new Option(`Cue ${index + 1} at ${time}`, String(index + 1)));
    }
    old.replaceWith(root);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const renderMs = performance.now() - started;
    const searchStart = performance.now();
    const found = arabiTools.search.find(window.longTexts, "كتاب", {});
    const findMs = performance.now() - searchStart;
    const field = root.querySelector("[data-search-query]");
    const run = async (query) => {
      const start = performance.now();
      field.value = query;
      field.dispatchEvent(new Event("input"));
      while (root.querySelector("[data-search-next]").disabled) await new Promise((resolve) => requestAnimationFrame(resolve));
      // Include highlight creation and paint, not just publishing the count.
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      return performance.now() - start;
    };
    const firstSearchMs = await run("نهايةالمحاضرة");
    const cachedSearchMs = await run("كتاب");
    return { cues: window.longTexts.length, characters: window.longTexts.join("\n").length, elements: root.querySelectorAll("*").length, renderMs, findMs, firstSearchMs, cachedSearchMs, matches: found.length };
  });
  expect(measurement.matches).toBe(4320);
  expect(await page.locator("[data-cue]").count()).toBe(2160);
  await page.getByRole("searchbox").focus();
  const beforeScroll = await page.evaluate(() => window.scrollY);
  await page.getByRole("searchbox").fill("نهايةالمحاضرة");
  await expect.poll(() => countText(page)).toBe("1/1");
  await expect(page.locator("[data-cue-select]")).toHaveValue("2160");
  const revealed = await page.locator("[data-cue]").last().evaluate((cue) => {
    const box = cue.getBoundingClientRect();
    const region = cue.closest(".cues").getBoundingClientRect();
    return box.top >= region.top && box.bottom <= region.bottom;
  });
  expect(revealed).toBe(true);
  expect(await page.evaluate(() => window.scrollY)).toBe(beforeScroll);
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", { value: { writeText: async (text) => { window.copied = text; } } }));
  await page.locator("[data-copy-all]").click();
  expect(await page.evaluate(() => window.copied === window.longTexts.map((text) => `${text}\n`).join(""))).toBe(true);
  console.log(`Long transcript: ${JSON.stringify(measurement)}`);
  await testInfo.attach("long-transcript-measurement", { body: JSON.stringify(measurement, null, 2), contentType: "application/json" });
});

test("silent audio stays successful and has no phantom search or copied explanation", async ({ page }, testInfo) => {
  const silent = testInfo.outputPath("silence.mp3");
  execFileSync("ffmpeg", ["-nostdin", "-v", "error", "-f", "lavfi", "-i", "anullsrc=r=16000:cl=mono", "-t", "1", "-c:a", "libmp3lame", "-y", silent]);
  await page.goto("/transcribe");
  await page.locator('input[type="file"]').setInputFiles(silent);
  await page.getByRole("button", { name: "ابدأ التفريغ", exact: true }).click();
  const root = page.locator('[data-result="transcript"]');
  await expect(root).toHaveAttribute("data-transcript-ready", "true", { timeout: 60_000 });
  await expect(page.getByText("لم نجد كلامًا في هذا التسجيل.")).toBeVisible();
  // The mounted workspace chooses another file without leaving the empty result: the whole
  // file row is the file input.
  const choosing = page.waitForEvent("filechooser");
  await page.locator(".dropzone").click();
  await (await choosing).setFiles(speech);
  await expect(page.locator("#upload-form [type=submit]")).toBeEnabled();
  await page.getByRole("searchbox").fill("كلام");
  await expect(page.locator("[data-search-count]")).toHaveText("لم يُعثر على مطابقات");
  expect(await page.locator("[data-cue]").count()).toBe(0);
  const id = await root.getAttribute("data-job-id");
  for (const extension of ["txt", "srt"]) expect(await (await page.request.get(`/transcribe/jobs/${id}/transcript.${extension}`)).body()).toHaveLength(0);
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", { value: { writeText: async (text) => { window.copied = text; } } }));
  await page.locator("[data-copy-all]").click();
  expect(await page.evaluate(() => window.copied)).toBe("");
  await page.goto(`/transcribe/jobs/${id}`);
  await expect(root).toHaveAttribute("data-transcript-ready", "true");
  await expect(page.getByRole("link", { name: "تفريغ ملف آخر", exact: true })).toBeVisible();
});
