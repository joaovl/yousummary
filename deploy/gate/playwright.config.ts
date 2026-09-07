import { defineConfig, devices } from "@playwright/test";
import { tmpdir } from "node:os";
import { join } from "node:path";

const PORT = 8199;
const BASE_URL = `http://127.0.0.1:${PORT}`;

// Each test run gets a fresh on-disk sqlite DB so jobs/state never leak between runs.
const DB_PATH = join(tmpdir(), `yousummary-e2e-${process.pid}-${Date.now()}.db`);

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: BASE_URL,
    trace: "retain-on-failure",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
  ],
  webServer: {
    command: "node index.js",
    url: `${BASE_URL}/api/health`,
    reuseExistingServer: false,
    timeout: 30_000,
    env: {
      MEETINGS_TOTP_SECRET: "JBSWY3DPEHPK3PXP",
      YOUSUMMARY_AGENT_TOKEN: "testtoken",
      YOUSUMMARY_DB_PATH: DB_PATH,
      MEETINGS_COOKIE_SECURE: "0",
      NODE_ENV: "test",
      PORT: String(PORT),
      UPSTREAM: "http://127.0.0.1:8198",
    },
  },
});
