import { defineConfig } from "@playwright/test";

// Browser tests against the real binary: a static report.html opened from disk and the UI
// served by `tarafold serve`. Set TARAFOLD_BIN to the binary (default: ../target/release).
// PW_CHANNEL=chrome or msedge uses an installed browser instead of a downloaded one.
export default defineConfig({
  testDir: "e2e",
  timeout: 120_000,
  workers: 1,
  globalSetup: "./e2e/setup.ts",
  reporter: [["list"]],
  use: {
    channel: process.env.PW_CHANNEL || undefined,
    viewport: { width: 1400, height: 1000 },
  },
});
