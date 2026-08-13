//! 笔记数据模型，与前端 TS 接口 `NoteMeta` 一一对应（camelCase 序列化）。

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteFormat {
    Md,
    Txt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteMeta {
    /// 相对路径的稳定哈希（十六进制），作为前端 key。
    pub id: String,
    /// frontmatter title，无则取文件名。
    pub title: String,
    /// 相对根目录路径（`/` 分隔）。
    pub path: String,
    pub format: NoteFormat,
    /// 最后修改时间（Unix 毫秒）。
    pub mtime: i64,
    pub image_count: u32,
    pub tags: Vec<String>,
    pub pinned: bool,
    /// 正文首行截断 80 字。
    pub preview: String,
    /// 正文前 500 字，供前端倒排索引（M2 搜索）。
    pub search_text: String,
}

/// `assets_import` 返回结果（M3-3）：图片在根内的相对引用 + 该笔记新图片总数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetImport {
    /// 图片相对根目录路径（`/` 分隔），如 `收件箱/note/assets/note-1.png`。
    pub rel: String,
    /// 导入后该笔记 `assets/` 下的图片总数。
    pub count: u32,
}
