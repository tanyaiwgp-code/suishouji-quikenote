/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import { configDefaults } from "vitest/config";
import { fileURLToPath, URL } from "node:url";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;
// E2E（Playwright）：用纯 Web 前端 + mock Tauri IPC（isTauri=false，见 e2e/mocks）
// @ts-expect-error process is a nodejs global
const e2e = process.env.E2E === "1";

// https://vite.dev/config/
export default defineConfig(async () => ({

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // E2E 模式下把 Tauri IPC 模块 alias 到内存 mock（不注入 __TAURI_INTERNALS__）
  // optimizeDeps.exclude：阻止 vite 预构建这两个包成独立缓存实例（否则与 alias 的 mock 双实例，
  // 导致 mock 内状态/记录各自独立）。
  ...(e2e
    ? {
        resolve: {
          alias: {
            "@tauri-apps/api/core": fileURLToPath(
              new URL("./e2e/mocks/tauri-core.ts", import.meta.url),
            ),
            "@tauri-apps/plugin-dialog": fileURLToPath(
              new URL("./e2e/mocks/tauri-dialog.ts", import.meta.url),
            ),
          },
        },
        optimizeDeps: {
          exclude: ["@tauri-apps/api/core", "@tauri-apps/plugin-dialog"],
        },
      }
    : {}),
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  // M4：双入口 —— 主窗口 index.html + 快速记录浮窗 quicknote.html
  build: {
    rollupOptions: {
      input: {
        index: fileURLToPath(new URL("./index.html", import.meta.url)),
        quicknote: fileURLToPath(new URL("./quicknote.html", import.meta.url)),
      },
    },
  },
  // Vitest：纯逻辑单测（node 环境，无 DOM），测试文件与源码同目录 `*.test.ts`
  // exclude e2e/：Playwright 的 *.spec.ts 不属于 vitest（由 @playwright/test 执行）
  test: {
    environment: "node",
    exclude: [...configDefaults.exclude, "e2e/**"],
  },
}));
