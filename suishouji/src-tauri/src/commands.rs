//! IPC 契约层：薄封装 `core::store`，全部命令在白名单内（见 build.rs AppManifest）。
//! 命令名必须与 build.rs 白名单、capabilities/default.json 保持一致（snake_case）。

use base64::Engine as _;
use crate::core::docx;
use crate::core::error::{CommandError, Error};
use crate::core::model::{AssetImport, NoteMeta};
use crate::core::store::FsStore;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
pub fn list_notes(store: State<FsStore>) -> Result<Vec<NoteMeta>, CommandError> {
    store.list_notes().map_err(CommandError::from)
}

#[tauri::command]
pub fn read_note(store: State<FsStore>, rel: String) -> Result<String, CommandError> {
    store.read_note(&rel).map_err(CommandError::from)
}

#[tauri::command]
pub fn write_note(store: State<FsStore>, rel: String, content: String) -> Result<(), CommandError> {
    store.write_note(&rel, &content).map_err(CommandError::from)
}

#[tauri::command]
pub fn delete_note(store: State<FsStore>, rel: String) -> Result<(), CommandError> {
    store.delete_note(&rel).map_err(CommandError::from)
}

#[tauri::command]
pub fn acquire_note_lock(store: State<FsStore>, rel: String) -> Result<(), CommandError> {
    store.acquire_lock(&rel).map_err(CommandError::from)
}

#[tauri::command]
pub fn release_note_lock(store: State<FsStore>, rel: String) -> Result<(), CommandError> {
    store.release_lock(&rel);
    Ok(())
}

/// M3-3：拖入图片 → 复制到 `笔记名/assets/` → 返回相对引用。
#[tauri::command]
pub fn assets_import(
    store: State<FsStore>,
    app: AppHandle,
    note_rel: String,
    source_path: String,
) -> Result<AssetImport, CommandError> {
    let result = store
        .import_asset(&note_rel, &source_path)
        .map_err(CommandError::from)?;
    let _ = app.emit("notes://changed", ());
    Ok(result)
}

/// M3-3：工具栏选图 → base64 解码写 assets/ → 返回相对引用。
/// base64 解码失败属 IPC 层错误（不经 core），单独映射 `image_decode_error` 码。
#[tauri::command]
pub fn assets_import_base64(
    store: State<FsStore>,
    app: AppHandle,
    note_rel: String,
    filename: String,
    data: String,
) -> Result<AssetImport, CommandError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|e| CommandError::new("image_decode_error", format!("图片数据解码失败：{e}")))?;
    let result = store
        .import_asset_bytes(&note_rel, &filename, &bytes)
        .map_err(CommandError::from)?;
    let _ = app.emit("notes://changed", ());
    Ok(result)
}

/// M3-3：返回笔记绝对路径（前端 `convertFileSrc` 渲染 assets/ 图片用）。
#[tauri::command]
pub fn note_abs_path(store: State<FsStore>, rel: String) -> Result<String, CommandError> {
    store.note_abs_path(&rel).map_err(CommandError::from)
}

// --- M5：DOCX 导入导出 ---

/// M5：DOCX 导入 → 转 Markdown 写入 `target_rel`（图片入 assets，占位符替换为真实相对引用）。
/// `source_path` 是系统对话框选的源文件（根外，仅读）；`target_rel` 由前端构造（`收件箱/xxx.md`）。
/// 安全在 `docx::import` 内：防 ZIP 炸弹 / 防 XXE / 图片仅从 zip 提取。
#[tauri::command]
pub fn docx_import(
    store: State<FsStore>,
    app: AppHandle,
    source_path: String,
    target_rel: String,
) -> Result<String, CommandError> {
    let bytes = std::fs::read(&source_path).map_err(|e| CommandError::from(Error::Io(e)))?;
    if bytes.len() as u64 > 50 * 1024 * 1024 {
        return Err(CommandError::from(Error::ImportTooLarge("源文件超 50MB".into())));
    }
    let mut note = docx::import(&bytes).map_err(CommandError::from)?;
    // 文档有标题但正文未以标题开头时，前置 `# 标题`（导入结果保持可读）
    if !note.title.is_empty() && !note.markdown.trim_start().starts_with('#') {
        note.markdown = format!("# {}\n\n{}", note.title, note.markdown);
    }
    // 先导入图片（复用 store 校验：白名单/20MB/50张），占位符替换为「相对笔记目录」的引用
    for (i, img) in note.images.iter().enumerate() {
        let filename = format!("docx{i}.{}", img.ext);
        let imported = store
            .import_asset_bytes(&target_rel, &filename, &img.bytes)
            .map_err(CommandError::from)?;
        note.markdown = note.markdown.replace(&img.placeholder, &rel_to_note(&target_rel, &imported.rel));
    }
    store.write_note(&target_rel, &note.markdown).map_err(CommandError::from)?;
    let _ = app.emit("notes://changed", ());
    Ok(target_rel)
}

/// M5：MD 笔记 → DOCX 写到 `target_path`（系统 save 对话框选的根外路径）。
#[tauri::command]
pub fn docx_export(
    store: State<FsStore>,
    rel: String,
    target_path: String,
) -> Result<(), CommandError> {
    let markdown = store.read_note(&rel).map_err(CommandError::from)?;
    // 收集 md 里的图片引用，读取对应 assets 图片字节（缺失跳过，不阻断导出）
    let mut image_pairs: Vec<(String, Vec<u8>)> = Vec::new();
    for src in md_image_srcs(&markdown) {
        if let Ok(bytes) = store.read_asset_bytes(&rel, &src) {
            image_pairs.push((src, bytes));
        }
    }
    let pairs: Vec<(&str, &[u8])> = image_pairs
        .iter()
        .map(|(s, b)| (s.as_str(), b.as_slice()))
        .collect();
    let docx_bytes = docx::export(&markdown, &pairs).map_err(CommandError::from)?;
    std::fs::write(&target_path, &docx_bytes).map_err(|e| CommandError::from(Error::Io(e)))?;
    Ok(())
}

/// 根内相对引用 → 相对笔记文件的引用（与前端 `relToNote` 一致：同目录内用短路径）。
fn rel_to_note(note_rel: &str, asset_rel: &str) -> String {
    let idx = note_rel.rfind('/');
    let dir_rel = idx.map(|i| &note_rel[..i + 1]).unwrap_or("");
    asset_rel.strip_prefix(dir_rel).unwrap_or(asset_rel).to_string()
}

/// 提取 markdown 中行首 `![...](src)` 的 src（与 docx::export 的图片嵌入顺序一致）。
fn md_image_srcs(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in md.lines() {
        let t = line.trim();
        if t.starts_with("![") {
            if let Some(src) = t.split("](").nth(1).and_then(|s| s.strip_suffix(')')) {
                out.push(src.to_string());
            }
        }
    }
    out
}

/// M4：显示并聚焦主窗口；同时隐藏快速记录浮窗，并通知主窗口刷新列表。
/// 用途：托盘「打开主窗口」、单实例二次启动、快速记录「打开主窗口」/ Ctrl+Shift+Enter。
#[tauri::command]
pub fn open_main_window(app: AppHandle) -> Result<(), CommandError> {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
    if let Some(q) = app.get_webview_window("quicknote") {
        let _ = q.hide();
    }
    // 草稿可能在浮窗内已新增/删除/落盘，让主窗口列表感知
    let _ = app.emit("notes://changed", ());
    Ok(())
}

/// M4：隐藏快速记录浮窗；通知主窗口刷新列表（保存并关闭 / 丢弃 / 关窗后调用）。
#[tauri::command]
pub fn hide_quicknote(app: AppHandle) -> Result<(), CommandError> {
    if let Some(q) = app.get_webview_window("quicknote") {
        let _ = q.hide();
    }
    let _ = app.emit("notes://changed", ());
    Ok(())
}
