// 命令名契约测试：锁定「TS api.ts / Rust commands.rs / build.rs 白名单 / capabilities 权限」4 清单一致。
// 新增命令忘同步任意一处即红。改命令名/权限请同时改 4 处。
import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// src/lib/contract.test.ts → suishouji/ 根 = ../../；src-tauri = ../../src-tauri
const ROOT = fileURLToPath(new URL("../../", import.meta.url));
const SRC_TAURI = join(ROOT, "src-tauri");

/** 从 src/lib/api.ts 提取 invoke/invokeOrThrow 的命令名字面量。 */
function commandsFromApi(): string[] {
  const src = readFileSync(join(ROOT, "src/lib/api.ts"), "utf8");
  return [...src.matchAll(/(?:invoke|invokeOrThrow)(?:<[^>]*>)?\(\s*["']([a-zA-Z0-9_]+)["']/g)].map(
    (m) => m[1],
  );
}

/** 从 src-tauri/src/commands.rs 提取 #[tauri::command] 函数名。 */
function commandsFromRust(): string[] {
  const src = readFileSync(join(SRC_TAURI, "src/commands.rs"), "utf8");
  return [...src.matchAll(/pub fn ([a-z0-9_]+)\(/g)].map((m) => m[1]);
}

/** 从 src-tauri/build.rs 提取 AppManifest::commands(&[...]) 白名单。 */
function commandsFromBuild(): string[] {
  const src = readFileSync(join(SRC_TAURI, "build.rs"), "utf8");
  const start = src.indexOf("commands(&[");
  const end = src.indexOf("])", start);
  if (start < 0 || end < 0) throw new Error("build.rs 中找不到 commands(&[ ... ]) 白名单");
  const arr = src.slice(start, end);
  return [...arr.matchAll(/["']([a-z0-9_]+)["']/g)].map((m) => m[1]);
}

/** 从 capabilities/*.json 提取 allow-* 权限（去掉 core:/opener: 前缀的项），- → _。 */
function commandsFromCapabilities(): string[] {
  const dir = join(SRC_TAURI, "capabilities");
  const files = readdirSync(dir).filter((f) => f.endsWith(".json"));
  const perms: string[] = [];
  for (const f of files) {
    const json = JSON.parse(readFileSync(join(dir, f), "utf8")) as { permissions?: string[] };
    for (const p of json.permissions ?? []) {
      // 仅匹配 `allow-xxx-yyy`（自定义命令权限，无命名空间冒号前缀）
      const m = /^allow-([a-z][a-z0-9-]*)$/.exec(p);
      if (m) perms.push(m[1].replace(/-/g, "_"));
    }
  }
  return perms;
}

function sortedUnique(arr: string[]): string[] {
  return [...new Set(arr)].sort();
}

describe("命令名 4 清单契约", () => {
  it("api.ts / commands.rs / build.rs / capabilities 四者一致", () => {
    const api = sortedUnique(commandsFromApi());
    const rust = sortedUnique(commandsFromRust());
    const build = sortedUnique(commandsFromBuild());
    const caps = sortedUnique(commandsFromCapabilities());

    expect(api, "前端调用 ≠ Rust 命令").toEqual(rust);
    expect(rust, "Rust 命令 ≠ build.rs 白名单").toEqual(build);
    expect(build, "build.rs 白名单 ≠ capabilities 权限").toEqual(caps);
    // 直接对完整四元组断言，避免某一环缺失时诊断信息不足
    expect(api.join(",")).toBe(build.join(","));
  });

  it("命令名全集已建立基线（防空白/误提取）", () => {
    const build = sortedUnique(commandsFromBuild());
    // M0-M6 已交付 16 个命令；新增命令时应同时更新此基线
    expect(build).toEqual([
      "acquire_note_lock",
      "assets_import",
      "assets_import_base64",
      "delete_note",
      "docx_export",
      "docx_import",
      "get_app_settings",
      "hide_quicknote",
      "list_notes",
      "note_abs_path",
      "open_main_window",
      "read_note",
      "release_note_lock",
      "set_app_root",
      "set_autostart",
      "set_note_title",
      "write_note",
    ]);
  });
});
