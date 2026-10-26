import { spawn } from "node:child_process";
import { resolve } from "node:path";

// Never inherit real Wit credentials or proxy settings from the developer's shell.
const env = Object.fromEntries(
  Object.entries(process.env).filter(
    ([name]) => !name.startsWith("WIT_TOKENS_") && !name.startsWith("DRIVE_") && name !== "CLIENT_IP_HEADER",
  ),
);
const mode = process.argv[2];
if (!["app", "mock", "unavailable"].includes(mode)) throw new Error("Expected app, mock or unavailable");
if (mode !== "app") {
  delete env.TASHKEEL_MODEL_CACHE;
  delete env.ORT_DYLIB_PATH;
}
const target = resolve(process.env.CARGO_TARGET_DIR || "target", "debug");
const binary = mode === "mock" ? "examples/mock_wit" : mode === "app" ? "examples/mock_ocr" : "arlang-tools";
// Binaries and assets are built before the suite; browser tests never start cargo builds.
const child = spawn(resolve(target, binary), mode === "mock" ? ["9804"] : [], {
  stdio: "inherit",
  env: {
    ...env,
    MOCK_WIT_DELAY_MS: "750",
    HOST: "127.0.0.1",
    PORT: "3205",
    WIT_API_URL: "http://127.0.0.1:9804",
    WIT_TOKENS_AR: "dev",
  },
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => child.kill(signal));
}
child.on("error", (error) => {
  console.error(error);
  process.exit(1);
});
child.on("exit", (code) => process.exit(code ?? 1));
