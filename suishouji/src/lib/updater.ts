// P0-商用化：自动更新检查 + 崩溃日志提示。
// 依赖 tauri-plugin-updater / tauri-plugin-process / tauri-plugin-log（Rust 端已注册，见 lib.rs）。
// 非 Tauri（纯 vite/E2E mock）环境全部静默跳过，不阻塞主流程。

import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { error as logError, info as logInfo } from "@tauri-apps/plugin-log";
import { isTauri } from "@tauri-apps/api/core";
import { getCrashReport } from "./api";
import type { CrashReport } from "../types";

/**
 * 检查 GitHub Releases 是否有新版本。
 * 有更新 → 确认框 → 下载并静默安装 → 自动重启。
 * 无更新 / 非 Tauri / 网络失败均静默返回。
 * @param manual 是否手动触发（设置页「检查更新」按钮）：无更新时提示"已是最新版本"。
 */
export async function checkForUpdates(manual = false): Promise<void> {
  if (!isTauri()) {
    if (manual) window.alert("当前为非安装版环境，无法检查更新。");
    return;
  }
  let update: Update | null = null;
  try {
    update = await check();
  } catch (e) {
    await logError(`检查更新失败：${e}`).catch(() => {});
    if (manual) window.alert("检查更新失败：网络不可达或服务异常，请稍后重试。");
    return; // 网络不可达等：静默，不打扰用户
  }
  if (!update) {
    await logInfo("已是最新版本").catch(() => {});
    if (manual) window.alert("已是最新版本。");
    return;
  }
  const body = update.body?.trim();
  const msg =
    `发现新版本 ${update.version}（当前 ${update.currentVersion}）\n` +
    (body ? `\n${body}\n` : "") +
    "\n是否立即下载并安装？安装完成后将自动重启。";
  if (!window.confirm(msg)) return;
  try {
    await update.downloadAndInstall();
    await relaunch();
  } catch (e) {
    await logError(`更新失败：${e}`).catch(() => {});
    window.alert(`更新失败：${e}\n可稍后重试，或到官网手动下载最新版。`);
  }
}

/**
 * 启动后调用：若上次运行发生过崩溃（crash.log 存在），温和提示用户。
 * 仅提示一次（localStorage 记版本号），可点击「知道了」关闭。
 */
export async function notifyCrash(): Promise<void> {
  if (!isTauri()) return;
  let report: CrashReport | null = null;
  try {
    report = await getCrashReport();
  } catch {
    return;
  }
  if (!report || !report.exists) return;
  const key = "crash-notified-v1";
  if (localStorage.getItem(key) === "1") return;
  window.confirm(
    "检测到上次运行异常退出（崩溃）。\n\n" +
      `日志已保存至：\n${report.path}\n\n` +
      "如问题反复出现，请将日志发送给开发者，以便修复。",
  );
  localStorage.setItem(key, "1");
}
