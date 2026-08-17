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

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::core::encoding;
use crate::core::error::Error;
use crate::core::filelock::LockRegistry;
use crate::core::frontmatter;
use crate::core::model::{AssetImport, NoteFormat, NoteMeta, TrashEntry};
use crate::core::pathguard::PathGuard;

/// P0-数据安全：回收站目录名（软删除笔记的存放处，列表扫描/备份时排除）。
const TRASH_DIR: &str = ".trash";
/// 回收站条目元数据文件名（`.trash/<id>/meta.json`）。
const TRASH_META: &str = "meta.json";

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
                // P0-数据安全：回收站目录（.trash）不参与笔记列表扫描
                if entry.file_name().to_string_lossy() == TRASH_DIR {
                    continue;
                }
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

    /// M9：设置笔记 frontmatter 标题（读 → 文本级改 title 行 → 原子写回）。
    pub fn set_note_title(&self, rel: &str, title: &str) -> Result<(), Error> {
        let content = self.read_note(rel)?;
        let updated = frontmatter::set_title(&content, title);
        self.write_note(rel, &updated)
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

    // ---------- P0-数据安全：回收站（软删除/恢复/清空） ----------

    /// 软删除：把笔记移动到 `.trash/<id>/`（保留原文件名 + 独占 assets + meta.json）。
    /// 与 `delete_note`（永久删除）不同，文件仍在磁盘，可 `restore_note` 恢复。
    pub fn trash_note(&self, rel: &str) -> Result<TrashEntry, Error> {
        let path = self.guard.resolve(rel)?;
        if path.is_dir() {
            return Err(Error::InvalidPath(rel.to_string()));
        }
        if !path.is_file() {
            return Err(Error::NotFound(rel.to_string()));
        }
        // 生成条目目录：`.trash/<时间戳>-<序号>/`
        let trash_root = self.root.join(TRASH_DIR);
        std::fs::create_dir_all(&trash_root)?;
        let id = format!(
            "{}-{}",
            modified_millis(&path).unwrap_or(0),
            TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let entry_dir = trash_root.join(&id);
        if entry_dir.exists() {
            // 极低概率碰撞：序号已全局唯一，此处兜底
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "回收站条目已存在",
            )));
        }
        std::fs::create_dir_all(&entry_dir)?;

        // 移动笔记文件（保留原文件名）
        let file_name = path
            .file_name()
            .ok_or_else(|| Error::InvalidPath(rel.to_string()))?;
        let target_note = entry_dir.join(file_name);
        std::fs::rename(&path, &target_note)?;
        self.own.record(target_note.clone());

        // 独占 assets 目录一并移动（无同 stem 兄弟时；共享时留在原地）
        let mut image_count = 0u32;
        let assets = path.with_extension("").join("assets");
        if assets.is_dir() && !has_sibling_same_stem(&path) {
            let target_assets = entry_dir.join("assets");
            std::fs::rename(&assets, &target_assets)?;
            self.own.record(target_assets.clone());
            // 注意：assets 移到 `.trash/<id>/assets`（与笔记文件平级），
            // 不能按 `count_images(&target_note)`（推导 `…/<id>/p/assets`）统计，会得 0。
            image_count = count_images_in(&target_assets);
        }

        // 写元数据（原始路径 + 删除时间 + 标题）
        let bytes = read_prefix(&target_note, META_READ_LIMIT)?;
        let text = encoding::decode(&bytes);
        let fm = frontmatter::parse(&text);
        let stem = target_note
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let meta = TrashMeta {
            original_rel: rel.to_string(),
            deleted_at: now_millis(),
            title: fm.title.clone().unwrap_or(stem),
        };
        std::fs::write(
            entry_dir.join(TRASH_META),
            serde_json::to_string(&meta).map_err(|e| Error::Io(std::io::Error::other(e)))?,
        )?;
        self.own.record(entry_dir.join(TRASH_META));

        let format = note_format(&target_note);
        Ok(TrashEntry {
            id,
            original_rel: meta.original_rel,
            title: meta.title,
            format,
            deleted_at: meta.deleted_at,
            image_count,
        })
    }

    /// 列出回收站全部条目（按删除时间倒序）。
    pub fn list_trash(&self) -> Result<Vec<TrashEntry>, Error> {
        let trash_root = self.root.join(TRASH_DIR);
        let rd = match std::fs::read_dir(&trash_root) {
            Ok(rd) => rd,
            Err(_) => return Ok(Vec::new()), // 回收站目录不存在 = 空
        };
        let mut out = Vec::new();
        for entry in rd.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let meta_path = dir.join(TRASH_META);
            if !meta_path.is_file() {
                continue; // 无 meta 的残留目录，忽略
            }
            let raw = match std::fs::read_to_string(&meta_path) {
                Ok(raw) => raw,
                Err(_) => continue,
            };
            let meta: TrashMeta = match serde_json::from_str(&raw) {
                Ok(m) => m,
                Err(_) => continue,
            };
            // 找条目内笔记文件（可能有 .md/.txt）
            let Some(note) = find_note_file(&dir) else {
                continue;
            };
            let id = entry.file_name().to_string_lossy().into_owned();
            out.push(TrashEntry {
                id,
                original_rel: meta.original_rel,
                title: meta.title,
                format: note_format(&note),
                deleted_at: meta.deleted_at,
                // assets 在 `.trash/<id>/assets`（与笔记文件平级），直接统计该目录
                image_count: count_images_in(&dir.join("assets")),
            });
        }
        out.sort_by_key(|b| std::cmp::Reverse(b.deleted_at));
        Ok(out)
    }

    /// 恢复：把 `.trash/<id>/` 里的笔记移回原路径（含独占 assets）。
    /// 原路径已存在文件时返回 `Error::RestoreConflict`。
    pub fn restore_note(&self, id: &str) -> Result<(), Error> {
        let entry_dir = self.trash_entry_dir(id)?;
        let meta_path = entry_dir.join(TRASH_META);
        let raw = std::fs::read_to_string(&meta_path)?;
        let meta: TrashMeta =
            serde_json::from_str(&raw).map_err(|e| Error::Io(std::io::Error::other(e)))?;
        let target = self.guard.resolve(&meta.original_rel)?;
        if target.exists() {
            return Err(Error::RestoreConflict(meta.original_rel));
        }
        let Some(note) = find_note_file(&entry_dir) else {
            return Err(Error::NotFound(id.to_string()));
        };
        // 移动笔记文件
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&note, &target)?;
        self.own.record(target.clone());
        // 移动独占 assets
        let entry_assets = entry_dir.join("assets");
        if entry_assets.is_dir() {
            let target_assets = target.with_extension("").join("assets");
            std::fs::create_dir_all(&target_assets)?;
            std::fs::rename(&entry_assets, &target_assets)?;
            self.own.record(target_assets.clone());
        }
        // 清理条目目录（meta 已无用）
        let _ = std::fs::remove_file(&meta_path);
        let _ = std::fs::remove_dir(&entry_dir);
        Ok(())
    }

    /// 永久删除单个回收站条目（彻底删除，不可恢复）。
    pub fn purge_note(&self, id: &str) -> Result<(), Error> {
        let entry_dir = self.trash_entry_dir(id)?;
        std::fs::remove_dir_all(&entry_dir)?;
        Ok(())
    }

    /// 清空回收站（全部永久删除）。返回删除的条目数。
    pub fn empty_trash(&self) -> Result<usize, Error> {
        let trash_root = self.root.join(TRASH_DIR);
        let rd = match std::fs::read_dir(&trash_root) {
            Ok(rd) => rd,
            Err(_) => return Ok(0),
        };
        let mut n = 0usize;
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                std::fs::remove_dir_all(&p)?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// 解析 `.trash/<id>` 路径：校验 id 无路径分隔符（防穿越），返回条目目录。
    fn trash_entry_dir(&self, id: &str) -> Result<PathBuf, Error> {
        if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
            return Err(Error::InvalidPath(id.to_string()));
        }
        let dir = self.root.join(TRASH_DIR).join(id);
        if !dir.is_dir() {
            return Err(Error::NotFound(id.to_string()));
        }
        Ok(dir)
    }

    // ---------- P0-数据安全：一键备份 / 恢复（zip） ----------

    /// 把整个笔记根目录（排除 `.trash` 回收站与临时文件）打包为 zip 写到 `target_path`。
    /// 返回写入的文件数。zip 结构 = 根目录内容（不含根目录本身一层）。
    pub fn backup_all(&self, target_path: &str) -> Result<usize, Error> {
        let file = std::fs::File::create(target_path)?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        let mut count = 0usize;
        self.zip_dir(&self.root, &mut zip, options, "", &mut count)?;
        zip.finish().map_err(|e| Error::Io(std::io::Error::other(e)))?;
        Ok(count)
    }

    /// 递归把目录内容写入 zip（前缀 `prefix` 为 zip 内相对路径，`""` 表示根）。
    fn zip_dir(
        &self,
        dir: &Path,
        zip: &mut ZipWriter<std::fs::File>,
        options: SimpleFileOptions,
        prefix: &str,
        count: &mut usize,
    ) -> Result<(), Error> {
        let rd = std::fs::read_dir(dir)?;
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            // 排除回收站与原子写临时文件（`.xxx.tmp`）
            if name == TRASH_DIR || (name.starts_with('.') && name.ends_with(".tmp")) {
                continue;
            }
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if path.is_dir() {
                zip.add_directory(&rel, options)
                    .map_err(|e| Error::Io(std::io::Error::other(e)))?;
                self.zip_dir(&path, zip, options, &rel, count)?;
            } else if path.is_file() {
                zip.start_file(&rel, options)
                    .map_err(|e| Error::Io(std::io::Error::other(e)))?;
                let mut f = std::fs::File::open(&path)?;
                std::io::copy(&mut f, zip).map_err(Error::Io)?;
                *count += 1;
            }
        }
        Ok(())
    }

    /// 从备份 zip 恢复：把 zip 内文件解压回根目录（覆盖同名；目录结构与 zip 一致）。
    /// 安全：每个条目路径都经 `PathGuard::resolve` 校验（防 zip-slip `../` 逃逸）。
    /// 返回解压的文件数。
    pub fn restore_backup(&self, source_path: &str) -> Result<usize, Error> {
        let file = std::fs::File::open(source_path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;
        let mut count = 0usize;
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| Error::Io(std::io::Error::other(e)))?;
            let name = entry.name().to_string();
            if name.ends_with('/') || entry.is_dir() {
                continue; // 目录条目由文件写入时的 create_dir_all 隐式创建
            }
            // 防 zip-slip：路径必须解析到根内
            let abs = self.guard.resolve(&name)?;
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&abs)?;
            std::io::copy(&mut entry, &mut out)?;
            self.own.record(abs);
            count += 1;
        }
        Ok(count)
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

/// 回收站条目元数据（`.trash/<id>/meta.json`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrashMeta {
    /// 被删前的原始相对路径。
    original_rel: String,
    /// 删除时间（Unix 毫秒）。
    deleted_at: i64,
    /// 展示标题（frontmatter title 或文件名）。
    title: String,
}

/// 当前时间（Unix 毫秒）。
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 在目录内查找第一个笔记文件（.md/.txt）。
fn find_note_file(dir: &Path) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    rd.flatten().find_map(|e| {
        let p = e.path();
        (p.is_file() && is_note_file(&p)).then_some(p)
    })
}

/// 根据扩展名判断笔记格式（默认 Txt）。
fn note_format(path: &Path) -> NoteFormat {
    if matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()),
        Some(e) if e == "md"
    ) {
        NoteFormat::Md
    } else {
        NoteFormat::Txt
    }
}

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
    count_images_in(&note_path.with_extension("").join("assets"))
}

/// 图片计数：直接统计给定 assets 目录下图片扩展名的文件数。
/// 回收站条目的 assets 在 `.trash/<id>/assets`（与笔记文件平级，非 `<stem>/assets`），
/// 需要按实际目录统计，因此单独抽出此函数。
fn count_images_in(assets: &Path) -> u32 {
    let rd = match std::fs::read_dir(assets) {
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
    fn set_note_title_updates_frontmatter_keeps_others() {
        let root = temp_root("set_title");
        let store = FsStore::new(root.clone()).unwrap();
        store
            .write_note("a.md", "---\ntitle: 旧\ncreated: x\n---\n正文")
            .unwrap();
        store.set_note_title("a.md", "新").unwrap();
        let content = store.read_note("a.md").unwrap();
        assert!(content.contains("title: 新"));
        assert!(!content.contains("旧"));
        assert!(content.contains("created: x"), "未知键保留");
        assert!(content.contains("正文"));
        cleanup(&root);
    }

    #[test]
    fn set_note_title_inserts_frontmatter_when_none() {
        let root = temp_root("set_title_none");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("b.txt", "纯文本内容").unwrap();
        store.set_note_title("b.txt", "标题").unwrap();
        assert_eq!(
            store.read_note("b.txt").unwrap(),
            "---\ntitle: 标题\n---\n纯文本内容"
        );
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

    // ---- P0-数据安全：回收站 ----

    #[test]
    fn trash_soft_deletes_and_restores() {
        let root = temp_root("trash_roundtrip");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("收件箱/a.md", "---\ntitle: 我的笔记\n---\n正文").unwrap();

        // 软删除：文件移入 .trash，原位置消失，列表为空
        let t = store.trash_note("收件箱/a.md").unwrap();
        assert_eq!(t.original_rel, "收件箱/a.md");
        assert_eq!(t.title, "我的笔记");
        assert!(!root.join("收件箱/a.md").exists());
        assert!(store.list_notes().unwrap().is_empty(), "回收站条目不进列表");
        assert_eq!(store.list_trash().unwrap().len(), 1);

        // 恢复：回到原路径，回收站清空
        store.restore_note(&t.id).unwrap();
        assert!(root.join("收件箱/a.md").is_file());
        assert!(store.list_trash().unwrap().is_empty());
        assert_eq!(store.read_note("收件箱/a.md").unwrap(), "---\ntitle: 我的笔记\n---\n正文");
        cleanup(&root);
    }

    #[test]
    fn trash_keeps_assets_and_counts_them() {
        let root = temp_root("trash_assets");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("p.md", "x").unwrap();
        let src = make_src_img(&root, "photo.png");
        store.import_asset("p.md", src.to_str().unwrap()).unwrap();
        assert!(root.join("p/assets").is_dir());

        let t = store.trash_note("p.md").unwrap();
        assert_eq!(t.image_count, 1);
        assert!(!root.join("p/assets").exists(), "独占 assets 随笔记移入回收站");
        assert!(!root.join("p.md").exists());

        store.restore_note(&t.id).unwrap();
        assert!(root.join("p/assets").is_dir(), "assets 一并恢复");
        assert_eq!(store.list_notes().unwrap()[0].image_count, 1);
        cleanup(&root);
    }

    #[test]
    fn trash_restore_conflict_when_original_exists() {
        let root = temp_root("trash_conflict");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("a.md", "旧内容").unwrap();
        let t = store.trash_note("a.md").unwrap();
        // 原路径被新文件占用
        store.write_note("a.md", "新内容").unwrap();
        assert!(matches!(
            store.restore_note(&t.id),
            Err(Error::RestoreConflict(_))
        ));
        cleanup(&root);
    }

    #[test]
    fn trash_purge_and_empty() {
        let root = temp_root("trash_purge");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("a.md", "x").unwrap();
        store.write_note("b.md", "y").unwrap();
        let ta = store.trash_note("a.md").unwrap();
        let tb = store.trash_note("b.md").unwrap();

        // 永久删除单个
        store.purge_note(&ta.id).unwrap();
        assert_eq!(store.list_trash().unwrap().len(), 1);

        // 清空全部
        let n = store.empty_trash().unwrap();
        assert_eq!(n, 1);
        assert!(store.list_trash().unwrap().is_empty());
        let _ = tb;
        cleanup(&root);
    }

    #[test]
    fn trash_rejects_path_traversal_ids() {
        let root = temp_root("trash_traversal");
        let store = FsStore::new(root.clone()).unwrap();
        for bad in ["../x", "a/b", "a\\b", "..", "..\\x"] {
            assert!(store.restore_note(bad).is_err(), "应拒绝: {bad}");
            assert!(store.purge_note(bad).is_err(), "应拒绝: {bad}");
        }
        cleanup(&root);
    }

    #[test]
    fn trash_dir_excluded_from_notes_list() {
        let root = temp_root("trash_excluded");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("a.md", "x").unwrap();
        store.trash_note("a.md").unwrap();
        // 手动在 .trash 里再放一个 .md 文件，也不应进入列表
        fs::create_dir_all(root.join(".trash/zzz")).unwrap();
        fs::write(root.join(".trash/zzz/leak.md"), "x").unwrap();
        assert!(store.list_notes().unwrap().is_empty(), ".trash 内文件不进笔记列表");
        cleanup(&root);
    }

    // ---- P0-数据安全：备份 / 恢复 ----

    #[test]
    fn backup_all_roundtrips_notes_and_assets() {
        let root = temp_root("backup_roundtrip");
        let store = FsStore::new(root.clone()).unwrap();
        store.write_note("收件箱/a.md", "---\ntitle: 甲\n---\n正文一").unwrap();
        store.write_note("b.txt", "纯文本").unwrap();
        let src = make_src_img(&root, "p.png");
        store.import_asset("b.txt", src.to_str().unwrap()).unwrap();
        // 回收站条目不应进备份
        store.write_note("t.md", "trash-me").unwrap();
        store.trash_note("t.md").unwrap();

        let backup = root.join("backup.zip");
        let n = store.backup_all(backup.to_str().unwrap()).unwrap();
        assert!(n >= 3, "a.md + b.txt + b/assets 图片: {n}");
        assert!(backup.is_file());

        // 恢复到一个全新根目录（模拟还原）
        let root2 = temp_root("backup_restore");
        let store2 = FsStore::new(root2.clone()).unwrap();
        let m = store2.restore_backup(backup.to_str().unwrap()).unwrap();
        assert_eq!(m, n);
        let notes = store2.list_notes().unwrap();
        assert_eq!(notes.len(), 2, "恢复后 2 篇笔记（回收站条目不恢复）");
        assert!(root2.join("收件箱/a.md").is_file());
        assert_eq!(store2.read_note("收件箱/a.md").unwrap(), "---\ntitle: 甲\n---\n正文一");
        assert!(root2.join("b/assets").is_dir(), "图片随笔记恢复");
        cleanup(&root);
        cleanup(&root2);
    }

    #[test]
    fn restore_backup_rejects_zip_slip() {
        // 构造含 `../evil.md` 条目的 zip，恢复必须被拒绝
        let root = temp_root("zip_slip");
        let store = FsStore::new(root.clone()).unwrap();
        let zip_path = root.join("evil.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.start_file("../evil.md", options).unwrap();
        use std::io::Write;
        zip.write_all(b"pwned").unwrap();
        zip.finish().unwrap();
        assert!(store.restore_backup(zip_path.to_str().unwrap()).is_err(), "zip-slip 越界应拒绝");
        assert!(!root.join("evil.md").exists());
        cleanup(&root);
    }
}
