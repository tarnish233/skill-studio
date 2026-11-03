//! Skill Studio 核心逻辑。
//!
//! 这个 crate 刻意不依赖 Tauri：agent 探测、skill 扫描、链接引擎、分组与项目
//! 的全部逻辑都在这里，`cargo test -p skill-studio-core` 不需要起 GUI 就能跑完。
//! `src-tauri` 只做一层 `#[tauri::command]` 转发。

// Reuse the integration-test fixture in the Unix scan-cost unit tests.
#[cfg(all(test, unix))]
extern crate self as skill_studio_core;

pub mod error;
pub mod fs;
pub mod models;
pub mod services;

pub use error::{Error, Result};
pub use models::*;
