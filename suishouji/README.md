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
| M4 浮窗托盘快捷键 | ⬜ 待开始 | — |
| M5 格式链路 | ⬜ 待开始 | — |
| M6 打磨发布 | ⬜ 待开始 | — |

> **2026-08-14 · M3.5 代码审视与加固**：对 M0-M3 全量审视（安全/功能/性能），修复并验证 7 项：
> **S1** 预览链接 scheme 白名单（防 `javascript:` 执行/整页导航）、**B1** 文件锁泄漏、**B2** 关窗落盘、
> **B4** 删除清理孤儿 assets、**B5** 新建文件名防碰撞、**S2** 主题防闪改外部脚本（CSP 兼容）、
> **B7** watcher 自写事件抑制 + **B8** 元数据前缀读取。剩余维护性项（`editor.ts` 实例化、前端测试、CI 等）为 M4 前置。

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
├── lib/            # api.ts（类型化 invoke）/ search.ts（倒排索引）/ store.ts（nanostores）
├── lib/editor.ts   # CodeMirror 6 编辑器 + markdown-it 预览 + 自动保存 + 图片导入 + 锁生命周期（M3/M3.5）
├── ui.ts           # 渲染（列表/编辑器占位/空状态）
└── main.ts         # 入口（主题/搜索/导航/新建/文件监听刷新）
public/
└── theme-init.js   # 主题防闪外部脚本（满足 CSP script-src 'self'，S2）
```

## 设计依据

- `设计规范文档.md` — UI 设计规范（Token 配色/字号/响应式断点）
- `实施方案与技术选型.md` — 技术选型 + M0–M6 分步计划与验收标准
