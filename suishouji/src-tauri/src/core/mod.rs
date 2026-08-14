//! 随手记 · M1 文件系统核心层
//!
//! 纯 Rust 逻辑，不依赖 Tauri，全部可 `cargo test` 单测。
//! 分层见《实施方案与技术选型.md》2.1：fs_store / markdown(frontmatter) / 文件锁 / 编码。

pub mod docx;
pub mod encoding;
pub mod error;
pub mod filelock;
pub mod frontmatter;
pub mod model;
pub mod pathguard;
pub mod settings;
pub mod store;
pub mod watcher;
