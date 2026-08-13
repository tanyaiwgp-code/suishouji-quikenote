// 随手记 · Rust 后端入口
// M0: 骨架 — 最小可运行窗口
// M1: 文件系统核心 — 注入 FsStore，注册白名单命令

mod commands;
mod core;

use core::store::FsStore;
use core::watcher;
use std::path::PathBuf;
use tauri::Emitter;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let store = FsStore::new(default_root()).expect("无法初始化笔记根目录");
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(store)
        .invoke_handler(tauri::generate_handler![
            commands::list_notes,
            commands::read_note,
            commands::write_note,
            commands::delete_note,
            commands::acquire_note_lock,
            commands::release_note_lock,
            commands::assets_import,
            commands::assets_import_base64,
            commands::note_abs_path,
        ])
        .setup(|app| {
            // M2-4：监听笔记根目录，外部修改 → 通知前端刷新列表
            let root = app.state::<FsStore>().root().to_path_buf();
            // B7：应用自身写入（自动保存/图片导入）不触发全量刷新，
            // 仅当去抖批次内存在非自写路径才广播 notes://changed
            let own = app.state::<FsStore>().own_writes();
            let handle = app.handle().clone();
            watcher::spawn(root.clone(), move |paths: &[std::path::PathBuf]| {
                let all_own = !paths.is_empty() && paths.iter().all(|p| own.is_own(p));
                if !all_own {
                    let _ = handle.emit("notes://changed", ());
                }
            });
            // M3-3：允许 asset:// 协议读取根目录下的图片（前端 convertFileSrc 渲染）
            let _ = app.asset_protocol_scope().allow_directory(root, true);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 默认笔记根目录：`<Documents>/随手记`；Documents 被 OneDrive 重定向时回退到用户主目录。
/// M6 设置页上线前先用此默认值。
fn default_root() -> PathBuf {
    let base = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    let documents = PathBuf::from(&base).join("Documents").join("随手记");
    if documents.exists() {
        documents
    } else {
        PathBuf::from(base).join("随手记")
    }
}
