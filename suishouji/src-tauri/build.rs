// M1-6 IPC 最小权限：
// AppManifest 声明白名单命令 → 打开应用命令的 ACL 检查（默认全放行会被关闭），
// 生成 allow-<cmd>/deny-<cmd> 权限；capabilities 只授予白名单。
// 命令名必须与 commands.rs 中的 #[tauri::command] 函数名一致。

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "list_notes",
                "read_note",
                "write_note",
                "delete_note",
                "acquire_note_lock",
                "release_note_lock",
                "assets_import",
                "assets_import_base64",
                "note_abs_path",
                "open_main_window",
                "hide_quicknote",
                "docx_import",
                "docx_export",
                "get_app_settings",
                "set_app_root",
                "set_autostart",
                "set_note_title",
            ]),
        ),
    )
    .expect("failed to run tauri build");
}
