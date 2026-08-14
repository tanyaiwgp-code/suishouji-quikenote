// E2E：选中笔记 → 编辑器加载 → 编辑 → 自动保存（write_note 落库）。
import { test, expect } from "@playwright/test";

test("选中笔记加载编辑器正文", async ({ page }) => {
  await page.goto("/");
  await page.locator(".note-card").nth(1).click(); // MD 会议记录
  const cm = page.locator(".cm-content");
  await expect(cm).toBeVisible();
  await expect(cm).toContainText("会议记录");
});

test("编辑内容触发自动保存（mock write_note 被调用）", async ({ page }) => {
  await page.goto("/");
  await page.locator(".note-card").nth(1).click();
  const cm = page.locator(".cm-content");
  await expect(cm).toBeVisible();
  await cm.click();
  await page.keyboard.press("Control+End"); // 光标到文末
  await page.keyboard.type("补充：已核。");
  await expect(cm).toContainText("补充：已核。");
  // 防抖保存（M3：自动保存），等待 write_note 落库
  await expect(async () => {
    const calls: string[] = await page.evaluate(
      () => (window as unknown as { __MOCK_CALLS__: string[] }).__MOCK_CALLS__,
    );
    expect(calls).toContain("write_note");
  }).toPass({ timeout: 5000 });
});
