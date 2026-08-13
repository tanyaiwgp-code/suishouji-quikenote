//! IPC 契约层：薄封装 `core::store`，全部命令在白名单内（见 build.rs AppManifest）。
//! 命令名必须与 build.rs 白名单、capabilities/default.json 保持一致（snake_case）。

use crate::core::model::NoteMeta;
use crate::core::store::FsStore;
use tauri::State;

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
