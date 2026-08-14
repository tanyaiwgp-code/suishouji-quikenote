// E2E：标题栏可编辑（M9）——点击进入编辑，回车保存，列表与标题栏同步更新。
import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
});

test("点击标题编辑，回车保存后列表与标题栏更新", async ({ page }) => {
  // 打开 MD 会议记录（种子 mtime 排序第 2 张）
  await page.locator(".note-card").nth(1).click();
  const title = page.locator("#ed-title");
  await expect(title).toHaveText("会议记录");

  // 点击进入编辑态（contenteditable）
  await title.click();
  await expect(title).toHaveAttribute("contenteditable", "true");

  // 全选后输入新标题，回车保存
  await page.keyboard.type("周会纪要");
  await page.keyboard.press("Enter");

  // set_note_title 更新 mtime → 该笔记排到列表最前，标题同步
  await expect(page.locator(".note-card").first().locator(".card-title")).toHaveText("周会纪要");
  await expect(title).toHaveText("周会纪要");
  await expect(title).not.toHaveAttribute("contenteditable", "true");
});

test("Esc 取消编辑，标题还原", async ({ page }) => {
  await page.locator(".note-card").nth(1).click();
  const title = page.locator("#ed-title");
  await expect(title).toHaveText("会议记录");
  await title.click();
  await page.keyboard.type("不应保存的新标题");
  await page.keyboard.press("Escape");
  await expect(title).toHaveText("会议记录"); // 还原
  await expect(page.locator(".note-card .card-title").nth(1)).toHaveText("会议记录");
});
