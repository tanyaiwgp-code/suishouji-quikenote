// E2E（P0-数据安全）：回收站 —— 删除入回收站、恢复、永久删除、清空。
import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
});

test("删除笔记 → 进入回收站 → 恢复", async ({ page }) => {
  // 删除第一篇笔记（软删除）
  await page.locator(".note-card").first().click();
  await page.click('.toolbar [data-cmd="delete"]');
  await expect(page.locator("#confirm-backdrop")).toBeVisible();
  await page.click("#confirm-delete");
  await expect(page.locator(".note-card")).toHaveCount(1); // 列表剩 1 篇

  // 进入回收站视图，看到被删条目
  await page.click('.nav-item[data-view="trash"]');
  await expect(page.locator(".trash-card")).toHaveCount(1);
  await expect(page.locator("#list-title")).toHaveText("回收站");

  // 恢复 → 回列表
  await page.click(".restore-btn");
  await expect(page.locator(".trash-card")).toHaveCount(0);
  await page.click('.nav-item[data-view="all"]');
  await expect(page.locator(".note-card")).toHaveCount(2);
});

test("永久删除需要确认，确认后条目消失", async ({ page }) => {
  await page.locator(".note-card").first().click();
  await page.click('.toolbar [data-cmd="delete"]');
  await page.click("#confirm-delete");
  await page.click('.nav-item[data-view="trash"]');
  await expect(page.locator(".trash-card")).toHaveCount(1);

  // 永久删除：确认框
  page.on("dialog", (d) => void d.accept());
  await page.click(".purge-btn");
  await expect(page.locator(".trash-card")).toHaveCount(0);
});

test("清空回收站：确认后全部移除", async ({ page }) => {
  // 删两篇
  for (let i = 0; i < 2; i++) {
    await page.locator(".note-card").first().click();
    await page.click('.toolbar [data-cmd="delete"]');
    await page.click("#confirm-delete");
  }
  await page.click('.nav-item[data-view="trash"]');
  await expect(page.locator(".trash-card")).toHaveCount(2);

  page.on("dialog", (d) => void d.accept());
  await page.click("#trash-empty");
  await expect(page.locator(".trash-card")).toHaveCount(0);
  await expect(page.locator(".empty-title")).toContainText("回收站是空的");
});

test("回收站视图点击条目不进入编辑器", async ({ page }) => {
  await page.locator(".note-card").first().click();
  await page.click('.toolbar [data-cmd="delete"]');
  await page.click("#confirm-delete");
  await page.click('.nav-item[data-view="trash"]');
  await expect(page.locator(".trash-card")).toHaveCount(1);

  await page.locator(".trash-card").click();
  await expect(page.locator(".editor-placeholder")).toBeVisible();
});
