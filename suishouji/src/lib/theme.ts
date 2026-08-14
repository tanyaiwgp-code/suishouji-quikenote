// 主题（三态：system/dark/light）+ 字号（三档：small/standard/large）。
// 系统跟随由 CSS media query 处理（无 data-theme 时）；显式选择存 localStorage。
// 两窗口（main/quicknote）同 origin 共享 localStorage，各自启动时应用。

export type ThemeMode = "system" | "dark" | "light";
export type FontScale = "small" | "standard" | "large";

/** 启动时应用主题与字号（DOMContentLoaded 前由 theme-init.js 防闪，此处兜底刷新）。 */
export function initTheme(): void {
  applyTheme(localStorage.getItem("theme"));
  applyFont(localStorage.getItem("fontScale"));
}

/** 应用主题三态：dark/light → 设 data-theme；system/其它 → 移除（跟随系统）。 */
export function applyTheme(stored: string | null): void {
  const el = document.documentElement;
  if (stored === "dark") {
    el.setAttribute("data-theme", "dark");
  } else if (stored === "light") {
    el.setAttribute("data-theme", "light");
  } else {
    el.removeAttribute("data-theme");
  }
}

/** 应用字号三档：small/large → data-font 覆盖 --fs-body；standard/其它 → 移除（默认 16px）。 */
export function applyFont(scale: string | null): void {
  const el = document.documentElement;
  if (scale === "small" || scale === "large") {
    el.setAttribute("data-font", scale);
  } else {
    el.removeAttribute("data-font");
  }
}
