// E2E：删除三入口（工具栏 🗑 / 右键菜单 / 键盘 Del）+ 二次确认。
import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
});

test("工具栏删除：确认后列表移除", async ({ page }) => {
  await page.locator(".note-card").nth(1).click(); // 选中 MD
  await page.click('.toolbar [data-cmd="delete"]');
  await expect(page.locator("#confirm-backdrop")).toBeVisible();
  await expect(page.locator("#confirm-text")).toContainText("会议记录");
  await page.click("#confirm-delete");
  await expect(page.locator(".note-card")).toHaveCount(1);
  await expect(page.locator("#confirm-backdrop")).toBeHidden();
});

test("右键菜单删除：取消不删", async ({ page }) => {
  await page.locator(".note-card").first().click({ button: "right" });
  await expect(page.locator("#ctx-menu")).toBeVisible();
  await page.click("#ctx-delete");
  await expect(page.locator("#confirm-backdrop")).toBeVisible();
  await page.click("#confirm-cancel");
  await expect(page.locator(".note-card")).toHaveCount(2);
  await expect(page.locator("#ctx-menu")).toBeHidden();
});

test("键盘 Del（点击卡片后焦点丢 body）弹确认框，Esc 关闭", async ({ page }) => {
  await page.locator(".note-card").first().click(); // 点击 → innerHTML 重绘丢焦点到 body
  await page.keyboard.press("Delete"); // 全局监听 → 确认框
  await expect(page.locator("#confirm-backdrop")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator("#confirm-backdrop")).toBeHidden();
  await expect(page.locator(".note-card")).toHaveCount(2); // 未删
});

test("编辑器内按 Delete 不触发删除（编辑操作）", async ({ page }) => {
  await page.locator(".note-card").nth(1).click(); // 打开编辑器（焦点进 CodeMirror）
  const cm = page.locator(".cm-content");
  await expect(cm).toBeVisible();
  await cm.click();
  await page.keyboard.press("Delete");
  await expect(page.locator("#confirm-backdrop")).toBeHidden();
});
