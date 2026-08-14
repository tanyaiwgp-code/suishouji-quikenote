// 由 ts-rs 从 Rust core::model 自动生成（types.gen.ts），消除手写镜像漂移。
// 改 Rust 模型字段/序列化名后，跑 `cd src-tauri && cargo test --lib` 即重新生成。
export * from "./lib/types.gen";
