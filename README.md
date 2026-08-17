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
- 🗑 **笔记管理**：新建（MD/TXT）/ 编辑 / 软删除（回收站可恢复）/ 改标题
- 🌗 **体验**：双主题跟随系统、字号三档、44px 可访问点击区、键盘全流程可达
- 📦 **分发**：NSIS 安装器（中文简体、当前用户免管理员）+ 绿色便携版
- 🔄 **自动更新**：启动 3s 后检查 GitHub Releases，有新版本一键下载安装并自动重启（tauri-plugin-updater）
- 🛡 **崩溃日志**：统一日志落盘（`%APPDATA%\com.suishouji.app\logs\`），panic 自动写 crash.log，下次启动提示（tauri-plugin-log + panic hook）
- 🗑 **回收站**：删除先入 `.trash/`（可恢复），支持单条恢复/永久删除/一键清空，防误删
- 💾 **一键备份/恢复**：整个笔记库打包 zip（排除回收站），可随时恢复，防数据丢失

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

## 发布与自动更新

见根目录 [`发布与自动更新流程.md`](发布与自动更新流程.md)。

- **自动发布**：推送 `v*` tag → CI（`.github/workflows/release.yml`）自动构建 + 签名 + 生成 latest.json + 创建 GitHub Release（tauri-action）
- **一次性前置**：在仓库 Secrets 配置 `TAURI_SIGNING_PRIVATE_KEY` 与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（见发布流程文档第 3 节）
- 手动构建/签名流程同样保留在文档中备用

## 测试

| 命令 | 覆盖 |
|---|---|
| `npm run check` | 前端门禁：tsc + eslint + Vitest（34 项） |
| `cd src-tauri && cargo test --lib` | Rust 核心单测（72 项） |
| `npm run test:e2e` | Playwright E2E（19 用例，Web 前端 + mock Tauri IPC） |

CI：GitHub Actions（`windows-latest`），push/PR 自动跑全部门禁 + E2E。

## 目录结构

```
suishouji/            # 应用（前端 + Tauri 后端）
  src/                # 前端（Vite + TS）
  src-tauri/src/      # Rust 后端（core/ 分层：store/frontmatter/pathguard/docx…）
  e2e/                # Playwright E2E + Tauri IPC mock
.github/workflows/    # CI
```

## License（双许可）

本软件采用 **双许可** 模式，按用途二选一：

| | 开源许可 | 商业授权 |
|---|---|---|
| **文件** | [LICENSE](LICENSE)（GNU AGPL-3.0） | [COMMERCIAL-LICENSE.md](COMMERCIAL-LICENSE.md) |
| **费用** | 免费 | 付费 |
| **适用** | 个人使用、学习、开源贡献、非商业 | 闭源商用、企业内部部署、定制交付 |
| **强制开源修改** | 是（copyleft） | 否 |

- **个人使用完全免费**（AGPL-3.0），无需任何授权；
- 若想**闭源商用**（集成进商业产品、企业内部分发、定制交付），请联系 <your-email@example.com> 洽谈商业授权；
- 详细规则见 [LICENSING.md](LICENSING.md) 与 [COMMERCIAL-LICENSE.md](COMMERCIAL-LICENSE.md)。
- 历史版本：v0.1.0 曾以 MIT 发布，其已分发副本仍保持 MIT；本版本起采用上述双许可。
