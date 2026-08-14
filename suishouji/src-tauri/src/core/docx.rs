//! M5 DOCX 导入导出（纯 Rust，零 Tauri 依赖，全部可单测）。
//!
//! # 安全验收（M5）
//! - **防 ZIP 炸弹**：解压前遍历全部条目，`uncompressed_size` 总和 ≤ 50MB、条目 ≤ 1000、单文件 ≤ 20MB。
//! - **防 XXE**：`roxmltree` 用 `allow_dtd: false` 解析（不加载/展开 DTD 与外部实体）。
//! - **图片不发起网络请求**：只从 zip 内的 `word/media/*` 按扩展名白名单提取，绝不按 rels 的 URL 拉取。
//!
//! 边界 case 降级策略（实施方案 §M5）：标题/段落/图片/表格精转，其余（嵌套表格/域/脚注等）降级为纯文本。

use crate::core::error::Error;
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

// ---------- 常量 ----------

/// 防 ZIP 炸弹：解压后总字节上限。
const MAX_ZIP_TOTAL: u64 = 50 * 1024 * 1024;
/// 防条目洪泛：zip 条目数上限。
const MAX_ENTRIES: usize = 1000;
/// 单条目（单文件）字节上限。
const MAX_SINGLE: u64 = 20 * 1024 * 1024;
/// 图片扩展名白名单（与 store.rs `IMAGE_EXTS` 一致）。
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PKG_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const PIC_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

// ---------- 数据结构 ----------

/// DOCX 导入的单张图片（占位符在 markdown 中，命令层替换为真实 assets 相对引用）。
#[derive(Debug)]
pub struct ImportedImage {
    pub placeholder: String,
    pub bytes: Vec<u8>,
    pub ext: String,
}

/// DOCX 导入结果。
#[derive(Debug)]
pub struct ImportedNote {
    pub markdown: String,
    pub images: Vec<ImportedImage>,
    /// 文档第一个标题（无则为空）。
    pub title: String,
}

// ---------- 导入：DOCX → Markdown ----------

/// 解析 DOCX 字节为 Markdown + 图片列表。
pub fn import(bytes: &[u8]) -> Result<ImportedNote, Error> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| Error::ImportFailed(format!("无法读取 docx（zip）：{e}")))?;

    // 防 ZIP 炸弹：先遍历条目元数据，不解压内容
    if archive.len() > MAX_ENTRIES {
        return Err(Error::ImportTooLarge(format!(
            "zip 条目数 {} 超限（{MAX_ENTRIES}）",
            archive.len()
        )));
    }
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let f = archive
            .by_index(i)
            .map_err(|e| Error::ImportFailed(e.to_string()))?;
        let sz = f.size();
        if sz > MAX_SINGLE {
            return Err(Error::ImportTooLarge(format!("单文件 {sz} 字节超限")));
        }
        total = total.saturating_add(sz);
        if total > MAX_ZIP_TOTAL {
            return Err(Error::ImportTooLarge("解压后总大小超 50MB".into()));
        }
    }

    let doc_bytes = read_zip_entry(&mut archive, "word/document.xml")?
        .ok_or_else(|| Error::UnsupportedFormat("缺少 word/document.xml".into()))?;
    let doc_str = String::from_utf8(doc_bytes)
        .map_err(|e| Error::ImportFailed(format!("document.xml 非 UTF-8：{e}")))?;
    let rels_bytes = read_zip_entry(&mut archive, "word/_rels/document.xml.rels")?.unwrap_or_default();

    let parsed = roxmltree::Document::parse_with_options(
        &doc_str,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            ..Default::default()
        },
    )
    .map_err(|e| Error::ImportFailed(format!("document.xml 解析失败：{e}")))?;
    let rel_map = parse_rels(&rels_bytes)?;

    let root = parsed.root_element();
    let body = root
        .children()
        .find(|n| n.is_element() && n.tag_name().namespace() == Some(W_NS) && n.tag_name().name() == "body")
        .ok_or_else(|| Error::UnsupportedFormat("缺少 w:body".into()))?;

    let mut md = String::new();
    let mut images: Vec<ImportedImage> = Vec::new();
    let mut title = String::new();
    for child in body.children().filter(|n| n.is_element()) {
        let ns = child.tag_name().namespace();
        let name = child.tag_name().name();
        if ns == Some(W_NS) && name == "p" {
            if title.is_empty() {
                let lvl = heading_level(&child);
                if lvl > 0 {
                    title = paragraph_text(&child).trim().to_string();
                }
            }
            push_paragraph(&mut md, &mut images, &child, &rel_map, &mut archive)?;
        } else if ns == Some(W_NS) && name == "tbl" {
            push_table(&mut md, &child);
        }
    }

    Ok(ImportedNote {
        markdown: md,
        images,
        title,
    })
}

/// 读 zip 内条目字节；不存在返回 `None`。
fn read_zip_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<Vec<u8>>, Error> {
    match archive.by_name(name) {
        Ok(mut f) => {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)
                .map_err(|e| Error::ImportFailed(e.to_string()))?;
            Ok(Some(buf))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(e) => Err(Error::ImportFailed(e.to_string())),
    }
}

/// 解析 `word/_rels/document.xml.rels` → `rId → Target`（media 路径）。
fn parse_rels(rels_bytes: &[u8]) -> Result<HashMap<String, String>, Error> {
    let mut map = HashMap::new();
    if rels_bytes.is_empty() {
        return Ok(map);
    }
    let s = String::from_utf8_lossy(rels_bytes);
    let doc = roxmltree::Document::parse_with_options(
        &s,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            ..Default::default()
        },
    )
    .map_err(|e| Error::ImportFailed(format!("rels 解析失败：{e}")))?;
    for el in doc
        .root_element()
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
    {
        let id = el.attribute("Id").unwrap_or("");
        let target = el.attribute("Target").unwrap_or("");
        if !id.is_empty() && !target.is_empty() {
            map.insert(id.to_string(), target.to_string());
        }
    }
    Ok(map)
}

/// 段落标题级别（Heading1-3 → 1-3，非标题 → 0）。
fn heading_level(p: &roxmltree::Node) -> usize {
    let val = p
        .descendants()
        .find(|n| {
            n.is_element()
                && n.tag_name().namespace() == Some(W_NS)
                && n.tag_name().name() == "pStyle"
        })
        .and_then(|s| s.attribute((W_NS, "val")))
        .map(|v| v.to_ascii_lowercase());
    if let Some(v) = val {
        if v.contains("heading") {
            let digits: String = v.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<usize>() {
                if (1..=3).contains(&n) {
                    return n;
                }
            }
        }
    }
    0
}

/// 段落内全部文本（w:t 拼接，w:br → 换行）。
fn paragraph_text(p: &roxmltree::Node) -> String {
    let mut s = String::new();
    for d in p.descendants() {
        if !d.is_element() || d.tag_name().namespace() != Some(W_NS) {
            continue;
        }
        match d.tag_name().name() {
            "t" => {
                if let Some(t) = d.text() {
                    s.push_str(t);
                }
            }
            "br" | "cr" => s.push('\n'),
            _ => {}
        }
    }
    s
}

/// media target 是否在白名单图片扩展名内。
fn ext_of_media(target: &str) -> Option<&'static str> {
    let ext = target.rsplit('.').next()?.to_ascii_lowercase();
    IMAGE_EXTS.iter().find(|e| **e == ext).copied()
}

/// 段落 → markdown（文本行 / 图片占位符 / 标题）。
fn push_paragraph(
    md: &mut String,
    images: &mut Vec<ImportedImage>,
    p: &roxmltree::Node,
    rels: &HashMap<String, String>,
    archive: &mut ZipArchive<Cursor<&[u8]>>,
) -> Result<(), Error> {
    let heading = heading_level(p);
    let text = paragraph_text(p);
    let mut img_lines: Vec<String> = Vec::new();
    for blip in p.descendants().filter(|n| {
        n.is_element()
            && n.tag_name().namespace() == Some(A_NS)
            && n.tag_name().name() == "blip"
    }) {
        let rid = blip
            .attribute((R_NS, "embed"))
            .or_else(|| blip.attribute((R_NS, "link")));
        let Some(rid) = rid else { continue };
        let Some(target) = rels.get(rid) else { continue };
        let Some(ext) = ext_of_media(target) else { continue };
        if let Some(bytes) = read_zip_entry(archive, &format!("word/{target}"))? {
            let placeholder = format!("@@IMG{}@@", images.len());
            images.push(ImportedImage {
                placeholder: placeholder.clone(),
                bytes,
                ext: ext.to_string(),
            });
            img_lines.push(format!("![]({placeholder})"));
        }
    }

    if heading > 0 {
        let t = text.trim();
        if !t.is_empty() {
            md.push_str(&"#".repeat(heading));
            md.push(' ');
            md.push_str(t);
            md.push('\n');
        }
        md.push('\n');
    } else if !img_lines.is_empty() {
        for line in img_lines {
            md.push_str(&line);
            md.push('\n');
        }
        md.push('\n');
        let t = text.trim();
        if !t.is_empty() {
            md.push_str(t);
            md.push('\n');
            md.push('\n');
        }
    } else if text.trim().is_empty() {
        md.push('\n');
    } else {
        md.push_str(text.trim());
        md.push('\n');
        md.push('\n');
    }
    Ok(())
}

/// 单元格文本（w:t 拼接）。
fn cell_text(tc: &roxmltree::Node) -> String {
    let mut s = String::new();
    for d in tc.descendants() {
        if d.is_element()
            && d.tag_name().namespace() == Some(W_NS)
            && d.tag_name().name() == "t"
        {
            if let Some(t) = d.text() {
                s.push_str(t);
            }
        }
    }
    s
}

/// 表格 → Markdown 表格（第一行作表头，单元格内 `|` 转义）。
fn push_table(md: &mut String, tbl: &roxmltree::Node) {
    let mut rows: Vec<Vec<String>> = Vec::new();
    for tr in tbl.children().filter(|n| {
        n.is_element() && n.tag_name().namespace() == Some(W_NS) && n.tag_name().name() == "tr"
    }) {
        let mut row = Vec::new();
        for tc in tr.children().filter(|n| {
            n.is_element() && n.tag_name().namespace() == Some(W_NS) && n.tag_name().name() == "tc"
        }) {
            row.push(cell_text(&tc));
        }
        if !row.is_empty() {
            rows.push(row);
        }
    }
    let Some(first) = rows.first() else { return };
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return;
    }
    let render = |cells: &[String]| {
        let mut line = String::from("|");
        for i in 0..width {
            let c = cells.get(i).map(|s| s.trim()).unwrap_or("");
            line.push(' ');
            line.push_str(&c.replace('|', "\\|"));
            line.push_str(" |");
        }
        line
    };
    md.push_str(&render(first));
    md.push('\n');
    md.push_str(&format!("|{}|", " --- |".repeat(width)));
    md.push('\n');
    for row in rows.iter().skip(1) {
        md.push_str(&render(row));
        md.push('\n');
    }
    md.push('\n');
}

// ---------- 导出：Markdown → DOCX ----------

/// 把 Markdown 文本转为 DOCX 字节。
/// `images` 元素为 `(源文件名/路径, 字节)`，与 markdown 中 `![...](...)` 的出现顺序一一对应；
/// 扩展名从源名派生，图片写入 `word/media/image{n}.<ext>` 并嵌入文档。
pub fn export(markdown: &str, images: &[(&str, &[u8])]) -> Result<Vec<u8>, Error> {
    let (document_xml, doc_rels_xml, media) = build_document_xml(markdown, images);

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let files = [
        ("[Content_Types].xml", content_types_xml(&media)),
        ("_rels/.rels", root_rels_xml()),
        ("word/document.xml", document_xml),
        ("word/_rels/document.xml.rels", doc_rels_xml),
    ];
    for (path, content) in files {
        writer
            .start_file(path, SimpleFileOptions::default())
            .map_err(|e| Error::ExportFailed(e.to_string()))?;
        writer
            .write_all(content.as_bytes())
            .map_err(|e| Error::ExportFailed(e.to_string()))?;
    }
    for (name, bytes) in &media {
        let path = format!("word/media/{name}");
        writer
            .start_file(&path, SimpleFileOptions::default())
            .map_err(|e| Error::ExportFailed(e.to_string()))?;
        writer
            .write_all(bytes)
            .map_err(|e| Error::ExportFailed(e.to_string()))?;
    }
    let cursor = writer
        .finish()
        .map_err(|e| Error::ExportFailed(e.to_string()))?;
    Ok(cursor.into_inner())
}

/// 块级 markdown → OOXML。返回 (document.xml, document.xml.rels, media 列表)。
fn build_document_xml(
    md: &str,
    images: &[(&str, &[u8])],
) -> (String, String, Vec<(String, Vec<u8>)>) {
    let mut body = String::new();
    let mut media: Vec<(String, Vec<u8>)> = Vec::new();
    let mut img_idx = 0usize;
    let mut in_code = false;
    let mut code_buf: Vec<String> = Vec::new();

    let mut lines = md.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if in_code {
                // 结束代码块：等宽段落
                for cl in &code_buf {
                    body.push_str(&para(&run(cl, false, true), None));
                }
                code_buf.clear();
                in_code = false;
            } else {
                in_code = true;
            }
            continue;
        }
        if in_code {
            code_buf.push(line.to_string());
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if let Some(level) = heading_line(trimmed) {
            let text = heading_text(trimmed);
            body.push_str(&para(&run(&text, false, false), Some(level)));
            continue;
        }
        if trimmed.starts_with('|') && trimmed.contains('|') && !is_table_separator(trimmed) {
            let mut rows: Vec<String> = vec![trimmed.to_string()];
            while let Some(nx) = lines.peek() {
                let nt = nx.trim();
                if nt.starts_with('|') && nt.contains('|') && !is_table_separator(nt) {
                    rows.push(nt.to_string());
                    lines.next();
                } else {
                    break;
                }
            }
            body.push_str(&table_xml(&rows));
            continue;
        }
        if let Some(src) = image_line(trimmed) {
            if img_idx < images.len() {
                let (src_hint, bytes) = &images[img_idx];
                let ext = ext_from_src(src_hint).unwrap_or("png");
                let fname = format!("image{}.{ext}", img_idx + 1);
                media.push((fname.clone(), bytes.to_vec()));
                body.push_str(&image_para(&fname, img_idx + 1));
                img_idx += 1;
            } else if !src.is_empty() {
                // 占位但无图片字节：仅输出 alt 文本，避免坏引用
                body.push_str(&para(&run(&src, false, false), None));
            }
            continue;
        }
        // 普通段落 / 列表行（保留前缀文本）
        body.push_str(&para(&run(trimmed, false, false), None));
    }

    let document_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{W_NS}" xmlns:r="{R_NS}" xmlns:a="{A_NS}" xmlns:pic="{PIC_NS}" xmlns:wp="{WP_NS}"><w:body>{body}</w:body></w:document>"#
    );

    // 文档级 rels：图片 rId1..N
    let mut doc_rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PKG_REL_NS}">"#
    );
    for (i, (name, _)) in media.iter().enumerate() {
        doc_rels.push_str(&format!(
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/{name}"/>"#,
            i + 1
        ));
    }
    doc_rels.push_str("</Relationships>");

    (document_xml, doc_rels, media)
}

/// 标题行级别（`#`/`##`/`###` → 1-3，其它 → None）。允许 `#标题`（无空格）。
fn heading_line(trimmed: &str) -> Option<usize> {
    if !trimmed.starts_with('#') {
        return None;
    }
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=3).contains(&level) {
        return None;
    }
    if trimmed.chars().skip(level).collect::<String>().trim().is_empty() {
        return None;
    }
    Some(level)
}

fn heading_text(trimmed: &str) -> String {
    trimmed.trim_start_matches('#').trim().to_string()
}

/// 表格分隔行（`| --- | --- |`）。
fn is_table_separator(line: &str) -> bool {
    let core: String = line.chars().filter(|c| *c != '|').collect();
    let core = core.trim();
    !core.is_empty() && core.chars().all(|c| c == '-' || c == ':' || c == ' ')
}

/// 图片行 `![alt](src)` → src。
fn image_line(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix("![")?;
    let src = rest.split("](").nth(1)?;
    let src = src.strip_suffix(')')?;
    Some(src.to_string())
}

/// 从源文件名派生扩展名。
fn ext_from_src(src: &str) -> Option<&'static str> {
    let ext = src.rsplit('.').next()?.to_ascii_lowercase();
    IMAGE_EXTS.iter().find(|e| **e == ext).copied()
}

/// 表格行数组 → w:tbl XML。
fn table_xml(rows: &[String]) -> String {
    let mut s = String::from(
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblBorders>\
         <w:top w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/>\
         <w:left w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/>\
         <w:bottom w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/>\
         <w:right w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/>\
         <w:insideH w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/>\
         <w:insideV w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/>\
         </w:tblBorders></w:tblPr>",
    );
    for row in rows {
        s.push_str("<w:tr>");
        for cell in row.split('|') {
            let cell = cell.trim();
            if cell.is_empty() {
                continue;
            }
            s.push_str("<w:tc><w:tcPr><w:tcW w:w=\"0\" w:type=\"auto\"/></w:tcPr>");
            s.push_str(&para(&run(cell, false, false), None));
            s.push_str("</w:tc>");
        }
        s.push_str("</w:tr>");
    }
    s.push_str("</w:tbl>");
    s
}

/// 单个文本 run（可选加粗 / 等宽）。
fn run(text: &str, bold: bool, mono: bool) -> String {
    let mut s = String::from("<w:r>");
    if bold || mono {
        s.push_str("<w:rPr>");
        if bold {
            s.push_str("<w:b/>");
        }
        if mono {
            s.push_str(r#"<w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/>"#);
        }
        s.push_str("</w:rPr>");
    }
    s.push_str(&format!(r#"<w:t xml:space="preserve">{}</w:t>"#, xml_escape(text)));
    s.push_str("</w:r>");
    s
}

/// 段落（可选标题样式）。
fn para(runs: &str, heading: Option<usize>) -> String {
    let mut s = String::from("<w:p>");
    if let Some(h) = heading {
        s.push_str(&format!(r#"<w:pPr><w:pStyle w:val="Heading{h}"/></w:pPr>"#));
    }
    s.push_str(runs);
    s.push_str("</w:p>");
    s
}

/// 图片段落（固定 5cm×5cm 占位，Word 内可调）。
fn image_para(media_name: &str, id: usize) -> String {
    format!(
        r#"<w:p><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0">
<wp:extent cx="5486400" cy="5486400"/><wp:docPr id="{id}" name="{media_name}"/>
<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">
<pic:pic><pic:nvPicPr><pic:cNvPr id="{id}" name="{media_name}"/><pic:cNvPicPr/></pic:nvPicPr>
<pic:blipFill><a:blip r:embed="rId{id}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>
<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="5486400" cy="5486400"/></a:xfrm>
<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>
</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
    )
}

/// `[Content_Types].xml`（含图片扩展名 Default）。
fn content_types_xml(media: &[(String, Vec<u8>)]) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>"#,
    );
    for (name, _) in media {
        if let Some(ext) = name.rsplit('.').next() {
            let ct = match ext {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                "bmp" => "image/bmp",
                "svg" => "image/svg+xml",
                _ => continue,
            };
            s.push_str(&format!(
                r#"<Default Extension="{ext}" ContentType="{ct}"/>"#
            ));
        }
    }
    s.push_str(
        r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
    );
    s
}

/// `_rels/.rels`（指向 document.xml）。
fn root_rels_xml() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PKG_REL_NS}">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#
    )
}

/// XML 转义。
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试里手工构造最小 docx（含标题/段落/表格/图片引用）。
    fn make_docx(document_xml: &str, with_media: bool) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default();
        let mut entries: Vec<(&str, Vec<u8>)> = vec![
            (
                "[Content_Types].xml",
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_vec(),
            ),
            (
                "_rels/.rels",
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec(),
            ),
            (
                "word/document.xml",
                document_xml.as_bytes().to_vec(),
            ),
        ];
        // 文档级 rels：with_media 时给出 rId5 → media/image1.png（供图片提取）
        let rels_xml = if with_media {
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/></Relationships>"#.to_vec()
        } else {
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>"#.to_vec()
        };
        entries.push(("word/_rels/document.xml.rels", rels_xml));
        for (name, content) in entries {
            writer.start_file(name, opts).unwrap();
            writer.write_all(&content).unwrap();
        }
        if with_media {
            writer.start_file("word/media/image1.png", opts).unwrap();
            writer.write_all(b"\x89PNG\x0d\x0a\x1a\x0a").unwrap();
        }
        let cursor = writer.finish().unwrap();
        cursor.into_inner()
    }

    const DOC_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing">
<w:body>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>会议纪要</w:t></w:r></w:p>
<w:p><w:r><w:t>第一段正文</w:t></w:r></w:p>
<w:p><w:r><w:t>带图片</w:t></w:r>
<w:r><w:drawing><wp:inline><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rId5"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>姓名</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>职务</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>张三</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>经理</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
</w:body></w:document>"#;

    #[test]
    fn import_parses_headings_paragraphs_table_and_image() {
        let bytes = make_docx(DOC_XML, true);
        let note = import(&bytes).unwrap();
        assert!(note.markdown.contains("# 会议纪要"));
        assert!(note.markdown.contains("第一段正文"));
        assert!(note.markdown.contains("![](@@IMG0@@)"));
        assert!(note.markdown.contains("| 姓名 | 职务 |"));
        assert!(note.markdown.contains("| 张三 | 经理 |"));
        assert_eq!(note.title, "会议纪要");
        assert_eq!(note.images.len(), 1);
        assert_eq!(note.images[0].ext, "png");
        assert_eq!(note.images[0].bytes, b"\x89PNG\x0d\x0a\x1a\x0a");
    }

    #[test]
    fn import_rejects_non_docx() {
        let err = import(b"this is not a zip").unwrap_err();
        assert_eq!(err.code(), "import_failed");
    }

    #[test]
    fn import_rejects_missing_document_xml() {
        // 合法 zip 但缺 document.xml
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("foo.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"x").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let err = import(&bytes).unwrap_err();
        assert_eq!(err.code(), "unsupported_format");
    }

    #[test]
    fn import_rejects_zip_bomb_total_size() {
        // 伪造 uncompressed_size 超 50MB 的 zip（数据少但元数据声明大）
        // 用 zip 无法直接声明假的 uncompressed_size，故改为构造单条目超 MAX_SINGLE 的块
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        let big = vec![0u8; (MAX_SINGLE + 1) as usize];
        writer.write_all(&big).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let err = import(&bytes).unwrap_err();
        assert_eq!(err.code(), "import_too_large");
    }

    #[test]
    fn import_defuses_xxe_doctype() {
        // 含外部实体的 DOCTYPE：allow_dtd=false → roxmltree 直接拒绝该文档，
        // 外部实体永不加载（防 XXE，无任何网络请求）。
        let evil = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<!DOCTYPE w:document [ <!ENTITY xxe SYSTEM "http://evil.example/x"> ]>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:t>&xxe;泄漏</w:t></w:r></w:p>
</w:body></w:document>"#;
        let bytes = make_docx(evil, false);
        let err = import(&bytes).unwrap_err();
        assert_eq!(err.code(), "import_failed");
    }

    #[test]
    fn export_produces_openable_docx_with_chinese() {
        let md = "# 标题\n\n正文含中文\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n";
        let bytes = export(md, &[]).unwrap();
        // 解包回读验证
        let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let mut f = archive.by_name("word/document.xml").unwrap();
        let mut xml = String::new();
        f.read_to_string(&mut xml).unwrap();
        assert!(xml.contains("encoding=\"UTF-8\""));
        assert!(xml.contains("标题"));
        assert!(xml.contains("正文含中文"));
        assert!(xml.contains("w:tbl"));
        assert!(xml.contains("Heading1"));
        // document.xml 本身可被 roxmltree 解析
        roxmltree::Document::parse(&xml).unwrap();
    }

    #[test]
    fn export_embeds_image_and_media_rels() {
        let md = "![示例](a/assets/note-1.png)\n";
        let img = b"\x89PNG\x0d\x0a\x1a\x0a";
        let bytes = export(md, &[("a/assets/note-1.png", &img[..])]).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        assert!(archive.by_name("word/media/image1.png").is_ok());
        {
            let mut f = archive.by_name("word/_rels/document.xml.rels").unwrap();
            let mut rels = String::new();
            f.read_to_string(&mut rels).unwrap();
            assert!(rels.contains("media/image1.png"));
        }
        {
            // 文档含 r:embed
            let mut dx = archive.by_name("word/document.xml").unwrap();
            let mut xml = String::new();
            dx.read_to_string(&mut xml).unwrap();
            assert!(xml.contains("r:embed=\"rId1\""));
        }
    }

    #[test]
    fn xml_escape_handles_special_chars() {
        assert_eq!(
            xml_escape("<a & \"b\">"),
            "&lt;a &amp; &quot;b&quot;&gt;"
        );
    }
}
