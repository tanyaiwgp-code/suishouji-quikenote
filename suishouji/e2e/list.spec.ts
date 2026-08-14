// E2E：列表加载、格式徽章、搜索过滤（种子 2 条：TXT 在前，MD 在后，按 mtime 倒序）。
import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
});

test("加载后渲染笔记列表，MD/TXT 徽章正确", async ({ page }) => {
  const cards = page.locator(".note-card");
  await expect(cards).toHaveCount(2);
  // 种子 mtime：txt=2000 > md=1000 → 倒序后 TXT 在前
  await expect(cards.first().locator(".badge")).toHaveText("TXT");
  await expect(cards.nth(1).locator(".badge")).toHaveText("MD");
  await expect(cards.first().locator(".card-title")).toHaveText("纯文本草稿：随手记 E2E。");
});

test("搜索过滤列表并更新计数", async ({ page }) => {
  await expect(page.locator(".note-card")).toHaveCount(2);
  await page.fill("#search-input", "会议");
  await expect(page.locator(".note-card")).toHaveCount(1);
  await expect(page.locator(".note-card .card-title")).toHaveText("会议记录");
  await expect(page.locator("#list-count")).toHaveText("1");
  // 清空恢复
  await page.fill("#search-input", "");
  await expect(page.locator(".note-card")).toHaveCount(2);
});
