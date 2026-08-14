//! 路径遍历防护（M1-5）。
//!
//! 所有文件操作前：相对路径 → 规范化绝对路径，并校验落在根目录内。
//! 目标文件可不存在（写路径）；通过「最近存在的祖先 canonicalize」再拼接剩余部分，
//! 同时天然化解符号链接逃逸（祖先解析结果若不在根内即拒绝）。

use std::path::{Component, Path, PathBuf};

use crate::core::error::Error;

pub struct PathGuard {
    root: PathBuf,
}

impl PathGuard {
    pub fn new(root: PathBuf) -> Result<Self, Error> {
        let root = normalize(root.canonicalize()?);
        Ok(Self { root })
    }

    #[allow(dead_code)] // M2 主窗口 UI 需要读取根目录展示
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 根内的绝对路径 → 相对根目录的 `/` 分隔字符串（供图片引用回写）。
    /// 路径不在根内时拒绝。
    pub fn relative(&self, abs: &Path) -> Result<String, Error> {
        let abs = normalize(abs.to_path_buf());
        if !abs.starts_with(&self.root) {
            return Err(Error::OutsideRoot(abs.display().to_string()));
        }
        Ok(abs
            .strip_prefix(&self.root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default())
    }

    /// 解析相对路径为根内的绝对路径。拒绝绝对路径、`..`、空路径、含 NUL。
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, Error> {
        if rel.is_empty() {
            return Err(Error::InvalidPath(rel.to_string()));
        }
        if rel.contains('\0') {
            return Err(Error::InvalidPath(rel.to_string()));
        }
        // Windows 用户可能传 `\` 分隔，统一转 `/` 再按 Path 解析
        let rel = rel.replace('\\', "/");
        let p = Path::new(&rel);
        if p.is_absolute() {
            return Err(Error::InvalidPath(rel));
        }
        for comp in p.components() {
            match comp {
                Component::ParentDir => return Err(Error::OutsideRoot(rel.clone())),
                Component::RootDir | Component::Prefix(_) => return Err(Error::InvalidPath(rel)),
                _ => {}
            }
        }

        let candidate = self.root.join(p);
        self.canonicalize_within(&candidate)
    }

    /// 找到 candidate 最近存在的祖先做 canonicalize，校验前缀在根内，再重建完整路径。
    fn canonicalize_within(&self, candidate: &Path) -> Result<PathBuf, Error> {
        let mut tail: Vec<PathBuf> = Vec::new();
        let mut cur = candidate.to_path_buf();

        let existing = loop {
            match cur.canonicalize() {
                Ok(c) => break c,
                Err(_) => {
                    let name = match cur.file_name() {
                        Some(name) => name.to_owned(),
                        None => return Err(Error::InvalidPath(candidate.display().to_string())),
                    };
                    tail.push(name.into());
                    match cur.parent() {
                        Some(parent) => cur = parent.to_path_buf(),
                        None => return Err(Error::InvalidPath(candidate.display().to_string())),
                    }
                }
            }
        };

        let existing = normalize(existing);
        if !existing.starts_with(&self.root) {
            return Err(Error::OutsideRoot(candidate.display().to_string()));
        }

        let mut final_path = existing;
        for comp in tail.iter().rev() {
            final_path.push(comp);
        }
        Ok(normalize(final_path))
    }
}

/// Windows 上 `canonicalize` 返回 `\\?\` verbatim 前缀路径，
/// 与普通路径比较/拼接会不一致，统一去掉该前缀。
#[cfg(windows)]
fn normalize(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}

#[cfg(not(windows))]
fn normalize(p: PathBuf) -> PathBuf {
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("suishouji_guard_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolves_normal_relative_path() {
        let root = temp_root("normal");
        let guard = PathGuard::new(root.clone()).unwrap();
        let resolved = guard.resolve("sub/a.md").unwrap();
        assert!(resolved.starts_with(guard.root()));
        assert!(resolved.ends_with("a.md"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_parent_dir_traversal() {
        let root = temp_root("traversal");
        let guard = PathGuard::new(root.clone()).unwrap();
        for bad in ["../x.md", "a/../../x.md", r"..\..\x.md", "..", "../../x.txt"] {
            assert!(guard.resolve(bad).is_err(), "应拒绝: {bad}");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_absolute_and_drive_paths() {
        let root = temp_root("absolute");
        let guard = PathGuard::new(root.clone()).unwrap();
        for bad in ["/etc/passwd", "C:/Windows/x", "\\\\server\\share\\x", "C:x.md"] {
            assert!(guard.resolve(bad).is_err(), "应拒绝: {bad}");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_empty_and_nul() {
        let root = temp_root("empty");
        let guard = PathGuard::new(root.clone()).unwrap();
        assert!(guard.resolve("").is_err());
        assert!(guard.resolve("a\0b.md").is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolves_existing_file_to_canonical() {
        let root = temp_root("exist");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/real.md"), "x").unwrap();
        let guard = PathGuard::new(root.clone()).unwrap();
        let resolved = guard.resolve("sub/real.md").unwrap();
        assert!(resolved.is_file());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolves_non_existing_write_target() {
        let root = temp_root("write");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let guard = PathGuard::new(root.clone()).unwrap();
        let resolved = guard.resolve("sub/new.md").unwrap();
        assert!(resolved.starts_with(guard.root()));
        assert!(resolved.ends_with("sub/new.md"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(windows)]
    #[test]
    fn rejects_symlink_escaping_root() {
        let root = temp_root("symlink");
        let outside = temp_root("symlink_outside");
        // 创建符号链接需要管理员或开发者模式；失败则跳过
        if std::os::windows::fs::symlink_dir(&outside, root.join("escape")).is_err() {
            std::fs::remove_dir_all(&root).ok();
            std::fs::remove_dir_all(&outside).ok();
            return;
        }
        let guard = PathGuard::new(root.clone()).unwrap();
        // 通过符号链接访问外部目录应被拒绝（含符号链接本身，canonicalize 后越界）
        assert!(guard.resolve("escape/secret.txt").is_err());
        assert!(guard.resolve("escape").is_err(), "符号链接指向根外 → 越界拒绝");
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }
}
