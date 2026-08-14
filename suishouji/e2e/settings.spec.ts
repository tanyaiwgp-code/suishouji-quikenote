// E2E：设置弹层（打开/主题切换/Esc 关闭）。
import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
});

test("设置弹层打开、切换主题、Esc 关闭", async ({ page }) => {
  await page.click("#settings-btn");
  await expect(page.locator("#settings-backdrop")).toBeVisible();
  // 主题三态：切到 dark
  await page.selectOption("#set-theme", "dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  // Esc 关闭
  await page.keyboard.press("Escape");
  await expect(page.locator("#settings-backdrop")).toBeHidden();
});

test("字号切换生效", async ({ page }) => {
  await page.click("#settings-btn");
  await page.selectOption("#set-font", "large");
  await expect(page.locator("html")).toHaveAttribute("data-font", "large");
});
