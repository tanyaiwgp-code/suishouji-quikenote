//! 极简 frontmatter 解析。
//!
//! 设计约束：只解析 title / tags / pinned 三个字段（见 ADR #2），其余键忽略，
//! 不引入 YAML 依赖。正文从闭合 `---` 之后取，供 preview 使用。

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Frontmatter {
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub pinned: bool,
    /// 去掉 frontmatter 块之后的正文。
    pub body: String,
}

/// 解析内容。无 frontmatter（或未闭合）时整体作为正文、字段取默认值。
pub fn parse(content: &str) -> Frontmatter {
    let Some(block) = split_block(content) else {
        return Frontmatter {
            body: content.to_string(),
            ..Default::default()
        };
    };

    let mut fm = Frontmatter {
        body: block.body,
        ..Default::default()
    };

    for line in block.lines {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // 兼容全角冒号
        let line = line.replace('：', ":");
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "title" => fm.title = Some(unquote(value.trim())),
            "pinned" => fm.pinned = parse_bool(value.trim()),
            "tags" => fm.tags = parse_tags(value.trim()),
            _ => {}
        }
    }
    fm
}

/// 设置/替换 frontmatter 中的 title（保留其它键与正文；无 frontmatter 时在开头插入）。
/// 文本级处理：在原 frontmatter 块内原位替换 `title:` 行（找不到则插入第 2 行），
/// 避免用结构重建丢失未知键（`created:` 等）或打乱行序。
pub fn set_title(content: &str, new_title: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let has_frontmatter = lines.first().map(|l| l.trim_end() == "---").unwrap_or(false);
    if has_frontmatter {
        // 从第 2 行起找闭合 `---`
        if let Some(close_off) = lines[1..].iter().position(|l| l.trim_end() == "---") {
            let close = close_off + 1; // 闭合行索引
            let mut replaced = false;
            let mut out: Vec<String> = Vec::with_capacity(lines.len());
            for (i, line) in lines.iter().enumerate() {
                if !replaced && i >= 1 && i < close {
                    let key = line.trim().replace('：', ":");
                    if key
                        .split_once(':')
                        .map(|(k, _)| k.trim() == "title")
                        .unwrap_or(false)
                    {
                        out.push(format!("title: {new_title}"));
                        replaced = true;
                        continue;
                    }
                }
                out.push((*line).to_string());
            }
            if !replaced {
                out.insert(1, format!("title: {new_title}"));
            }
            return out.join("\n");
        }
    }
    // 无 frontmatter（或未闭合）：在开头插入块
    format!("---\ntitle: {new_title}\n---\n{}", content.trim_end())
}

struct Block<'a> {
    lines: Vec<&'a str>,
    body: String,
}

/// 内容以 `---` 开头且有闭合 `---` 时返回块；否则 None（视为无 frontmatter）。
fn split_block(content: &str) -> Option<Block<'_>> {
    let mut lines = content.lines();
    let opening = lines.next()?;
    if opening.trim_end() != "---" {
        return None;
    }

    let mut block = Vec::new();
    for line in lines.by_ref() {
        if line.trim_end() == "---" {
            let body = lines.collect::<Vec<_>>().join("\n");
            return Some(Block { lines: block, body });
        }
        block.push(line);
    }
    None // 只有开头没有闭合 → 降级为无 frontmatter
}

fn unquote(s: &str) -> String {
    let bytes = s.as_bytes();
    if s.len() >= 2 {
        let (first, last) = (bytes[0], bytes[s.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

fn parse_bool(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "true" | "yes" | "1"
    )
}

fn parse_tags(s: &str) -> Vec<String> {
    let s = s.trim();
    let s = s
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(s);
    s.split([',', '，'])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_title_tags_pinned() {
        let content = "---\ntitle: 周报\npinned: true\ntags: [工作, 周报]\n---\n正文第一行\n第二行";
        let fm = parse(content);
        assert_eq!(fm.title.as_deref(), Some("周报"));
        assert!(fm.pinned);
        assert_eq!(fm.tags, vec!["工作".to_string(), "周报".to_string()]);
        assert_eq!(fm.body, "正文第一行\n第二行");
    }

    #[test]
    fn no_frontmatter_falls_back() {
        let content = "直接开写，没有元数据\n";
        let fm = parse(content);
        assert_eq!(fm.title, None);
        assert!(!fm.pinned);
        assert!(fm.tags.is_empty());
        assert_eq!(fm.body, content);
    }

    #[test]
    fn unclosed_frontmatter_treated_as_plain_body() {
        let content = "---\ntitle: 未闭合\n正文";
        let fm = parse(content);
        assert_eq!(fm.title, None);
        assert_eq!(fm.body, content);
    }

    #[test]
    fn quoted_title_unquoted() {
        let fm = parse("---\ntitle: \"带引号\"\n---\n");
        assert_eq!(fm.title.as_deref(), Some("带引号"));
        let fm = parse("---\ntitle: '单引号'\n---\n");
        assert_eq!(fm.title.as_deref(), Some("单引号"));
    }

    #[test]
    fn boolean_aliases() {
        for (s, expect) in [("true", true), ("yes", true), ("1", true), ("false", false), ("no", false), ("0", false)] {
            let fm = parse(&format!("---\npinned: {s}\n---\n"));
            assert_eq!(fm.pinned, expect, "pinned: {s}");
        }
    }

    #[test]
    fn comma_and_cjk_list() {
        let fm = parse("---\ntags: 生活, 随手\n---\n");
        assert_eq!(fm.tags, vec!["生活".to_string(), "随手".to_string()]);
    }

    #[test]
    fn unknown_keys_ignored() {
        let fm = parse("---\ntitle: A\nfoo: bar\ncreated: 2026-08-13\n---\nbody");
        assert_eq!(fm.title.as_deref(), Some("A"));
        assert_eq!(fm.body, "body");
    }

    #[test]
    fn empty_frontmatter() {
        let fm = parse("---\n---\n内容");
        assert_eq!(fm.title, None);
        assert_eq!(fm.body, "内容");
    }

    #[test]
    fn set_title_replaces_existing_keeps_others() {
        let content = "---\ntitle: 旧标题\ncreated: 2026-08-13\npinned: true\n---\n正文第一行";
        let updated = set_title(content, "新标题");
        assert!(updated.starts_with("---\n"));
        assert!(updated.contains("title: 新标题"));
        assert!(!updated.contains("旧标题"));
        assert!(updated.contains("created: 2026-08-13"), "未知键保留");
        assert!(updated.contains("pinned: true"));
        assert!(updated.contains("正文第一行"));
    }

    #[test]
    fn set_title_inserts_when_no_frontmatter() {
        let updated = set_title("直接开写\n第二行", "新标题");
        assert_eq!(updated, "---\ntitle: 新标题\n---\n直接开写\n第二行");
    }

    #[test]
    fn set_title_inserts_into_empty_frontmatter() {
        let updated = set_title("---\n---\n内容", "标题");
        assert!(updated.starts_with("---\ntitle: 标题\n---\n内容"));
    }
}
