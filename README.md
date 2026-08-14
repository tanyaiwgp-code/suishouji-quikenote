# 随手记 · Quick Notes

> 1 秒内开始输入、0 负担记录的 **Windows 桌面笔记应用**。
> 纯本地文件存储（Markdown / TXT + assets/），零数据库，数据开放、随时可迁移。

个人向极简本地笔记：快速记录、托盘常驻、数据完全自己掌控。

## 特性

- ⚡ **秒开 + 托盘常驻**：Ctrl+Alt+N 全局唤出快速记录浮窗，随时记，一键开主窗
- 📄 **纯本地、格式开放**：每篇笔记一个 `.md` / `.txt` 文件，无数据库，可随意迁移
- 🖼 **图文混排**：图片拖入自动进 `assets/` 目录，随笔记保存
- 🔄 **DOCX 互通**：Word 文档 ↔ Markdown 导入导出（中文不乱码）
- 🔍 **本地搜索**：CJK 单字倒排索引，快
- ✏️ **内置编辑器**：CodeMirror 6 + 实时预览 + 自动保存 + 可编辑标题
- 🗑 **笔记管理**：新建（MD/TXT）/ 编辑 / 删除（二次确认）/ 改标题
- 🌗 **体验**：双主题跟随系统、字号三档、44px 可访问点击区、键盘全流程可达
- 📦 **分发**：NSIS 安装器（中文简体、当前用户免管理员）+ 绿色便携版

## 快速开始

### 下载即用
从 [Releases](https://github.com/tanyaiwgp-code/suishouji-quikenote/releases) 下载：

| 文件 | 说明 |
|---|---|
| `随手记_0.1.0_x64-setup.exe` | NSIS 安装器，双击安装（免管理员） |
| `随手记_绿色版_v0.1.0.zip` | 绿色便携版，解压即用（需系统 WebView2） |

### 从源码构建

依赖：Node 20+ · Rust 1.97+ · MSVC · WebView2

```bash
npm ci
npm run tauri dev      # 开发模式（热重载）
npm run tauri build    # 打包 NSIS 安装器
```

## 技术栈

Tauri 2（Rust 后端 + Vite + TypeScript）· CodeMirror 6 · markdown-it · nanostores · Vitest · Playwright

## 数据存储

笔记默认存于 `文档\随手记`（设置里可改数据目录）。每篇笔记 = 一个 `.md`/`.txt` 文件 + 同名 `assets/` 图片目录。
**删除数据文件夹即删除全部笔记。**

## 测试

| 命令 | 覆盖 |
|---|---|
| `npm run check` | 前端门禁：tsc + eslint + Vitest（34 项） |
| `cd src-tauri && cargo test --lib` | Rust 核心单测（64 项） |
| `npm run test:e2e` | Playwright E2E（15 用例，Web 前端 + mock Tauri IPC） |

CI：GitHub Actions（`windows-latest`），push/PR 自动跑全部门禁 + E2E。

## 目录结构

```
suishouji/            # 应用（前端 + Tauri 后端）
  src/                # 前端（Vite + TS）
  src-tauri/src/      # Rust 后端（core/ 分层：store/frontmatter/pathguard/docx…）
  e2e/                # Playwright E2E + Tauri IPC mock
.github/workflows/    # CI
```

## License

[MIT](LICENSE)
