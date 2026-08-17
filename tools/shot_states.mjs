// 补拍关键状态截图：选中笔记（编辑器全态）/ 设置弹层 / 删除确认。
// 运行：node tools/shot_states.mjs（需先起 vite：E2E=1 npx vite --port 1420）
import { createRequire } from "module";
const require = createRequire("E:/WorkBuddy tasks/notes-app-design/suishouji/package.json");
const { chromium } = require("@playwright/test");

const OUT = "E:/WorkBuddy tasks/.workbuddy/screenshots/";

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
await page.goto("http://localhost:1420/");
await page.waitForTimeout(1500);

// 1. 选中第一篇笔记 → 编辑器全态（标题行/工具栏/编辑区/状态栏）
await page.click(".note-card");
await page.waitForTimeout(800);
await page.screenshot({ path: `${OUT}live-main-selected.png` });
console.log("shot selected");

// 2. 删除确认弹层（危险按钮软样式）
await page.click('[data-cmd="delete"]');
await page.waitForTimeout(400);
await page.screenshot({ path: `${OUT}live-delete-modal.png` });
console.log("shot delete modal");
await page.keyboard.press("Escape");
await page.waitForTimeout(300);

// 3. 设置弹层
await page.click("#settings-btn");
await page.waitForTimeout(400);
await page.screenshot({ path: `${OUT}live-settings.png` });
console.log("shot settings");

await browser.close();
console.log("done");
