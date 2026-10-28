import { expect, test } from "@playwright/test";

const locales = [
  { code: "ar", direction: "rtl", skip: "انتقل إلى المحتوى الرئيسي", source: "الشيفرة المصدرية", telegram: "قناتنا على تيليجرام", telegramText: "تيليجرام", email: "البريد الإلكتروني" },
  { code: "en", direction: "ltr", skip: "Skip to main content", source: "Source code", telegram: "Our Telegram channel", telegramText: "Telegram", email: "Email" },
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
        // The footer's plain account links keep their full accessible names.
        const telegram = page.getByRole("contentinfo").getByRole("link", { name: locale.telegram, exact: true });
        await expect(telegram).toHaveAttribute("href", "https://t.me/ieasybooks");
        await expect(telegram).toHaveText(locale.telegramText);
        // The email link is named by exactly the text it shows.
        const email = page.getByRole("contentinfo").getByRole("link", { name: locale.email, exact: true });
        await expect(email).toHaveAttribute("href", "mailto:easybooksdev@gmail.com");
        await expect(email).toHaveText(locale.email);
        await expect(email).not.toHaveAttribute("aria-label", /./);
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

test("the header, titles and tools share one width in both languages", async ({ page, context, baseURL }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const layouts = new Set();
  for (const locale of locales) {
    await context.addCookies([{ name: "lang", value: locale.code, url: baseURL }]);
    for (const path of ["/transcribe", "/ocr", "/diacritize"]) {
      await page.goto(path);
      // The logo, the title and the tool start on the same edge, at the same place on every tool.
      layouts.add(JSON.stringify(await page.evaluate(() => {
        const edge = (selector) => {
          const box = document.querySelector(selector).getBoundingClientRect();
          return Math.round(document.dir === "rtl" ? document.documentElement.clientWidth - box.right : box.left);
        };
        const width = (selector) => Math.round(document.querySelector(selector).getBoundingClientRect().width);
        return { logo: edge(".brand"), title: edge("main h1"), tool: edge("[data-workspace]"), header: width(".header-inner"), page: width(".workspace .container") };
      })));
    }
  }
  expect([...layouts]).toHaveLength(1);
  const layout = JSON.parse([...layouts][0]);
  expect(layout.logo).toBe(layout.title);
  expect(layout.title).toBe(layout.tool);
  expect(layout.header).toBe(layout.page);
});
