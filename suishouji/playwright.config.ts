// Playwright E2E：纯 Web 前端 + mock Tauri IPC（vite E2E 模式 alias 到 e2e/mocks）。
// 每个 test 独立 context → mock 模块（内存笔记库）每页独立实例，天然隔离。
import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  fullyParallel: false,
  workers: 1, // 保守串行（mock 内存态，避免 vite 模块缓存带来的共享疑云）
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:1420",
    trace: "on-first-retry",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "npx vite --port 1420 --strictPort",
    url: "http://localhost:1420",
    reuseExistingServer: !process.env.CI,
    env: { E2E: "1" },
    timeout: 60_000,
  },
});
