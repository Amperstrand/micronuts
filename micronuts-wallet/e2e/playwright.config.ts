import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  timeout: 90_000,
  retries: 1,
  reporter: "list",
  use: {
    viewport: { width: 560, height: 900 },
  },
});
