//! IPC 契约层：薄封装 `core::store`，全部命令在白名单内（见 build.rs AppManifest）。
//! 命令名必须与 build.rs 白名单、capabilities/default.json 保持一致（snake_case）。

use base64::Engine as _;
use crate::core::model::{AssetImport, NoteMeta};
use crate::core::store::FsStore;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
pub fn list_notes(store: State<FsStore>) -> Result<Vec<NoteMeta>, String> {
    store.list_notes().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_note(store: State<FsStore>, rel: String) -> Result<String, String> {
    store.read_note(&rel).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn write_note(store: State<FsStore>, rel: String, content: String) -> Result<(), String> {
    store.write_note(&rel, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_note(store: State<FsStore>, rel: String) -> Result<(), String> {
    store.delete_note(&rel).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn acquire_note_lock(store: State<FsStore>, rel: String) -> Result<(), String> {
    store.acquire_lock(&rel).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn release_note_lock(store: State<FsStore>, rel: String) -> Result<(), String> {
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
) -> Result<AssetImport, String> {
    let result = store.import_asset(&note_rel, &source_path).map_err(|e| e.to_string())?;
    let _ = app.emit("notes://changed", ());
    Ok(result)
}

/// M3-3：工具栏选图 → base64 解码写 assets/ → 返回相对引用。
#[tauri::command]
pub fn assets_import_base64(
    store: State<FsStore>,
    app: AppHandle,
    note_rel: String,
    filename: String,
    data: String,
) -> Result<AssetImport, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|e| format!("图片数据解码失败：{e}"))?;
    let result = store
        .import_asset_bytes(&note_rel, &filename, &bytes)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("notes://changed", ());
    Ok(result)
}

/// M3-3：返回笔记绝对路径（前端 `convertFileSrc` 渲染 assets/ 图片用）。
#[tauri::command]
pub fn note_abs_path(store: State<FsStore>, rel: String) -> Result<String, String> {
    store.note_abs_path(&rel).map_err(|e| e.to_string())
}

/// M4：显示并聚焦主窗口；同时隐藏快速记录浮窗，并通知主窗口刷新列表。
/// 用途：托盘「打开主窗口」、单实例二次启动、快速记录「打开主窗口」/ Ctrl+Shift+Enter。
#[tauri::command]
pub fn open_main_window(app: AppHandle) -> Result<(), String> {
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
pub fn hide_quicknote(app: AppHandle) -> Result<(), String> {
    if let Some(q) = app.get_webview_window("quicknote") {
        let _ = q.hide();
    }
    let _ = app.emit("notes://changed", ());
    Ok(())
}
