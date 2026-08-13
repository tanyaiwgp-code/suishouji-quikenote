//! 编码兼容：自动检测 UTF-8 / GBK / GB2312（M1-8）。
//!
//! 策略：先严格 UTF-8（含 BOM 剥离）；失败则用 chardetng 检测（GBK/GB2312 覆盖），
//! 最后兜底 UTF-8 lossy。GB2312 是 GBK 子集，统一走 GBK 解码即可。

/// 解码字节为字符串，剥离 BOM，绝不 panic。
pub fn decode(bytes: &[u8]) -> String {
    // 剥离 UTF-8 BOM（GBK 编码里 0xEF 0xBB 0xBF 不会作为前缀出现，安全）
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);

    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }

    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    let enc = detector.guess(None, true);
    let (cow, _, _) = enc.decode(bytes);
    cow.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::GBK;

    #[test]
    fn utf8_without_bom() {
        let s = "随手记 hello";
        assert_eq!(decode(s.as_bytes()), s);
    }

    #[test]
    fn utf8_with_bom_stripped() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("有 BOM".as_bytes());
        assert_eq!(decode(&bytes), "有 BOM");
    }

    #[test]
    fn gbk_decodes_without_garbling() {
        let (bytes, _, had_errors) = GBK.encode("中文测试不乱码");
        assert!(!had_errors);
        assert_eq!(decode(&bytes), "中文测试不乱码");
    }

    #[test]
    fn gb2312_compatible_chars() {
        // GB2312 常用汉字，也是合法 GBK 字节流
        let (bytes, _, _) = GBK.encode("你好，世界");
        assert_eq!(decode(&bytes), "你好，世界");
    }

    #[test]
    fn empty_input() {
        assert_eq!(decode(b""), "");
    }
}
