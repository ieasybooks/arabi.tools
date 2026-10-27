import { expect, test, webkit } from "@playwright/test";
import { completedTranscript, openTranscript, countText } from "./review-helpers.js";

let jobPath;
test.beforeAll(async ({ browser }) => { jobPath = await completedTranscript(browser); });

// Every normalization example in REPORT section 5, plus retained marks,
// compatibility forms, case sensitivity and whole-emoji boundaries.
const cases = [
  { text: "كِتَاب", query: "كتاب", spans: [[0, 6]] },
  { text: "كِتَاب", query: "كتاب", options: { ignoreMarks: false }, spans: [] },
  { text: "مـدرسة", query: "مدرسة", spans: [[0, 6]] },
  { text: "مـدرسة", query: "مدرسة", options: { ignoreTatweel: false }, spans: [] },
  { text: "مُدَرِّس", query: "مدرس", spans: [[0, 8]] },
  { text: "أمل", query: "امل", spans: [] },
  { text: "عَلَم", query: "عل", options: { ignoreMarks: false }, spans: [] },
  { text: "عـَلَم", query: "عَلَم", options: { ignoreMarks: false }, spans: [[0, 6]] },
  { text: "ا\u0654مل", query: "أمل", options: { ignoreMarks: false, ignoreTatweel: false }, spans: [[0, 4]] },
  { text: "e\u0301", query: "é", spans: [[0, 2]] },
  { text: "مُدَرِّس", query: "مُدَرِّس", options: { ignoreMarks: false }, spans: [[0, 8]] },
  { text: "😀 كتاب", query: "كتاب", spans: [[3, 7]] },
  { text: "👩🏽‍💻 كتاب", query: "👩", spans: [] },
  { text: "👩🏽‍💻 كتاب", query: "👩🏽‍💻", spans: [[0, 7]] },
  { texts: ["كتاب", "", "هذا كتاب كتاب"], query: "كتاب", matches: [[0, 0, 4], [2, 4, 8], [2, 9, 13]] },
  { texts: ["كت", "اب"], query: "كتاب", matches: [] },
  ...["<b>", "&", '"', "[a.*]", "(a+)?"].map((text) => ({ text, query: text, spans: [[0, text.length]] })),
  { texts: ["", ""], query: "No text", matches: [] },
  ...["", " \n\t", "ـ", "َ", "ـَ"].map((query) => ({ text: "كتاب", query, spans: [] })),
  ...["\u0653", "\u0654", "\u0655", "\u0656", "\u065f", "\u0670"].map((mark) => ({ text: `ب${mark}`, query: "ب", spans: [] })),
  ...[["إ", "ا"], ["ؤ", "و"], ["ئ", "ي"], ["ى", "ي"], ["ة", "ه"], ["é", "e"], ["ﻻ", "لا"], ["ABC", "abc"]].map(([text, query]) => ({ text, query, spans: [] })),
  { text: "كتاب", query: "تاب", spans: [[1, 4]] },
  { text: "aaa", query: "aa", spans: [[0, 2], [1, 3]] },
];

for (const engine of ["chromium", "webkit"]) {
  test(`search acceptance examples in ${engine}`, async ({ page, browser }, testInfo) => {
    const extraBrowser = engine === "webkit" ? await webkit.launch() : null;
    const tab = extraBrowser ? await extraBrowser.newPage() : page;
    try {
      await openTranscript(tab, jobPath);
      const results = await tab.evaluate((cases) => cases.map(({ text, texts, query, options }) =>
        window.arabiTools.search.find(texts ?? [text], query, options)
          .map(({ segment, start, end }) => [segment, start, end])), cases);
      cases.forEach((entry, index) => {
        expect(results[index], JSON.stringify(entry)).toEqual(entry.matches ?? entry.spans.map(([start, end]) => [0, start, end]));
      });
      console.log(`${engine} ${extraBrowser ? extraBrowser.version() : browser.version()}: ${cases.length} search acceptance cases`);
      await testInfo.attach(`${engine}-version`, { body: extraBrowser ? extraBrowser.version() : browser.version(), contentType: "text/plain" });
    } finally {
      await extraBrowser?.close();
    }
  });
}

for (const fallback of [false, true]) {
  test(`whole-document adapter reveals unmounted matches with ${fallback ? "background" : "custom"} highlights and keeps focus`, async ({ page }) => {
    if (fallback) await page.addInitScript(() => { window.Highlight = undefined; });
    await openTranscript(page, jobPath);
    // Exercise the public adapter exactly as the OCR viewer does: only one page
    // is mounted, while the index contains all three pages (including a blank).
    await page.evaluate(() => {
      const toolbar = document.querySelector("[data-search]").cloneNode(true);
      document.querySelector('[data-result="transcript"]').remove();
      const panel = document.createElement("section");
      const text = document.createElement("p");
      text.id = "adapter-text";
      panel.append(toolbar, text);
      document.querySelector("main").append(panel);
      const texts = ["كتاب كتاب", "", "آخر كتاب"];
      let page = 0;
      panel.dataset.zoom = "150";
      window.adapterSearch = arabiTools.search.attach(toolbar, {
        texts,
        show(segment) {
          page = segment;
          panel.dataset.page = String(segment);
          text.textContent = texts[segment];
          return Promise.resolve(text);
        },
        element: (segment) => page === segment ? text : null,
        label: (segment) => `Page ${segment + 1}`,
      });
    });
    const field = page.getByRole("searchbox");
    await field.fill("كتاب");
    await expect.poll(() => countText(page)).toBe("Match 1 of 3");
    await field.press("Shift+Enter");
    await expect(field).toBeFocused();
    await expect(page.locator("#adapter-text")).toHaveText("آخر كتاب");
    await expect(page.locator("[data-search-active]")).toHaveText("Page 3");
    await expect(page.locator("[data-zoom]")).toHaveAttribute("data-zoom", "150");
    const next = page.getByRole("button", { name: "Next match" });
    await next.click();
    await expect(next).toBeFocused();
    await expect.poll(() => countText(page)).toBe("Match 1 of 3");
    await field.fill("No text");
    await expect(page.locator("[data-search-count]")).toHaveText("No matches found");
    await expect(next).toBeDisabled();
    await field.fill("ـَ");
    await expect(page.locator("[data-search-count]")).toBeEmpty();
    await page.evaluate(() => window.adapterSearch.dispose());
    expect(await page.locator("mark").count()).toBe(0);
    expect(await page.locator(".search-painted").count()).toBe(0);
    if (!fallback) expect(await page.evaluate(() => CSS.highlights.get("arabi-search-match").size)).toBe(0);
  });
}

test("stale searches and delayed show calls cannot restore old highlights", async ({ page }) => {
  await openTranscript(page, jobPath);
  await page.evaluate(() => {
    const toolbar = document.querySelector("[data-search]").cloneNode(true);
    document.querySelector('[data-result="transcript"]').remove();
    const element = document.createElement("p");
    element.textContent = "كتاب عالم";
    document.querySelector("main").append(toolbar, element);
    window.releases = [];
    window.testSearch = arabiTools.search.attach(toolbar, {
      texts: Array(2500).fill(element.textContent),
      element: (segment) => segment === 0 ? element : null,
      show: () => new Promise((resolve) => window.releases.push(() => resolve(element))),
      label: () => "Cue 1",
    });
  });
  await page.getByRole("searchbox").fill("كتاب");
  await page.getByRole("searchbox").fill("عالم");
  await expect.poll(() => countText(page)).toBe("Match 1 of 2500");
  await page.getByRole("button", { name: "Clear search" }).click();
  await page.evaluate(() => { window.releases.forEach((release) => release()); });
  await expect(page.locator("[data-search-count]")).toBeEmpty();
  expect(await page.evaluate(() => CSS.highlights.get("arabi-search-match").size)).toBe(0);
  await page.evaluate(() => window.testSearch.dispose());
});
