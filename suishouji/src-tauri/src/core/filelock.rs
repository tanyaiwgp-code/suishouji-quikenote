//! 进程内文件锁（M1-7）。
//!
//! 防止快速记录浮窗与主窗口同时编辑同一文件。Tauri 前后端同进程，
//! 用进程内注册表即可；锁生命周期由前端显式 acquire / release 控制。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::core::error::Error;

#[derive(Debug, Default)]
pub struct LockRegistry {
    inner: Mutex<HashSet<PathBuf>>,
}

impl LockRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 尝试锁定路径。已被锁定则返回 `Error::Locked`。
    pub fn acquire(&self, path: PathBuf) -> Result<(), Error> {
        let mut set = self.inner.lock().unwrap();
        if set.contains(&path) {
            return Err(Error::Locked(path.display().to_string()));
        }
        set.insert(path);
        Ok(())
    }

    /// 释放锁。幂等：未持有的路径释放无副作用。
    pub fn release(&self, path: &Path) {
        self.inner.lock().unwrap().remove(path);
    }

    pub fn is_locked(&self, path: &Path) -> bool {
        self.inner.lock().unwrap().contains(path)
    }

    /// 释放全部锁（窗口关闭时兜底，防止前端忘记 release）。
    pub fn clear(&self) {
        self.inner.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_acquire_conflicts_then_release_frees() {
        let registry = LockRegistry::new();
        let path = PathBuf::from(r"C:\notes\a.md");

        assert!(registry.acquire(path.clone()).is_ok());
        assert!(registry.is_locked(&path));
        // 二次打开同一文件 → 锁冲突
        assert!(matches!(
            registry.acquire(path.clone()),
            Err(Error::Locked(_))
        ));

        registry.release(&path);
        assert!(!registry.is_locked(&path));
        // 关闭后再次打开成功
        assert!(registry.acquire(path).is_ok());
    }

    #[test]
    fn different_paths_do_not_conflict() {
        let registry = LockRegistry::new();
        assert!(registry.acquire(PathBuf::from("a.md")).is_ok());
        assert!(registry.acquire(PathBuf::from("b.md")).is_ok());
    }

    #[test]
    fn release_is_idempotent() {
        let registry = LockRegistry::new();
        let path = PathBuf::from("a.md");
        registry.release(&path); // 未持有，无副作用
        assert!(registry.acquire(path.clone()).is_ok());
        registry.clear();
        assert!(!registry.is_locked(&path));
    }
}
