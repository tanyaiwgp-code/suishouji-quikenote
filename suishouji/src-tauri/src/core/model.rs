//! 笔记数据模型，与前端 TS 接口 `NoteMeta` 一一对应（camelCase 序列化）。
//! `TS` derive 生成 `types.gen.ts`（`#[ts(export_to)]` + `export_typescript_bindings` 测试），
//! 消除前后端手写镜像漂移 —— 改字段/序列化名后跑 `cargo test` 即同步前端类型。

use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export_to = "types.gen.ts")]
pub enum NoteFormat {
    Md,
    Txt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "types.gen.ts")]
pub struct NoteMeta {
    /// 相对路径的稳定哈希（十六进制），作为前端 key。
    pub id: String,
    /// frontmatter title，无则取文件名。
    pub title: String,
    /// 相对根目录路径（`/` 分隔）。
    pub path: String,
    pub format: NoteFormat,
    /// 最后修改时间（Unix 毫秒）。
    #[ts(type = "number")] // i64 默认生成 bigint，与前端 `mtime: number` 对齐
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "types.gen.ts")]
pub struct AssetImport {
    /// 图片相对根目录路径（`/` 分隔），如 `收件箱/note/assets/note-1.png`。
    pub rel: String,
    /// 导入后该笔记 `assets/` 下的图片总数。
    pub count: u32,
}

#[cfg(test)]
mod ts_export {
    use super::*;

    /// 生成 `suishouji/src/lib/types.gen.ts`（相对 CARGO_MANIFEST_DIR=src-tauri 的 `../src/lib`）。
    /// `NoteMeta::export_all` 会递归导出依赖 `NoteFormat`；`AssetImport` 独立导出。
    /// 运行 `cargo test` 即重写前端类型文件（入库）。
    #[test]
    fn export_typescript_bindings() {
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib");
        let cfg = ts_rs::Config::new().with_out_dir(&out);
        NoteMeta::export_all(&cfg).expect("导出 NoteMeta 到 types.gen.ts");
        AssetImport::export_all(&cfg).expect("导出 AssetImport 到 types.gen.ts");
    }
}

