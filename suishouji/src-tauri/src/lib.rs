// 随手记 · Rust 后端入口
// M0: 骨架 — 最小可运行窗口
// M1: 文件系统核心 — 注入 FsStore，注册白名单命令
// M4: 系统托盘 + 全局快捷键 + 单实例 + 开机自启 + 窗口状态记忆 + 快速记录浮窗

mod commands;
mod core;

use core::settings;
use core::store::FsStore;
use core::watcher;
use std::path::PathBuf;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{Code, Modifiers, ShortcutState};
use tauri_plugin_window_state::{Builder as WindowStateBuilder, StateFlags};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // M4-2：全局快捷键 Ctrl+Alt+N → 显示快速记录
    // with_shortcuts 对非法加速键返回 Err；此处是受控常量，直接用 expect。
    let shortcut_plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_shortcuts(["ctrl+alt+n"])
        .expect("注册全局快捷键失败：ctrl+alt+n")
        .with_handler(|app, shortcut, event| {
            if event.state == ShortcutState::Pressed
                && shortcut.matches(Modifiers::CONTROL | Modifiers::ALT, Code::KeyN)
            {
                show_quicknote(app);
            }
        })
        .build();

    tauri::Builder::default()
        // M4-3：单实例：二次启动聚焦既有主窗口（官方要求第一个注册）
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = commands::open_main_window(app.clone());
        }))
        .plugin(shortcut_plugin)
        .plugin(tauri_plugin_opener::init())
        // M5：系统文件对话框（open 导入 DOCX / save 导出 DOCX）
        .plugin(tauri_plugin_dialog::init())
        // M4-4：开机自启（Windows 注册表 Run 键；macOS 用 LaunchAgent，Windows 忽略）
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        // M4-5：窗口状态记忆（位置/尺寸），不记忆可见性：
        //   主窗口启动即显示；快速记录浮窗默认隐藏、按需显示。
        .plugin(
            WindowStateBuilder::default()
                .with_state_flags(StateFlags::all() & !StateFlags::VISIBLE)
                .build(),
        )
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
            commands::open_main_window,
            commands::hide_quicknote,
            commands::docx_import,
            commands::docx_export,
            commands::get_app_settings,
            commands::set_app_root,
            commands::set_autostart,
        ])
        .setup(|app| {
            // M6：读设置决定根目录（settings.json → 默认），FsStore 在 setup 内 manage（改根重启生效）
            let cfg_dir = app.path().app_config_dir().unwrap_or_default();
            let root = settings::read(&cfg_dir)
                .ok()
                .and_then(|s| s.root)
                .map(PathBuf::from)
                .unwrap_or_else(default_root);
            app.manage(FsStore::new(root).expect("无法初始化笔记根目录"));

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
            // M4-1：系统托盘
            build_tray(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 显示快速记录浮窗（托盘「新建快速记录」/ 全局快捷键 Ctrl+Alt+N 用）。
/// 先通知前端新建草稿，再显示，减小「旧草稿/空白闪一帧」的概率。
fn show_quicknote(app: &AppHandle) {
    let _ = app.emit_to("quicknote", "quicknote:show", ());
    if let Some(q) = app.get_webview_window("quicknote") {
        let _ = q.show();
        let _ = q.unminimize();
        let _ = q.set_focus();
    }
}

/// M4-1：系统托盘 + 菜单。
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let quicknote_item = MenuItem::with_id(app, "quicknote", "新建快速记录", true, None::<&str>)?;
    let open_item = MenuItem::with_id(app, "open", "打开主窗口", true, None::<&str>)?;
    let autostart_item =
        CheckMenuItem::with_id(app, "autostart", "开机自启", true, false, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &quicknote_item,
            &open_item,
            &PredefinedMenuItem::separator(app)?,
            &autostart_item,
            &PredefinedMenuItem::separator(app)?,
            &quit_item,
        ],
    )?;

    // 以 autostart 当前状态初始化勾选
    let enabled = app.autolaunch().is_enabled().unwrap_or(false);
    autostart_item.set_checked(enabled)?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().expect("默认图标缺失").clone())
        .tooltip("随手记")
        .menu(&menu)
        // 左键=快速记录，右键=菜单（muda 默认左键弹菜单，此处覆盖）
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_quicknote(tray.app_handle());
            }
        })
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "quicknote" => show_quicknote(app),
            "open" => {
                let _ = commands::open_main_window(app.clone());
            }
            "autostart" => {
                let enabled = app.autolaunch().is_enabled().unwrap_or(false);
                if enabled {
                    let _ = app.autolaunch().disable();
                } else {
                    let _ = app.autolaunch().enable();
                }
                let _ = autostart_item.set_checked(!enabled);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
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
