// 主题初始化（消除 FOUC）。主窗口与快速记录浮窗共用。
// 系统跟随由 CSS media query 处理；显式选择存 localStorage.theme。
export function initTheme(): void {
  const stored = localStorage.getItem("theme");
  if (stored === "dark" || stored === "light") {
    document.documentElement.setAttribute("data-theme", stored);
  }
  // 否则跟随系统（CSS media query 自动处理）
}
