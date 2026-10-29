import { fileURLToPath } from "node:url";
import { defineConfig, devices } from "@playwright/test";

const root = fileURLToPath(new URL("../../", import.meta.url));
const server = {
  cwd: root,
  timeout: 120_000,
  reuseExistingServer: false,
  gracefulShutdown: { signal: "SIGINT", timeout: 10_000 },
};

export default defineConfig({
  testDir: ".",
  testMatch: "*.spec.js",
  // The app allows one active transcription per IP address.
  workers: 1,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  timeout: 90_000,
  expect: { timeout: 15_000 },
  reporter: "list",
  use: {
    ...devices[process.env.BROWSER === "webkit" ? "Desktop Safari" : "Desktop Chrome"],
    baseURL: "http://127.0.0.1:3204",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  webServer: [
    {
      ...server,
      command: "node tests/browser/server.js mock",
      url: "http://127.0.0.1:9804/apps",
    },
    {
      ...server,
      command: "node tests/browser/server.js unavailable",
      url: "http://127.0.0.1:3205/health",
    },
    {
      ...server,
      command: "node tests/browser/server.js app",
      url: "http://127.0.0.1:3204/health",
    },
  ],
});
