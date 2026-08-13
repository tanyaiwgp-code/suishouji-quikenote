// 在 CSS 加载前同步设置主题，消除 FOUC（设计规范 §2）。
// 独立外部文件以满足 CSP `script-src 'self'`（内联脚本会被 Tauri 注入的 CSP 拦截）。
(function () {
  var t = localStorage.getItem("theme");
  if (t === "dark" || t === "light") {
    document.documentElement.setAttribute("data-theme", t);
  }
})();
