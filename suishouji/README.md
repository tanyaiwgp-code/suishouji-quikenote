# 随手记 (Quick Notes)

1 秒内开始输入、0 负担记录的 Windows 桌面笔记应用。
纯本地文件存储（.md + assets/），零数据库，数据开放可迁移。

技术栈：**Tauri 2**（Rust 后端 + Vite + TypeScript 前端）+ nanostores + CodeMirror 6（编辑器，M3）。

## 开发进度

| 里程碑 | 状态 | 日期 |
|---|---|---|
| M0 环境与骨架 | ✅ | 2026-08-12 |
| M1 文件系统核心 | ✅ | 2026-08-13 |
| M2 主窗口三栏 UI | ✅ | 2026-08-13 |
| M3 编辑器与图文混排 | ✅ | 2026-08-13 |
| M3.5 代码审视与加固 | ✅ | 2026-08-14 |
| M4 前置 editor.ts 工厂化 | ✅ | 2026-08-14 |
| M4 浮窗托盘快捷键 | ✅ | 2026-08-14 |
| M5 格式链路 | ⬜ 待开始 | — |
| M6 打磨发布 | ⬜ 待开始 | — |

> **2026-08-14 · M3.5 代码审视与加固**：对 M0-M3 全量审视（安全/功能/性能），修复并验证 7 项：
> **S1** 预览链接 scheme 白名单（防 `javascript:` 执行/整页导航）、**B1** 文件锁泄漏、**B2** 关窗落盘、
> **B4** 删除清理孤儿 assets、**B5** 新建文件名防碰撞、**S2** 主题防闪改外部脚本（CSP 兼容）、
> **B7** watcher 自写事件抑制 + **B8** 元数据前缀读取。
>
> **2026-08-14 · M4 前置 editor.ts 工厂化**：单例 → `createEditor(target)` 类工厂，编辑器状态全部 per-instance
> （主窗口与快速记录浮窗各持一个实例）；窗口关窗接线（onCloseRequested/beforeunload → flushSave）移至入口。
> 43 单测全绿 + headless 真实 DOM 冒烟（占位/选笔记/壳/CodeMirror）零页面错误，无行为变化。
>
> **2026-08-14 · M4 浮窗托盘快捷键**：托盘常驻（左键=快速记录，右键菜单：新建快速记录/打开主窗口/开机自启勾选/退出）；
> 全局快捷键 Ctrl+Alt+N 唤出快速记录浮窗；浮窗复用编辑器工厂精简壳（隐藏返回/模式/状态栏/标题徽章，保留工具栏图片按钮），
> 草稿时间戳命名自动落盘 `收件箱/`、标题入 frontmatter，Ctrl+Enter 存并关 / Ctrl+Shift+Enter 存并开主窗 / Esc 丢弃；
> 单实例二次启动聚焦主窗；窗口位置/尺寸记忆；**主窗口 X 改为隐藏到托盘**（进程存活，退出走托盘）。
> 4 个官方插件（global-shortcut / single-instance / autostart / window-state，Rust-only 集成）。
> 剩余维护性项（前端测试、CI 等）为 M5/M6 后续项。

## 常用命令

```bash
npm run tauri dev     # 启动开发（前后端热重载，窗口 1280×800）
npm run build         # 前端 tsc 类型检查 + vite 打包
npm run tauri build   # 打包安装器（M6 验证）
cd src-tauri && cargo test --lib   # Rust 核心层单测（43 项）
```

> Rust 工具链在 `E:\rust\`，新终端需先：
> `export PATH="/e/rust/.cargo/bin:$PATH" RUSTUP_HOME="E:\\rust\\.rustup" CARGO_HOME="E:\\rust\\.cargo"`

## 目录结构

```
src-tauri/src/
├── core/           # 纯 Rust 核心层（可单测，零 Tauri 依赖）
│   ├── store.rs    # FsStore：扫描/排序/读写/原子写/图片计数
│   ├── frontmatter.rs  # 极简 frontmatter 解析
│   ├── encoding.rs # UTF-8/GBK/GB2312 检测解码
│   ├── pathguard.rs    # 路径遍历防护（canonicalize + 前缀校验）
│   ├── filelock.rs     # 进程内文件锁
│   └── watcher.rs      # notify 文件监听（M2-4）
├── commands.rs     # IPC 命令薄封装（白名单：build.rs AppManifest）
src/
├── lib/            # api.ts（类型化 invoke）/ search.ts（倒排索引）/ store.ts（nanostores）/ theme.ts（initTheme）
├── lib/editor.ts   # createEditor(target, opts) 工厂：CodeMirror 6 + markdown-it + 自动保存 + 图片 + 锁 + 精简壳选项（M3/M4）
├── ui.ts           # 渲染（列表/编辑器占位/空状态；setEditor 注入编辑器实例）
├── quicknote.ts    # 快速记录浮窗入口（M4：草稿自动落盘/标题 frontmatter/快捷键）
└── main.ts         # 主窗口入口（主题/搜索/导航/新建/文件监听/关窗→托盘）
index.html / quicknote.html   # 双入口（vite build.rollupOptions.input）
public/
└── theme-init.js   # 主题防闪外部脚本（满足 CSP script-src 'self'，S2）
```

## 设计依据

- `设计规范文档.md` — UI 设计规范（Token 配色/字号/响应式断点）
- `实施方案与技术选型.md` — 技术选型 + M0–M6 分步计划与验收标准
