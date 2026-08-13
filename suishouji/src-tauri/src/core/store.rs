//! fs_store：目录扫描、笔记模型、读写、原子写、图片计数。
//!
//! M1-1 / M1-2 / M1-4 / M1-5 / M1-7 在此汇聚，全部以单测验收。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use crate::core::encoding;
use crate::core::error::Error;
use crate::core::filelock::LockRegistry;
use crate::core::frontmatter;
use crate::core::model::{NoteFormat, NoteMeta};
use crate::core::pathguard::PathGuard;

/// preview 截断长度（设计文档：首行 80 字）。
const PREVIEW_LEN: usize = 80;
/// 搜索索引正文截断（M2：标题 + 正文前 500 字）。
const SEARCH_EXCERPT_LEN: usize = 500;
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];
/// 原子写临时文件计数器，保证同目录内名字唯一。
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct FsStore {
    root: PathBuf,
    guard: PathGuard,
    locks: Arc<LockRegistry>,
}

impl FsStore {
    /// 创建 store。根目录不存在时自动创建。
    pub fn new(root: PathBuf) -> Result<Self, Error> {
        std::fs::create_dir_all(&root)?;
        let guard = PathGuard::new(root.clone())?;
        Ok(Self {
            root,
            guard,
            locks: Arc::new(LockRegistry::new()),
        })
    }

    #[allow(dead_code)] // M2 主窗口 UI 需要读取根目录展示
    pub fn root(&self) -> &Path {
        &self.root
    }

    // ---------- M1-1 目录扫描 ----------

    /// 递归扫描根目录，返回排序后的笔记列表。
    /// 排序：置顶优先 → mtime 倒序（新在前）→ 路径字典序（稳定）。
    pub fn list_notes(&self) -> Result<Vec<NoteMeta>, Error> {
        let mut metas = Vec::new();
        self.walk(&self.root, &mut metas)?;
        metas.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then(b.mtime.cmp(&a.mtime))
                .then(a.path.cmp(&b.path))
        });
        Ok(metas)
    }

    fn walk(&self, dir: &Path, out: &mut Vec<NoteMeta>) -> Result<(), Error> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let ft = entry.file_type()?;
            if ft.is_dir() {
                self.walk(&path, out)?;
            } else if is_note_file(&path) {
                out.push(self.build_meta(&path)?);
            }
        }
        Ok(())
    }

    fn build_meta(&self, abs: &Path) -> Result<NoteMeta, Error> {
        let bytes = std::fs::read(abs)?;
        let text = encoding::decode(&bytes);
        let fm = frontmatter::parse(&text);

        let rel = abs
            .strip_prefix(&self.root)
            .expect("扫描路径必然在根内")
            .to_string_lossy()
            .replace('\\', "/");
        let stem = abs
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = fm.title.clone().unwrap_or_else(|| stem.clone());
        let format = if matches!(
            abs.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()),
            Some(e) if e == "md"
        ) {
            NoteFormat::Md
        } else {
            NoteFormat::Txt
        };

        Ok(NoteMeta {
            id: hash_rel(&rel),
            title,
            path: rel,
            format,
            mtime: modified_millis(abs)?,
            image_count: count_images(abs),
            tags: fm.tags,
            pinned: fm.pinned,
            preview: first_line(&fm.body, PREVIEW_LEN),
            search_text: truncate_chars(&fm.body, SEARCH_EXCERPT_LEN),
        })
    }

    // ---------- M1-2 读写 + 原子写 ----------

    /// 读取笔记，自动解码编码。
    pub fn read_note(&self, rel: &str) -> Result<String, Error> {
        let path = self.guard.resolve(rel)?;
        let bytes = std::fs::read(&path)?;
        Ok(encoding::decode(&bytes))
    }

    /// 原子写：同目录写临时文件 → rename 覆盖。任一步失败清理临时文件。
    pub fn write_note(&self, rel: &str, content: &str) -> Result<(), Error> {
        let target = self.guard.resolve(rel)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = tmp_path(&target);
        let res = std::fs::write(&tmp, content).and_then(|_| std::fs::rename(&tmp, &target));
        if res.is_err() {
            let _ = std::fs::remove_file(&tmp); // 失败不留残留
        }
        res?;
        Ok(())
    }

    pub fn delete_note(&self, rel: &str) -> Result<(), Error> {
        let path = self.guard.resolve(rel)?;
        if path.is_dir() {
            return Err(Error::InvalidPath(rel.to_string()));
        }
        std::fs::remove_file(path)?;
        Ok(())
    }

    // ---------- M1-7 文件锁 ----------

    pub fn acquire_lock(&self, rel: &str) -> Result<(), Error> {
        let path = self.guard.resolve(rel)?;
        self.locks.acquire(path)
    }

    pub fn release_lock(&self, rel: &str) {
        if let Ok(path) = self.guard.resolve(rel) {
            self.locks.release(&path);
        }
    }

    #[allow(dead_code)] // M2/M4 前端编辑会话与窗口关闭清理会用
    pub fn is_locked(&self, rel: &str) -> bool {
        match self.guard.resolve(rel) {
            Ok(path) => self.locks.is_locked(&path),
            Err(_) => false,
        }
    }

    #[allow(dead_code)] // M4 浮窗/主窗口关闭时兜底释放
    pub fn release_all_locks(&self) {
        self.locks.clear();
    }
}

// ---------- 内部工具 ----------

fn is_note_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "txt"))
        .unwrap_or(false)
}

/// 图片计数：统计 `笔记名/assets/` 下图片扩展名的文件数（ADR #3）。
fn count_images(note_path: &Path) -> u32 {
    let assets = note_path.with_extension("").join("assets");
    let rd = match std::fs::read_dir(&assets) {
        Ok(rd) => rd,
        Err(_) => return 0,
    };
    rd.filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_image_ext(p))
        .count() as u32
}

fn is_image_ext(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => IMAGE_EXTS.iter().any(|img| ext.eq_ignore_ascii_case(img)),
        None => false,
    }
}

fn modified_millis(path: &Path) -> Result<i64, Error> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
    Ok(mtime
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0))
}

/// 截断到前 n 个 Unicode 字符（中文按字计）。
fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// 首行截断：取正文第一个非空行，超过 max 字截断并加省略号。
fn first_line(body: &str, max: usize) -> String {
    let line = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if line.chars().count() <= max {
        line.to_string()
    } else {
        line.chars().take(max).collect::<String>() + "…"
    }
}

/// 相对路径 → 稳定十六进制哈希（DefaultHasher 固定密钥，跨运行确定）。
fn hash_rel(rel: &str) -> String {
    let mut h = DefaultHasher::new();
    rel.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn tmp_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    dir.join(format!(".{}.{}.{}.tmp", name, std::process::id(), counter))
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 建独立临时根目录（每次调用唯一路径）。
    fn temp_root(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("suishouji_store_{}_{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(root: &Path) {
        fs::remove_dir_all(root).ok();
    }

    /// 显式设置 mtime（Unix 毫秒），保证排序测试确定性。
    /// 需写访问（GENERIC_WRITE 含 FILE_WRITE_ATTRIBUTES，SetFileTime 依赖）。
    fn set_mtime(path: &Path, millis: u64) {
        let f = fs::OpenOptions::new().write(true).open(path).unwrap();
        let times = fs::FileTimes::new()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_millis(millis));
        f.set_times(times).unwrap();
    }

    // ---- M1-1 扫描与排序 ----

    #[test]
    fn scan_recurses_and_sorts() {
        let root = temp_root("scan");
        // 3 层嵌套目录 + 20 个笔记，含非笔记文件
        for i in 0..20 {
            let dir = root.join(format!("a{}/b{}", i % 4, i % 3));
            fs::create_dir_all(&dir).unwrap();
            let path = if i % 2 == 0 {
                dir.join(format!("note{i}.md"))
            } else {
                dir.join(format!("note{i}.txt"))
            };
            fs::write(&path, format!("内容 {i}\n第二行")).unwrap();
            set_mtime(&path, 1_000 + i as u64);
        }
        // 非笔记文件应被忽略
        fs::write(root.join("notes.json"), "{}").unwrap();
        fs::write(root.join("readme.md.bak"), "x").unwrap();
        fs::write(root.join("assets.log"), "x").unwrap();

        let store = FsStore::new(root.clone()).unwrap();
        let metas = store.list_notes().unwrap();
        assert_eq!(metas.len(), 20, "只统计 .md/.txt");
        // mtime 倒序：最新的在前
        assert!(metas[0].mtime >= metas[metas.len() - 1].mtime);
        for (i, w) in metas.iter().enumerate() {
            let creation = 19 - i; // mtime 倒序：列表第 i 项 = 创建序号 19-i
            assert_eq!(
                w.format,
                if creation % 2 == 0 { NoteFormat::Md } else { NoteFormat::Txt }
            );
            assert_eq!(w.preview, format!("内容 {creation}"), "mtime 倒序首行");
        }
        cleanup(&root);
    }

    #[test]
    fn scan_pinned_first_then_mtime() {
        let root = temp_root("pinned");
        fs::create_dir_all(&root).unwrap();
        // 旧但置顶
        let pinned_old = root.join("pinned.md");
        fs::write(&pinned_old, "---\npinned: true\n---\n置顶的旧笔记").unwrap();
        set_mtime(&pinned_old, 1_000);
        // 新但不置顶
        let new_note = root.join("new.md");
        fs::write(&new_note, "最新的普通笔记").unwrap();
        set_mtime(&new_note, 9_000);

        let store = FsStore::new(root.clone()).unwrap();
        let metas = store.list_notes().unwrap();
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].path, "pinned.md", "置顶优先，即使更旧");
        assert_eq!(metas[1].path, "new.md");
        assert!(metas[0].pinned);
        cleanup(&root);
    }

    // ---- M1-3 frontmatter 落到 NoteMeta ----

    #[test]
    fn meta_uses_frontmatter_title_and_tags() {
        let root = temp_root("frontmatter");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("meeting.md"),
            "---\ntitle: 周会纪要\ntags: [工作, 会议]\n---\n今天讨论了……",
        )
        .unwrap();
        // 无 frontmatter → 用文件名
        fs::write(root.join("随手.txt"), "随便记点").unwrap();

        let store = FsStore::new(root.clone()).unwrap();
        let metas = store.list_notes().unwrap();
        let meeting = metas.iter().find(|m| m.path == "meeting.md").unwrap();
        assert_eq!(meeting.title, "周会纪要");
        assert_eq!(meeting.tags, vec!["工作".to_string(), "会议".to_string()]);
        assert_eq!(meeting.preview, "今天讨论了……");

        let plain = metas.iter().find(|m| m.path == "随手.txt").unwrap();
        assert_eq!(plain.title, "随手");
        assert!(plain.tags.is_empty());
        cleanup(&root);
    }

    // ---- M1-2 读写往返 + 原子写 ----

    #[test]
    fn write_read_roundtrip() {
        let root = temp_root("roundtrip");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("sub/a.md", "# 标题\n\n正文内容").unwrap();
        assert_eq!(store.read_note("sub/a.md").unwrap(), "# 标题\n\n正文内容");
        assert!(root.join("sub/a.md").is_file());
        // 成功写后不留临时文件
        let tmps = fs::read_dir(root.join("sub"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(tmps, 0);
        cleanup(&root);
    }

    #[test]
    fn atomic_write_failure_cleans_tmp() {
        let root = temp_root("atomicfail");
        let store = FsStore::new(root.clone()).unwrap();
        // 目标是目录 → rename 失败 → 临时文件应被清理
        fs::create_dir_all(root.join("target.md")).unwrap();
        assert!(store.write_note("target.md", "x").is_err());
        let tmps = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(tmps, 0, "失败后 tmp 不残留");
        cleanup(&root);
    }

    #[test]
    fn delete_note_removes_file() {
        let root = temp_root("delete");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("gone.md", "x").unwrap();
        store.delete_note("gone.md").unwrap();
        assert!(!root.join("gone.md").exists());
        assert!(store.delete_note("gone.md").is_err(), "不存在应报错");
        cleanup(&root);
    }

    // ---- M1-5 越界读写拒绝 ----

    #[test]
    fn out_of_bounds_read_write_rejected() {
        let root = temp_root("oob");
        let store = FsStore::new(root.clone()).unwrap();
        for bad in ["../secret.md", r"..\..\x.txt", "/etc/passwd"] {
            assert!(store.read_note(bad).is_err(), "read 应拒绝: {bad}");
            assert!(store.write_note(bad, "x").is_err(), "write 应拒绝: {bad}");
            assert!(store.delete_note(bad).is_err(), "delete 应拒绝: {bad}");
        }
        cleanup(&root);
    }

    // ---- M1-4 图片计数 ----

    #[test]
    fn image_count_zero_one_many() {
        let root = temp_root("images");
        let store = FsStore::new(root.clone()).unwrap();

        // 0 图
        store.write_note("noimg.md", "无图").unwrap();
        // 1 图
        store.write_note("one.md", "一张图").unwrap();
        fs::create_dir_all(root.join("one/assets")).unwrap();
        fs::write(root.join("one/assets/pic.png"), "png").unwrap();
        // 多图 + 忽略非图片
        store.write_note("many.md", "多图").unwrap();
        fs::create_dir_all(root.join("many/assets")).unwrap();
        for n in ["a.jpg", "b.PNG", "c.webp", "notes.txt", "sprite.gif"] {
            fs::write(root.join(format!("many/assets/{n}")), "x").unwrap();
        }

        let metas = store.list_notes().unwrap();
        let by_path = |p: &str| metas.iter().find(|m| m.path == p).unwrap();
        assert_eq!(by_path("noimg.md").image_count, 0);
        assert_eq!(by_path("one.md").image_count, 1);
        assert_eq!(by_path("many.md").image_count, 4, "a/b/c/gif，txt 不算");
        cleanup(&root);
    }

    // ---- M1-7 文件锁（store 层） ----

    #[test]
    fn store_lock_acquire_conflict_release() {
        let root = temp_root("lock");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("a.md", "x").unwrap();

        store.acquire_lock("a.md").unwrap();
        assert!(store.is_locked("a.md"));
        assert!(store.acquire_lock("a.md").is_err(), "二次打开 → 锁冲突");

        store.release_lock("a.md");
        assert!(!store.is_locked("a.md"));
        store.acquire_lock("a.md").unwrap();
        store.release_all_locks();
        assert!(!store.is_locked("a.md"));
        cleanup(&root);
    }
}
