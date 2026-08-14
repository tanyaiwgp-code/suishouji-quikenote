//! 核心层统一错误类型 + IPC 层结构化错误响应。

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// 解析后的路径落在根目录之外（`../` 等遍历攻击）。
    #[error("路径越界：{0}")]
    OutsideRoot(String),

    /// 相对路径本身非法（绝对路径 / 空 / 含 NUL）。
    #[error("非法路径：{0}")]
    InvalidPath(String),

    /// 文件正在被编辑（锁冲突）。
    #[error("笔记被锁定：{0}")]
    Locked(String),

    /// 目标资源不存在。
    #[error("资源不存在：{0}")]
    NotFound(String),

    /// 不支持的图片类型（非 png/jpg/jpeg/gif/webp/bmp/svg）。
    #[error("不支持的图片类型：{0}")]
    UnsupportedType(String),

    /// 单张图片超出 20MB 上限。
    #[error("图片超过 20MB 上限：{0}")]
    ImageTooLarge(String),

    /// 单笔记图片数达到 50 张上限。
    #[error("单笔记图片数已达上限（50 张）：{0}")]
    ImageLimitReached(String),

    /// 底层 IO 错误。
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    /// 稳定的 snake_case 错误码，供前端 `ApiError.code` 分支判断（跨版本不变）。
    /// 新增错误变体时必须补对应 code，契约测试与前端均依赖此映射。
    pub fn code(&self) -> &'static str {
        match self {
            Error::OutsideRoot(_) => "outside_root",
            Error::InvalidPath(_) => "invalid_path",
            Error::Locked(_) => "locked",
            Error::NotFound(_) => "not_found",
            Error::UnsupportedType(_) => "unsupported_type",
            Error::ImageTooLarge(_) => "image_too_large",
            Error::ImageLimitReached(_) => "image_limit_reached",
            Error::Io(_) => "io",
        }
    }
}

/// IPC 层结构化错误响应：`{ code, message }` 序列化后传前端（非拍平字符串）。
/// 不依赖 Tauri，可在 core 层单测。`code` 语义同 [`Error::code`]。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl From<Error> for CommandError {
    fn from(e: Error) -> Self {
        Self::new(e.code(), e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    /// 每个变体的 code 必须映射稳定且唯一；文案保持中文（前端状态栏展示用）。
    #[test]
    fn error_code_mapping_is_stable() {
        let cases: &[(Error, &str)] = &[
            (Error::OutsideRoot("a".into()), "outside_root"),
            (Error::InvalidPath("a".into()), "invalid_path"),
            (Error::Locked("a".into()), "locked"),
            (Error::NotFound("a".into()), "not_found"),
            (Error::UnsupportedType("a".into()), "unsupported_type"),
            (Error::ImageTooLarge("a".into()), "image_too_large"),
            (Error::ImageLimitReached("a".into()), "image_limit_reached"),
            (Error::Io(io::Error::other("x")), "io"),
        ];
        let mut seen = std::collections::HashSet::new();
        for (err, expect) in cases {
            assert_eq!(err.code(), *expect);
            assert!(seen.insert(*expect), "重复 code: {expect}");
        }
    }

    /// CommandError 序列化为 `{code, message}`，message 保留中文文案，code 与 Error::code 一致。
    #[test]
    fn command_error_from_error_preserves_code_and_message() {
        let err = Error::Locked("收件箱/a.md".into());
        let ce = CommandError::from(err);
        assert_eq!(ce.code, "locked");
        assert_eq!(ce.message, "笔记被锁定：收件箱/a.md");
        let json = serde_json::to_value(&ce).unwrap();
        assert_eq!(json["code"], "locked");
        assert_eq!(json["message"], "笔记被锁定：收件箱/a.md");
        // 无冗余字段（前端解析依赖精确形状）
        assert!(json.as_object().unwrap().len() == 2);
    }

    #[test]
    fn command_error_new_sets_fields() {
        let ce = CommandError::new("image_decode_error", "图片数据解码失败：bad");
        assert_eq!(ce.code, "image_decode_error");
        assert_eq!(ce.message, "图片数据解码失败：bad");
    }
}

