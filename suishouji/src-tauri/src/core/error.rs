//! 核心层统一错误类型。

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
