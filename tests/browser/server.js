import { spawn } from "node:child_process";

// Never inherit real Wit credentials or proxy settings from the developer's shell.
const env = Object.fromEntries(
  Object.entries(process.env).filter(
    ([name]) => !name.startsWith("WIT_TOKENS_") && name !== "CLIENT_IP_HEADER",
  ),
);
const mode = process.argv[2];
if (!["app", "mock"].includes(mode)) throw new Error("Expected app or mock");

const child = mode === "mock"
  ? spawn("cargo", ["run", "--locked", "--example", "mock_wit", "--", "9804"], {
      stdio: "inherit",
      env: { ...env, MOCK_WIT_DELAY_MS: "750" },
    })
  : spawn("cargo", ["run", "--locked", "--bin", "arlang-tools"], {
      stdio: "inherit",
      env: {
        ...env,
        HOST: "127.0.0.1",
        PORT: "3204",
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
