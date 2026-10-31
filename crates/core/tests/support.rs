//! 集成测试共享脚手架。
//!
//! 每个用例都在**独立的临时 HOME** 里跑，绝不碰真实的 `~/.claude` / `~/.codex`。
//! 借鉴 cc-switch `tests/support.rs` 的做法，但用 per-test 临时目录而不是全局
//! 单例，配合 `#[serial]` 保证环境变量互不干扰。

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use skill_studio_core::fs::paths;
use skill_studio_core::services::store::Store;
use skill_studio_core::services::studio::Studio;

pub struct Env {
    pub home: tempfile::TempDir,
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

impl Env {
    /// 建一个隔离环境，并清掉可能干扰路径解析的环境变量。
    pub fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, home.path());
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        std::env::remove_var("CODEX_HOME");
        for var in [
            "XDG_CONFIG_HOME",
            "OPENCODE_CONFIG_DIR",
            "PI_CODING_AGENT_DIR",
            "GROK_HOME",
            "DSH_HOME",
            "DSH_AGENTS_HOME",
        ] {
            std::env::remove_var(var);
        }
        Self { home }
    }

    pub fn path(&self) -> &Path {
        self.home.path()
    }

    pub fn studio(&self) -> Studio {
        Studio::new(Store::new(self.path().join(".skill-studio")))
    }

    pub fn claude_skills(&self) -> PathBuf {
        self.path().join(".claude/skills")
    }

    pub fn codex_skills(&self) -> PathBuf {
        self.path().join(".codex/skills")
    }

    /// Codex 的第二个 User scope 根（中立共享根）
    pub fn agents_skills(&self) -> PathBuf {
        self.path().join(".agents/skills")
    }

    pub fn hub(&self) -> PathBuf {
        self.path().join(".skill-studio/skills")
    }

    /// 在指定目录下造一个最小可用的 skill
    pub fn write_skill(&self, root: &Path, name: &str, body: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
        dir
    }

    pub fn write_simple_skill(&self, root: &Path, name: &str) -> PathBuf {
        self.write_skill(
            root,
            name,
            &format!("---\nname: {name}\ndescription: 测试用 {name}\n---\n\n正文\n"),
        )
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        std::env::remove_var(paths::TEST_HOME_ENV);
    }
}
