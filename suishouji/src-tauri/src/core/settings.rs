//! M6 应用设置持久化：`settings.json` 落在 `app_config_dir()`（`%APPDATA%\com.suishouji.app\`）。
//! 纯 Rust 零 Tauri 依赖，`read`/`write` 接收目录路径，可单测。
//! `root: None` 表示用默认根目录（`lib.rs::default_root`）。

use crate::core::error::Error;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

const FILENAME: &str = "settings.json";

/// settings.json 磁盘结构。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SettingsFile {
    /// 数据根目录（绝对路径）；None = 用默认。
    pub root: Option<String>,
}

/// 读取设置。文件缺失或损坏 → 空默认（不阻断启动，损坏时打印日志降级）。
pub fn read(dir: &Path) -> Result<SettingsFile, Error> {
    let path = dir.join(FILENAME);
    if !path.is_file() {
        return Ok(SettingsFile { root: None });
    }
    let bytes = std::fs::read(&path).map_err(Error::Io)?;
    match serde_json::from_slice::<SettingsFile>(&bytes) {
        Ok(s) => Ok(s),
        Err(e) => {
            eprintln!("settings.json 解析失败，使用默认设置：{e}");
            Ok(SettingsFile { root: None })
        }
    }
}

/// 原子写 settings.json（tmp + rename，失败清理 tmp）。
pub fn write(dir: &Path, settings: &SettingsFile) -> Result<(), Error> {
    std::fs::create_dir_all(dir).map_err(Error::Io)?;
    let path = dir.join(FILENAME);
    let tmp = dir.join(format!(
        ".settings.{}.{}.tmp",
        std::process::id(),
        WRITE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| Error::Io(std::io::Error::other(e)))?;
    std::fs::write(&tmp, json).map_err(Error::Io)?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::Io(e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "suishouji_settings_test_{}_{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn read_missing_file_returns_default() {
        let d = temp_dir("missing");
        let s = read(&d).unwrap();
        assert_eq!(s, SettingsFile { root: None });
    }

    #[test]
    fn write_then_read_roundtrip() {
        let d = temp_dir("roundtrip");
        write(&d, &SettingsFile { root: Some("C:\\notes".into()) }).unwrap();
        let s = read(&d).unwrap();
        assert_eq!(s.root.as_deref(), Some("C:\\notes"));
        // 原子写不留 tmp 残留
        assert!(std::fs::read_dir(&d).unwrap().all(|e| e.unwrap().file_name() != ".settings.tmp"));
    }

    #[test]
    fn corrupted_file_falls_back_to_default() {
        let d = temp_dir("corrupt");
        std::fs::write(d.join(FILENAME), b"not json").unwrap();
        let s = read(&d).unwrap();
        assert_eq!(s, SettingsFile { root: None });
    }

    #[test]
    fn overwrite_updates_root() {
        let d = temp_dir("overwrite");
        write(&d, &SettingsFile { root: Some("A".into()) }).unwrap();
        write(&d, &SettingsFile { root: Some("B".into()) }).unwrap();
        assert_eq!(read(&d).unwrap().root.as_deref(), Some("B"));
    }
}
