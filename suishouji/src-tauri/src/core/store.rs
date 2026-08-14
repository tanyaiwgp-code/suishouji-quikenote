//! fs_store：目录扫描、笔记模型、读写、原子写、图片计数。
//!
//! M1-1 / M1-2 / M1-4 / M1-5 / M1-7 在此汇聚，全部以单测验收。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::core::encoding;
use crate::core::error::Error;
use crate::core::filelock::LockRegistry;
use crate::core::frontmatter;
use crate::core::model::{AssetImport, NoteFormat, NoteMeta};
use crate::core::pathguard::PathGuard;

/// preview 截断长度（设计文档：首行 80 字）。
const PREVIEW_LEN: usize = 80;
/// 搜索索引正文截断（M2：标题 + 正文前 500 字）。
const SEARCH_EXCERPT_LEN: usize = 500;
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];
/// 单张图片大小上限（M3-3，安全审查：≤20MB）。
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
/// 单笔记图片数上限（M3-3，安全审查：≤50 张）。
const MAX_IMAGES_PER_NOTE: u32 = 50;
/// 原子写临时文件计数器，保证同目录内名字唯一。
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
/// 导入图片文件名计数器，保证同名目录内名字唯一。
static IMPORT_COUNTER: AtomicU64 = AtomicU64::new(0);
/// 元数据读取上限（B8）：frontmatter + preview(80字) + 搜索摘录(500字) 均在前 8KB 内，
/// 无需为构建列表而全量读文件。
const META_READ_LIMIT: usize = 8 * 1024;
/// 应用自身最近写入路径的窗口（B7）：此窗口内的写事件视为自写，不触发全量刷新。
const OWN_WRITE_WINDOW: Duration = Duration::from_millis(600);

/// 记录应用自身（自动保存/图片导入）写入的路径，供 watcher 抑制回环通知（B7）。
pub struct OwnWriteRegistry {
    inner: Mutex<Vec<(PathBuf, Instant)>>,
}

impl OwnWriteRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
        }
    }

    /// 记录一次自写路径（顺带清理过期项）。
    pub fn record(&self, path: PathBuf) {
        let mut v = self.inner.lock().unwrap();
        v.retain(|(_, t)| t.elapsed() < OWN_WRITE_WINDOW);
        v.push((path, Instant::now()));
    }

    /// 该路径是否在自写窗口内（Windows 下按大小写不敏感比较，防盘符大小写抖动）。
    pub fn is_own(&self, path: &Path) -> bool {
        let v = self.inner.lock().unwrap();
        v.iter()
            .any(|(p, t)| t.elapsed() < OWN_WRITE_WINDOW && path_eq(p, path))
    }
}

/// 路径比较：Windows 上路径大小写不敏感，统一小写后比较。
#[cfg(windows)]
fn path_eq(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_ascii_lowercase() == b.to_string_lossy().to_ascii_lowercase()
}

#[cfg(not(windows))]
fn path_eq(a: &Path, b: &Path) -> bool {
    a == b
}

pub struct FsStore {
    root: PathBuf,
    guard: PathGuard,
    locks: Arc<LockRegistry>,
    /// 自写路径注册表（B7）：供 watcher 抑制应用自身写产生的回环刷新。
    own: Arc<OwnWriteRegistry>,
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
            own: Arc::new(OwnWriteRegistry::new()),
        })
    }

    #[allow(dead_code)] // M2 主窗口 UI 需要读取根目录展示
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// B7：自写路径注册表（供 watcher 过滤自身写入事件）。
    pub fn own_writes(&self) -> Arc<OwnWriteRegistry> {
        self.own.clone()
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
        // B8：只读前 8KB（frontmatter/preview/搜索摘录都在前部），避免对整库全量读文件。
        let bytes = read_prefix(abs, META_READ_LIMIT)?;
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
    /// B7：把临时文件与目标都登记为「自写」，避免 watcher 对本次保存触发全量刷新。
    pub fn write_note(&self, rel: &str, content: &str) -> Result<(), Error> {
        let target = self.guard.resolve(rel)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = tmp_path(&target);
        self.own.record(tmp.clone()); // 临时文件创建/改名源
        self.own.record(target.clone()); // 改名落地目标
        let res = std::fs::write(&tmp, content).and_then(|_| std::fs::rename(&tmp, &target));
        if res.is_err() {
            let _ = std::fs::remove_file(&tmp); // 失败不留残留
        }
        res?;
        Ok(())
    }

    /// 删除笔记文件；若该笔记独享 `assets/` 目录（无同 stem 兄弟笔记共享），一并清理（B4）。
    pub fn delete_note(&self, rel: &str) -> Result<(), Error> {
        let path = self.guard.resolve(rel)?;
        if path.is_dir() {
            return Err(Error::InvalidPath(rel.to_string()));
        }
        std::fs::remove_file(&path)?;
        // 仅笔记（.md/.txt）有 assets 目录；清理为尽力而为，失败不阻断删除本身
        if is_note_file(&path) {
            let assets = path.with_extension("").join("assets");
            if assets.is_dir() && !has_sibling_same_stem(&path) {
                let _ = std::fs::remove_dir_all(&assets);
            }
        }
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

    // ---------- M3-3 图片导入 ----------

    /// 拖入场景：把磁盘上的源图片复制到 `笔记名/assets/`（ADR #3）。
    /// 校验：源为文件 + 图片扩展名 + ≤20MB + 该笔记图数 <50。
    pub fn import_asset(&self, note_rel: &str, source: &str) -> Result<AssetImport, Error> {
        let src = PathBuf::from(source);
        if !src.is_file() {
            return Err(Error::NotFound(source.to_string()));
        }
        self.place_asset(note_rel, &src, &src)
    }

    /// 工具栏选图场景：把 base64 解码后的字节写入 `笔记名/assets/`。
    /// `filename` 仅用于推导扩展名；校验规则同 `import_asset`。
    pub fn import_asset_bytes(
        &self,
        note_rel: &str,
        filename: &str,
        bytes: &[u8],
    ) -> Result<AssetImport, Error> {
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err(Error::ImageTooLarge(format!("{filename} ({:.1}MB)", bytes.len() as f64 / 1_048_576.0)));
        }
        // 派生扩展名（文件名可能含非法字符，扩展名白名单校验后丢弃文件名）
        let ext = Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| Error::UnsupportedType(filename.to_string()))?;
        if !IMAGE_EXTS.iter().any(|img| ext == *img) {
            return Err(Error::UnsupportedType(filename.to_string()));
        }

        // 写入临时文件再走统一放置逻辑（校验在 `place_asset` 内复用）
        let tmp = std::env::temp_dir().join(format!(
            "suishouji_import_{}_{}.{}",
            std::process::id(),
            IMPORT_COUNTER.fetch_add(1, Ordering::Relaxed),
            ext
        ));
        let result = std::fs::write(&tmp, bytes)
            .map_err(Error::from)
            .and_then(|_| self.place_asset(note_rel, &tmp, &tmp));
        let _ = std::fs::remove_file(&tmp);
        result
    }

    /// 返回笔记文件的绝对路径（供前端 `convertFileSrc` 渲染图片）。
    pub fn note_abs_path(&self, rel: &str) -> Result<String, Error> {
        let path = self.guard.resolve(rel)?;
        if !path.is_file() {
            return Err(Error::NotFound(rel.to_string()));
        }
        Ok(path.to_string_lossy().into_owned())
    }

    /// M5：读取笔记 `assets/` 下的图片字节（DOCX 导出嵌入用）。
    /// `asset_src` 是 Markdown 里的图片相对引用（相对笔记所在目录），
    /// 经 `PathGuard::relative` 校验在根内 + `resolve` 规范化，防 `../` 逃逸与符号链接。
    pub fn read_asset_bytes(&self, note_rel: &str, asset_src: &str) -> Result<Vec<u8>, Error> {
        let note_abs = self.guard.resolve(note_rel)?;
        let dir = note_abs
            .parent()
            .ok_or_else(|| Error::InvalidPath(note_rel.to_string()))?;
        let candidate = dir.join(asset_src);
        let rel = self.guard.relative(&candidate)?;
        let abs = self.guard.resolve(&rel)?;
        if !abs.is_file() {
            return Err(Error::NotFound(asset_src.to_string()));
        }
        std::fs::read(&abs).map_err(Into::into)
    }

    /// 统一放置逻辑：校验源为图片且未超限，复制到目标，返回相对引用。
    fn place_asset(&self, note_rel: &str, src: &Path, display: &Path) -> Result<AssetImport, Error> {
        let note = self.guard.resolve(note_rel)?;
        if !note.is_file() {
            return Err(Error::NotFound(note_rel.to_string()));
        }
        let ext = match src.extension().and_then(|e| e.to_str()) {
            Some(e) if IMAGE_EXTS.iter().any(|img| e.eq_ignore_ascii_case(img)) => {
                e.to_ascii_lowercase()
            }
            _ => return Err(Error::UnsupportedType(display.display().to_string())),
        };
        if src.metadata()?.len() > MAX_IMAGE_BYTES {
            return Err(Error::ImageTooLarge(format!(
                "{} ({:.1}MB)",
                display.display(),
                src.metadata()?.len() as f64 / 1_048_576.0
            )));
        }
        if count_images(&note) >= MAX_IMAGES_PER_NOTE {
            return Err(Error::ImageLimitReached(note_rel.to_string()));
        }

        let assets_dir = note.with_extension("").join("assets");
        std::fs::create_dir_all(&assets_dir)?;
        let target = unique_asset_path(&assets_dir, &ext);
        std::fs::copy(src, &target)?;
        // B7：目录创建与图片写入都登记为自写，抑制 watcher 回环（图片数更新由 commands 显式广播）。
        self.own.record(assets_dir.clone());
        self.own.record(target.clone());

        let rel = self.guard.relative(&target)?;
        Ok(AssetImport {
            rel,
            count: count_images(&note),
        })
    }
}

// ---------- 内部工具 ----------

fn is_note_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "txt"))
        .unwrap_or(false)
}

/// 读取文件前 `limit` 字节（B8）。截断的字节流由 `encoding::decode` 容忍——
/// 丢失的尾部字节不会影响 frontmatter/preview(80字)/搜索摘录(500字)。
fn read_prefix(path: &Path, limit: usize) -> Result<Vec<u8>, Error> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; limit];
    let n = f.read(&mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

/// 同目录下是否存在与 `note` 同 stem 的其它笔记文件。
/// `a.md` 与 `a.txt` 共享 `a/assets/`（`with_extension("")` 命名），
/// 删除其中一篇时不能清掉另一篇的图（B4/B7）。
fn has_sibling_same_stem(note: &Path) -> bool {
    let Some(stem) = note.file_stem() else {
        return false;
    };
    let dir = note.parent().unwrap_or_else(|| Path::new("."));
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    rd.flatten().any(|e| {
        let p = e.path();
        p != note && p.is_file() && is_note_file(&p) && p.file_stem() == Some(stem)
    })
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

/// 生成不冲突的图片文件名：`note-YYYYMMDD-<序号>.<ext>`（设计文档 §8 示例）。
fn unique_asset_path(assets_dir: &Path, ext: &str) -> PathBuf {
    let stamp = today_stamp();
    loop {
        let n = IMPORT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = assets_dir.join(format!("note-{stamp}-{n}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
}

/// 当前日期 YYYYMMDD（Hinnant civil_from_days，纯算术无时区依赖）。
fn today_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86400) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}{m:02}{d:02}")
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

    // ---- B4 删除时清理孤儿 assets ----

    #[test]
    fn delete_note_cleans_orphan_assets() {
        let root = temp_root("delassets");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("note.md", "x").unwrap();
        let assets = root.join("note/assets");
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("pic.png"), "png").unwrap();

        store.delete_note("note.md").unwrap();
        assert!(!root.join("note.md").exists());
        assert!(!assets.exists(), "独享的孤儿 assets/ 应一并清理");
        cleanup(&root);
    }

    #[test]
    fn delete_note_keeps_shared_assets_when_sibling_same_stem() {
        let root = temp_root("delshared");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("a.md", "md").unwrap();
        store.write_note("a.txt", "txt").unwrap();
        let assets = root.join("a/assets");
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("pic.png"), "png").unwrap();

        // a.md 与 a.txt 共享 a/assets（B7 现状）；删 a.md 不得清掉 a.txt 的图
        store.delete_note("a.md").unwrap();
        assert!(!root.join("a.md").exists());
        assert!(assets.join("pic.png").is_file(), "同 stem 兄弟共享的 assets 不应被删");
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

    // ---- M3-3 图片导入 ----

    /// 造一张位于根外的源图。
    fn make_src_img(root: &Path, name: &str) -> std::path::PathBuf {
        fs::create_dir_all(root.join("_src")).unwrap();
        let p = root.join(format!("_src/{name}"));
        fs::write(&p, b"\x89PNG fake-bytes").unwrap();
        p
    }

    #[test]
    fn import_asset_copies_and_updates_count() {
        let root = temp_root("import_ok");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("note.md", "# 标题").unwrap();
        let src = make_src_img(&root, "photo.png");

        let out = store.import_asset("note.md", src.to_str().unwrap()).unwrap();
        assert!(out.rel.starts_with("note/assets/note-"), "相对引用: {}", out.rel);
        assert!(out.rel.ends_with(".png"));
        assert!(root.join(&out.rel).is_file(), "图片已复制入 assets");
        assert_eq!(out.count, 1);

        // 列表 image_count 同步更新（M2 卡片「N 图」）
        assert_eq!(store.list_notes().unwrap()[0].image_count, 1);

        // 再次导入不冲突
        let out2 = store.import_asset("note.md", src.to_str().unwrap()).unwrap();
        assert_eq!(out2.count, 2);
        assert_ne!(out.rel, out2.rel);
        cleanup(&root);
    }

    #[test]
    fn import_asset_bytes_png_ok() {
        let root = temp_root("import_bytes");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("n.md", "x").unwrap();
        let out = store.import_asset_bytes("n.md", "snap.PNG", b"fake-png").unwrap();
        assert!(out.rel.ends_with(".png"));
        assert!(root.join(&out.rel).is_file());
        assert_eq!(out.count, 1);
        cleanup(&root);
    }

    #[test]
    fn import_asset_rejects_bad_sources() {
        let root = temp_root("import_bad");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("n.md", "x").unwrap();

        // 源不存在
        assert!(store.import_asset("n.md", root.join("_src/missing.png").to_str().unwrap()).is_err());
        // 非图片扩展名
        let pdf = make_src_img(&root, "doc.pdf");
        assert!(store.import_asset("n.md", pdf.to_str().unwrap()).is_err());
        assert!(store.import_asset_bytes("n.md", "doc.pdf", b"x").is_err());
        // 笔记不存在
        let png = make_src_img(&root, "a.png");
        assert!(store.import_asset("ghost.md", png.to_str().unwrap()).is_err());
        cleanup(&root);
    }

    #[test]
    fn import_asset_rejects_oversize() {
        let root = temp_root("import_big");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("n.md", "x").unwrap();
        // >20MB 文件（路径导入）
        let big = root.join("_src/big.png");
        fs::create_dir_all(root.join("_src")).unwrap();
        fs::write(&big, vec![0u8; MAX_IMAGE_BYTES as usize + 1]).unwrap();
        assert!(store.import_asset("n.md", big.to_str().unwrap()).is_err());
        // >20MB 字节（工具栏选图）
        let huge = vec![0u8; MAX_IMAGE_BYTES as usize + 1];
        assert!(store.import_asset_bytes("n.md", "huge.png", &huge).is_err());
        cleanup(&root);
    }

    #[test]
    fn import_asset_blocks_at_50_images() {
        let root = temp_root("import_50");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("m.md", "x").unwrap();
        let src = make_src_img(&root, "a.png");
        // 直接塞满 50 张
        let assets = root.join("m/assets");
        fs::create_dir_all(&assets).unwrap();
        for i in 0..MAX_IMAGES_PER_NOTE {
            fs::write(assets.join(format!("pre{i}.png")), "x").unwrap();
        }
        assert!(store.import_asset("m.md", src.to_str().unwrap()).is_err(), "达 50 张上限拒绝");
        assert!(store.import_asset_bytes("m.md", "a.png", b"x").is_err());
        cleanup(&root);
    }

    #[test]
    fn note_abs_path_resolves_within_root() {
        let root = temp_root("abspath");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("x.md", "hi").unwrap();
        let abs = store.note_abs_path("x.md").unwrap();
        assert!(std::path::Path::new(&abs).is_file());
        assert!(store.note_abs_path("missing.md").is_err());
        cleanup(&root);
    }

    #[test]
    fn today_stamp_format_is_8_digits() {
        let s = today_stamp();
        assert_eq!(s.len(), 8, "YYYYMMDD: {s}");
        assert!(s.chars().all(|c| c.is_ascii_digit()));
    }

    // ---- B7 自写注册表 ----

    #[test]
    fn own_write_registry_tracks_recent_paths() {
        let reg = OwnWriteRegistry::new();
        let p = PathBuf::from("C:/notes/a.md");
        assert!(!reg.is_own(&p), "未记录前不应判定为自写");
        reg.record(p.clone());
        assert!(reg.is_own(&p));
        assert!(!reg.is_own(&PathBuf::from("C:/notes/b.md")), "其它路径不受影响");
    }
}
