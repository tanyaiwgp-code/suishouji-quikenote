// E2E：新建菜单（M7 修复回归）与新建笔记。
import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
});

test("新建菜单两项均可见（Markdown + 纯文本）", async ({ page }) => {
  await page.click("#new-note");
  const menu = page.locator("#new-menu");
  await expect(menu).toBeVisible();
  await expect(menu.locator('button[data-ext="md"]')).toHaveText("新建 Markdown");
  await expect(menu.locator('button[data-ext="txt"]')).toHaveText("新建纯文本");
  // M7 修复回归：菜单不再被底部裁剪，下方 txt 项可见
  await expect(menu.locator('button[data-ext="txt"]')).toBeVisible();
});

test("新建 Markdown 笔记出现在列表最前", async ({ page }) => {
  await page.click("#new-note");
  await page.click('#new-menu button[data-ext="md"]');
  await expect(page.locator(".note-card")).toHaveCount(3);
  await expect(page.locator(".note-card").first().locator(".badge")).toHaveText("MD");
});

test("新建纯文本笔记带 TXT 徽章", async ({ page }) => {
  await page.click("#new-note");
  await page.click('#new-menu button[data-ext="txt"]');
  await expect(page.locator(".note-card")).toHaveCount(3);
  await expect(page.locator(".note-card").first().locator(".badge")).toHaveText("TXT");
});
