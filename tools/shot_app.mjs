// 对当前前端做真机渲染截图，供与画布设计稿逐项对照。
// 运行：node tools/shot_app.mjs（需先起 vite：E2E=1 npx vite --port 1420）
import { createRequire } from "module";
const require = createRequire("E:/WorkBuddy tasks/notes-app-design/suishouji/package.json");
const { chromium } = require("@playwright/test");

const OUT = "E:/WorkBuddy tasks/.workbuddy/screenshots/";
const jobs = [
  ["main-light", "http://localhost:1420/", 1280, 800],
  ["main-dark", "http://localhost:1420/", 1280, 800],
  ["quick-light", "http://localhost:1420/quicknote.html", 560, 420],
  ["quick-dark", "http://localhost:1420/quicknote.html", 560, 420],
];

for (const [name, url, w, h] of jobs) {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: w, height: h } });
  await page.goto(url);
  await page.waitForTimeout(1500);
  if (name.includes("dark")) {
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "dark"));
  }
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${OUT}live-${name}.png` });
  console.log("shot", name);
  await browser.close();
}
console.log("done");
