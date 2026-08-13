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

    /// 底层 IO 错误。
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
}
