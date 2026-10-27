import { expect, test } from "@playwright/test";

const locales = [
  { code: "ar", direction: "rtl", skip: "انتقل إلى المحتوى الرئيسي", source: "الشيفرة المصدرية" },
  { code: "en", direction: "ltr", skip: "Skip to main content", source: "Source code" },
];

for (const locale of locales) {
  for (const width of [1280, 320]) {
    for (const [name, path] of [["home", "/"], ["tool", "/transcribe"]]) {
      test(`${name} shell in ${locale.code} at ${width}px`, async ({ page, context, baseURL }) => {
        await context.addCookies([{ name: "lang", value: locale.code, url: baseURL }]);
        await page.setViewportSize({ width, height: 900 });
        await page.goto(path);
        await page.evaluate(() => document.fonts.ready);
        await expect(page.locator("html")).toHaveAttribute("lang", locale.code);
        await expect(page.locator("html")).toHaveAttribute("dir", locale.direction);

        await page.keyboard.press("Tab");
        const skip = page.getByRole("link", { name: locale.skip, exact: true });
        await expect(skip).toBeFocused();
        await expect(skip).toBeInViewport();
        await expect(skip).toHaveCSS("outline-style", "solid");
        await page.keyboard.press("Enter");
        await expect(page.getByRole("main")).toBeFocused();
        await page.keyboard.press("Tab");
        expect(await page.getByRole("main").evaluate((main) => main.contains(document.activeElement))).toBe(true);

        await expect(page.getByRole("contentinfo").getByRole("link", { name: locale.source, exact: true }))
          .toHaveAttribute("href", "https://github.com/ieasybooks/arabi.tools");
        expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      });
    }
  }

  test(`skip link works without JavaScript in ${locale.code}`, async ({ browser, baseURL }) => {
    const context = await browser.newContext({ javaScriptEnabled: false });
    try {
      await context.addCookies([{ name: "lang", value: locale.code, url: baseURL }]);
      const page = await context.newPage();
      await page.goto(baseURL);
      await page.keyboard.press("Tab");
      await expect(page.getByRole("link", { name: locale.skip, exact: true })).toBeFocused();
      await page.keyboard.press("Enter");
      await expect(page.getByRole("main")).toBeFocused();
    } finally {
      await context.close();
    }
  });
}
