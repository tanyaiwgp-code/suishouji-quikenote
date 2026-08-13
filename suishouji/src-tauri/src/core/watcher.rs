//! 文件系统监听（M2-4）。
//!
//! 递归监听笔记根目录，外部编辑器改动文件后，事件静默 300ms 触发回调，
//! 前端据此重新拉取列表。忽略监听失败（目录不可读等），静默降级。

use notify::Watcher;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// 事件静默窗口：连续事件流结束后等这么久再触发。
const DEBOUNCE_MS: u64 = 300;

/// 递归监听 root；外部修改后（去抖）调用 on_change（参数为去抖批次内的事件路径）。
/// 阻塞运行直到 channel 断开。
pub fn spawn(root: PathBuf, mut on_change: impl FnMut(&[PathBuf]) + Send + 'static) {
    std::thread::spawn(move || {
        let (tx, rx) = mpsc::channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[suishouji] 文件监听启动失败: {e}");
                return;
            }
        };
        if let Err(e) = watcher.watch(&root, notify::RecursiveMode::Recursive) {
            eprintln!("[suishouji] 无法监听目录 {}: {e}", root.display());
            return;
        }
        let mut pending = false;
        let mut paths: Vec<PathBuf> = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_millis(DEBOUNCE_MS)) {
                Ok(Ok(evt)) => {
                    // 又有新事件，重置静默窗口
                    pending = true;
                    paths.extend(evt.paths);
                }
                Ok(Err(_)) => {} // 忽略 notify 内部错误事件
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if pending {
                        on_change(&paths);
                        pending = false;
                        paths.clear();
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    // 监听线程难以做确定性单测；去抖/递归行为由真实改动验证。
    // 这里仅保证模块可编译、spawn 不 panic。
    #[test]
    fn spawn_with_missing_root_is_graceful() {
        let root = std::env::temp_dir().join(format!(
            "suishouji_watcher_nonexistent_{}",
            std::process::id()
        ));
        let fired = Arc::new(AtomicBool::new(false));
        let flag = fired.clone();
        spawn(root.clone(), move |_paths: &[PathBuf]| flag.store(true, Ordering::SeqCst));
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!fired.load(Ordering::SeqCst));
        let _ = std::fs::remove_dir_all(&root);
    }
}
