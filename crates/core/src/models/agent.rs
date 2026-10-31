//! Agent 注册表。
//!
//! cc-switch 把 agent 的路径散在三处（`settings.rs` 的 8 个 `get_*_override_dir`、
//! `services/skill.rs::get_app_skills_dir`、`commands/misc.rs::VALID_TOOLS`），
//! 加一个 agent 要改多处 `match`。这里收拢成一张静态表，加 agent 只加一条。

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::fs::paths;

/// 停用一个 skill 的原生机制。**优先用 agent 自己的配置开关，而不是删文件** ——
/// 瞬时、无损、可逆，且用户在 agent 里看到的状态与 Studio 一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToggleMechanism {
    /// `~/.claude/settings.json` 的 `skillOverrides: { "<name>": "off" }`
    ClaudeSettingsJson,
    /// `~/.codex/config.toml` 的 `[[skills.config]] name/enabled`
    CodexConfigToml,
    /// `~/.pi/agent/settings.json` 的 `skills` 数组：`-<skill 目录>` 精确排除自动发现的
    /// skill（含 `~/.agents/skills` 里的），见 pi 的 settings 文档「Resources」
    PiSettingsJson,
    /// 尚未接入该 agent 的原生启停机制；分组仅撤回 Studio 自己部署的文件
    None,
}

/// 一个全局 skill 根目录。
#[derive(Debug, Clone, Copy)]
pub struct SkillRoot {
    /// 相对 home 的默认路径
    pub home_relative: &'static str,
    /// 是否随该 agent 的配置目录覆盖一起移动。
    /// `~/.agents/skills` 这类中立共享根为 false —— 它不在 `CODEX_HOME` 之下。
    pub follows_config_dir: bool,
    /// 是否为多 agent 共享的中立根（扫描时要标注来源，避免重复计数）
    pub shared: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AgentDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    /// 可改写配置目录的环境变量，如 `CLAUDE_CONFIG_DIR` / `CODEX_HOME`
    pub home_env: Option<&'static str>,
    /// 配置目录名（相对 home），如 `.claude`
    pub config_dir: &'static str,
    /// 遵循 XDG_CONFIG_HOME 的配置子目录。
    pub xdg_config_subdir: Option<&'static str>,
    /// Agent 支持的附加配置目录环境变量（只扫描，不替代默认写入目录）。
    pub extra_config_env: Option<&'static str>,
    /// 可独立覆盖共享 Agent 配置根的环境变量；其 skills 子目录只参与扫描。
    pub shared_home_env: Option<&'static str>,
    /// 全局 skill 根，可以多个。**第一个是写入目标**，其余只参与扫描。
    pub global_roots: &'static [SkillRoot],
    /// 项目级 skill 目录（相对项目根）。`None` = 该 agent 不支持项目级。
    ///
    /// 必须是显式白名单：全局有 `config_dir/skills` 约定，**不代表**该工具支持
    /// 项目级 skill。逐个实机验证后才填。
    pub project_skill_dir: Option<&'static str>,
    /// CLI 命令名，用于 `--version` 探测
    pub cli_command: &'static str,
    pub toggle: ToggleMechanism,
    /// 扫描时必须跳过的目录名（大小写不敏感）
    pub reserved_dir_names: &'static [&'static str],
}

impl AgentDescriptor {
    /// 解析该 agent 的配置目录。
    ///
    /// 优先级：**用户在 Studio 里的显式设置 > 环境变量 > 默认 `~/<config_dir>`**。
    /// 用户设置排在最前，是因为 agent 的环境变量常写在 shell profile 里，
    /// GUI 进程未必继承得到，此时需要用户手工指明。
    pub fn resolved_config_dir(&self, overrides: &HashMap<String, PathBuf>) -> PathBuf {
        if let Some(custom) = overrides.get(self.id) {
            return self.normalize_home(custom.clone());
        }
        if let Some(env) = self.home_env.and_then(|name| self.env_dir_override(name)) {
            return self.normalize_home(env);
        }
        if let Some(subdir) = self.xdg_config_subdir {
            if let Some(base) = paths::env_dir_override("XDG_CONFIG_HOME") {
                return base.join(subdir);
            }
        }
        paths::home_dir().join(self.config_dir)
    }

    /// 解析全部全局 skill 根目录（顺序与 `global_roots` 一致）。
    pub fn resolved_global_roots(&self, overrides: &HashMap<String, PathBuf>) -> Vec<PathBuf> {
        let config_dir = self.resolved_config_dir(overrides);
        let home = paths::home_dir();
        let mut roots: Vec<_> = self
            .global_roots
            .iter()
            .map(|root| {
                if root.follows_config_dir {
                    // Strip the entire config prefix: Pi and OpenCode use multi-level paths.
                    let tail = std::path::Path::new(root.home_relative)
                        .strip_prefix(self.config_dir)
                        .expect("agent skill root must be below its config directory");
                    config_dir.join(tail)
                } else if let Some(shared) = root
                    .shared
                    .then(|| {
                        self.shared_home_env
                            .and_then(|name| self.env_dir_override(name))
                    })
                    .flatten()
                {
                    normalize_harness_home(shared).join("skills")
                } else {
                    home.join(root.home_relative)
                }
            })
            .collect();
        if !overrides.contains_key(self.id) {
            if let Some(extra) = self.extra_config_env.and_then(paths::env_dir_override) {
                let root = extra.join("skills");
                if !roots.contains(&root) {
                    roots.push(root);
                }
            }
        }
        roots
    }

    /// 注册新 skill 时写入的目标根（`global_roots` 的第一个）。
    pub fn primary_global_root(&self, overrides: &HashMap<String, PathBuf>) -> PathBuf {
        self.resolved_global_roots(overrides)
            .into_iter()
            .next()
            .expect("每个 agent 至少要有一个全局 skill 根")
    }

    /// 该 agent 会读、但同时被其他 agent 读取的全局根（如 `~/.agents/skills`）。
    pub fn resolved_shared_roots(&self, overrides: &HashMap<String, PathBuf>) -> Vec<PathBuf> {
        self.global_roots
            .iter()
            .zip(self.resolved_global_roots(overrides))
            .filter(|(root, _)| root.shared)
            .map(|(_, path)| path)
            .collect()
    }

    /// 该 agent 读取配置目录的三个环境变量名（主目录、追加目录、共享根）。
    ///
    /// 只用于「有哪些变量会被忽略」的枚举：CLI 交互模式和远程沙箱各按自己的策略
    /// 清空/上报，解析优先级仍由 [`Self::resolved_config_dir`] 与
    /// [`Self::resolved_global_roots`] 决定。
    pub fn env_var_names(&self) -> impl Iterator<Item = &'static str> {
        [self.home_env, self.extra_config_env, self.shared_home_env]
            .into_iter()
            .flatten()
    }

    /// 该项目下这个 agent 的 skill 目录。不支持项目级时返回 `None`。
    pub fn project_root(&self, project_root: &std::path::Path) -> Option<PathBuf> {
        self.project_skill_dir.map(|rel| project_root.join(rel))
    }

    /// 原生配置文件路径（用于三态启停）
    pub fn toggle_config_path(&self, overrides: &HashMap<String, PathBuf>) -> Option<PathBuf> {
        let dir = self.resolved_config_dir(overrides);
        match self.toggle {
            ToggleMechanism::ClaudeSettingsJson => Some(dir.join("settings.json")),
            ToggleMechanism::CodexConfigToml => Some(dir.join("config.toml")),
            ToggleMechanism::PiSettingsJson => Some(dir.join("settings.json")),
            ToggleMechanism::None => None,
        }
    }

    fn normalize_home(&self, path: PathBuf) -> PathBuf {
        if self.home_env == Some("DSH_HOME") {
            normalize_harness_home(path)
        } else {
            path
        }
    }

    /// Harness only treats whitespace-only env values as unset; valid paths keep
    /// their significant leading/trailing spaces. Other Agents keep the generic
    /// trimming override semantics.
    fn env_dir_override(&self, var: &str) -> Option<PathBuf> {
        if self.id == "deepseek-harness" {
            harness_env_dir_override(var)
        } else {
            paths::env_dir_override(var)
        }
    }

    /// Harness only discovers the nearest Git ancestor, not an arbitrary monorepo subdirectory.
    /// Validate writes without changing the selected project or redirecting them outside it.
    pub fn validate_project_root(&self, root: &std::path::Path) -> crate::error::Result<()> {
        if self.id != "deepseek-harness" {
            return Ok(());
        }
        let root = root
            .canonicalize()
            .map_err(|e| crate::error::Error::io(root, e))?;
        for ancestor in root.ancestors() {
            let marker = ancestor.join(".git");
            match std::fs::metadata(&marker) {
                Ok(_) => {
                    if ancestor != root {
                        return Err(crate::error::Error::invalid(format!(
                            "DeepSeek Harness 的项目根是 {}；请选择该仓库根目录，不能把 Skill 写入其子项目目录",
                            ancestor.display()
                        )));
                    }
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(crate::error::Error::io(&marker, e)),
            }
        }
        Ok(())
    }

    pub fn is_reserved_dir(&self, name: &str) -> bool {
        self.reserved_dir_names
            .iter()
            .any(|r| r.eq_ignore_ascii_case(name))
    }
}

/// Harness home-paths expands current-user tilde forms and resolves relative overrides.
fn harness_env_dir_override(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).and_then(|value| {
        let path = PathBuf::from(value);
        (!path.to_string_lossy().trim().is_empty()).then_some(path)
    })
}

fn normalize_harness_home(path: PathBuf) -> PathBuf {
    let expanded = match path.to_str() {
        Some("~") => paths::home_dir(),
        Some(text) => text
            .strip_prefix("~/")
            .or_else(|| text.strip_prefix("~\\"))
            .map(|tail| paths::home_dir().join(tail))
            .unwrap_or(path),
        None => path,
    };
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| paths::home_dir())
            .join(expanded)
    };
    paths::normalize_path_lexically(&absolute)
}

/// Claude Code。
///
/// 路径与机制来自官方文档 code.claude.com/docs/en/skills：
/// - 全局 `~/.claude/skills/<name>/SKILL.md`，项目 `.claude/skills/`（逐级向上加载到 repo root）
/// - `CLAUDE_CONFIG_DIR` 可改写配置目录
/// - 明确支持符号链接：Claude 从 target 读 `SKILL.md` 且只加载一次
/// - `synced`（任意大小写）是 claude.ai 同步专用保留名，扫描必须跳过
/// - 改动 skill 目录后热重载生效，不需要重启
const CLAUDE_CODE: AgentDescriptor = AgentDescriptor {
    id: "claude-code",
    display_name: "Claude Code",
    home_env: Some("CLAUDE_CONFIG_DIR"),
    config_dir: ".claude",
    xdg_config_subdir: None,
    extra_config_env: None,
    shared_home_env: None,
    global_roots: &[SkillRoot {
        home_relative: ".claude/skills",
        follows_config_dir: true,
        shared: false,
    }],
    project_skill_dir: Some(".claude/skills"),
    cli_command: "claude",
    toggle: ToggleMechanism::ClaudeSettingsJson,
    reserved_dir_names: &["synced"],
};

/// Codex。
///
/// 路径来自源码 `codex-rs/ext/skills/src/host_roots.rs`（权威）：
/// `AGENTS_DIR_NAME = ".agents"`、`SKILLS_DIR_NAME = "skills"`，User scope
/// **同时**包含 `$CODEX_HOME/skills` 和 `~/.agents/skills` 两个根；
/// Repo scope 是 `.agents/skills`，逐级向上探测祖先目录。
/// 官方文档确认支持并跟随符号链接。
///
/// 注意：市面上不少同类工具把项目级写成 `.codex/skills`，是错的。
const CODEX: AgentDescriptor = AgentDescriptor {
    id: "codex",
    display_name: "Codex",
    home_env: Some("CODEX_HOME"),
    config_dir: ".codex",
    xdg_config_subdir: None,
    extra_config_env: None,
    shared_home_env: None,
    global_roots: &[
        SkillRoot {
            home_relative: ".codex/skills",
            follows_config_dir: true,
            shared: false,
        },
        SkillRoot {
            // 中立共享根，不随 CODEX_HOME 移动
            home_relative: ".agents/skills",
            follows_config_dir: false,
            shared: true,
        },
    ],
    project_skill_dir: Some(".agents/skills"),
    cli_command: "codex",
    toggle: ToggleMechanism::CodexConfigToml,
    reserved_dir_names: &[],
};

/// OpenCode: opencode.ai/docs/skills. OPENCODE_CONFIG_DIR is additive;
/// XDG_CONFIG_HOME relocates the standard global configuration directory.
const OPENCODE: AgentDescriptor = AgentDescriptor {
    id: "opencode",
    display_name: "OpenCode",
    home_env: None,
    config_dir: ".config/opencode",
    xdg_config_subdir: Some("opencode"),
    extra_config_env: Some("OPENCODE_CONFIG_DIR"),
    shared_home_env: None,
    global_roots: &[
        SkillRoot {
            home_relative: ".config/opencode/skills",
            follows_config_dir: true,
            shared: false,
        },
        SHARED_SKILLS,
    ],
    project_skill_dir: Some(".opencode/skills"),
    cli_command: "opencode",
    toggle: ToggleMechanism::None,
    reserved_dir_names: &[],
};

/// Pi: badlogic/pi-mono, packages/coding-agent/src/config.ts and docs/skills.md.
const PI: AgentDescriptor = AgentDescriptor {
    id: "pi",
    display_name: "Pi",
    home_env: Some("PI_CODING_AGENT_DIR"),
    config_dir: ".pi/agent",
    xdg_config_subdir: None,
    extra_config_env: None,
    shared_home_env: None,
    global_roots: &[
        SkillRoot {
            home_relative: ".pi/agent/skills",
            follows_config_dir: true,
            shared: false,
        },
        SHARED_SKILLS,
    ],
    project_skill_dir: Some(".pi/skills"),
    cli_command: "pi",
    toggle: ToggleMechanism::PiSettingsJson,
    reserved_dir_names: &[],
};

/// Grok Build: xai-org/grok-build, user-guide/08-skills.md.
const GROK: AgentDescriptor = AgentDescriptor {
    id: "grok",
    display_name: "Grok Build",
    home_env: Some("GROK_HOME"),
    config_dir: ".grok",
    xdg_config_subdir: None,
    extra_config_env: None,
    shared_home_env: None,
    global_roots: &[
        SkillRoot {
            home_relative: ".grok/skills",
            follows_config_dir: true,
            shared: false,
        },
        SHARED_SKILLS,
    ],
    project_skill_dir: Some(".grok/skills"),
    cli_command: "grok",
    toggle: ToggleMechanism::None,
    reserved_dir_names: &[],
};

/// deepseek-ai/deepseek-harness, dsh-v0.2.1-rc.5:
/// packages/skill/skill-filesystem and packages/util/home-paths.
/// This adapter manages directory bundles only, not flat .md files or custom Profile providers.
const DEEPSEEK_HARNESS: AgentDescriptor = AgentDescriptor {
    id: "deepseek-harness",
    display_name: "DeepSeek Harness",
    home_env: Some("DSH_HOME"),
    config_dir: ".dsh",
    xdg_config_subdir: None,
    extra_config_env: None,
    shared_home_env: Some("DSH_AGENTS_HOME"),
    global_roots: &[
        SkillRoot {
            home_relative: ".dsh/skills",
            follows_config_dir: true,
            shared: false,
        },
        SHARED_SKILLS,
    ],
    project_skill_dir: Some(".dsh/skills"),
    cli_command: "dsh",
    toggle: ToggleMechanism::None,
    reserved_dir_names: &[".system"],
};

const SHARED_SKILLS: SkillRoot = SkillRoot {
    home_relative: ".agents/skills",
    follows_config_dir: false,
    shared: true,
};

/// 支持 Skill 管理的 Agent；MCP 使用自己的能力白名单。
pub static AGENTS: &[AgentDescriptor] = &[CLAUDE_CODE, CODEX, OPENCODE, PI, GROK, DEEPSEEK_HARNESS];

/// `path` 是否直接位于共享根下（`~/.agents/skills/<name>`）。
///
/// 放进共享根的 skill 会被 Codex、OpenCode、Pi、Grok 同时加载，分组和原生开关都管不住，
/// 所以 Studio 从不代替某一个 agent 往这里写：回链、还原、移出 Hub 都改写到该 agent
/// 自己的根（见 `primary_global_root`）。
pub fn in_shared_root(path: &std::path::Path, overrides: &HashMap<String, PathBuf>) -> bool {
    path.parent().is_some_and(|parent| {
        AGENTS
            .iter()
            .flat_map(|agent| agent.resolved_shared_roots(overrides))
            .any(|root| paths::paths_alias(parent, &root))
    })
}

pub fn find_agent(id: &str) -> Option<&'static AgentDescriptor> {
    AGENTS.iter().find(|a| a.id == id)
}

pub fn require_agent(id: &str) -> crate::error::Result<&'static AgentDescriptor> {
    find_agent(id).ok_or_else(|| {
        crate::error::Error::invalid(format!(
            "未知的 agent: '{id}'。可选值: {}",
            AGENTS.iter().map(|a| a.id).collect::<Vec<_>>().join(", ")
        ))
    })
}

/// 传给前端的 agent 运行时状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub display_name: String,
    /// 配置目录是否存在（= 用户装过这个 agent）
    pub detected: bool,
    /// CLI 是否可执行
    pub cli_available: bool,
    /// CLI 已找到但 `--version` 失败（装了却跑不起来，如 Node 版本不达标）。
    /// 让前端能区分"未安装"与"已安装·无法运行"，不用去匹配错误文案。
    pub cli_broken: bool,
    pub cli_version: Option<String>,
    pub config_dir: PathBuf,
    pub global_skill_dirs: Vec<PathBuf>,
    pub supports_project_skills: bool,
    /// 相对项目根的写入目录；旧远程后端可缺省。
    #[serde(default)]
    pub project_skill_dir: Option<String>,
    pub supports_native_toggle: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn no_overrides() -> HashMap<String, PathBuf> {
        HashMap::new()
    }

    #[test]
    fn registry_has_all_skill_agents() {
        assert_eq!(AGENTS.len(), 6);
        for id in ["opencode", "pi", "grok", "deepseek-harness"] {
            assert!(find_agent(id).is_some());
        }
        assert!(find_agent("claude-code").is_some());
        assert!(find_agent("codex").is_some());
        assert!(find_agent("nope").is_none());
    }

    #[test]
    fn unknown_agent_error_lists_valid_ids() {
        let err = require_agent("bogus").unwrap_err().to_string();
        assert!(err.contains("claude-code"), "{err}");
        assert!(err.contains("codex"), "{err}");
    }

    #[test]
    #[serial]
    fn codex_exposes_both_user_scope_roots() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, dir.path());
        std::env::remove_var("CODEX_HOME");

        let codex = find_agent("codex").unwrap();
        let roots = codex.resolved_global_roots(&no_overrides());
        // 源码确认：$CODEX_HOME/skills 和 ~/.agents/skills 同为 User scope
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0], dir.path().join(".codex/skills"));
        assert_eq!(roots[1], dir.path().join(".agents/skills"));
        // 写入目标是 agent 专属根，不是共享根
        assert_eq!(codex.primary_global_root(&no_overrides()), roots[0]);

        std::env::remove_var(paths::TEST_HOME_ENV);
    }

    #[test]
    #[serial]
    fn codex_home_moves_only_the_agent_specific_root() {
        let dir = tempfile::tempdir().unwrap();
        let custom = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, dir.path());
        std::env::set_var("CODEX_HOME", custom.path());

        let roots = find_agent("codex")
            .unwrap()
            .resolved_global_roots(&no_overrides());
        assert_eq!(roots[0], custom.path().join("skills"));
        // 共享根不随 CODEX_HOME 移动
        assert_eq!(roots[1], dir.path().join(".agents/skills"));

        std::env::remove_var("CODEX_HOME");
        std::env::remove_var(paths::TEST_HOME_ENV);
    }

    #[test]
    #[serial]
    fn claude_config_dir_env_is_honored() {
        let dir = tempfile::tempdir().unwrap();
        let custom = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, dir.path());
        std::env::set_var("CLAUDE_CONFIG_DIR", custom.path());

        let claude = find_agent("claude-code").unwrap();
        assert_eq!(
            claude.resolved_global_roots(&no_overrides())[0],
            custom.path().join("skills")
        );
        assert_eq!(
            claude.toggle_config_path(&no_overrides()).unwrap(),
            custom.path().join("settings.json")
        );

        std::env::remove_var("CLAUDE_CONFIG_DIR");
        std::env::remove_var(paths::TEST_HOME_ENV);
    }

    #[test]
    #[serial]
    fn explicit_user_override_beats_env_var() {
        let dir = tempfile::tempdir().unwrap();
        let env_dir = tempfile::tempdir().unwrap();
        let user_dir = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, dir.path());
        std::env::set_var("CLAUDE_CONFIG_DIR", env_dir.path());

        let mut overrides = HashMap::new();
        overrides.insert("claude-code".to_string(), user_dir.path().to_path_buf());

        assert_eq!(
            find_agent("claude-code")
                .unwrap()
                .resolved_config_dir(&overrides),
            user_dir.path()
        );

        std::env::remove_var("CLAUDE_CONFIG_DIR");
        std::env::remove_var(paths::TEST_HOME_ENV);
    }

    #[test]
    fn env_var_names_enumerate_each_descriptor_override() {
        // 独立硬编码预期：枚举不能靠生产实现自证。
        for (id, names) in [
            ("claude-code", vec!["CLAUDE_CONFIG_DIR"]),
            ("codex", vec!["CODEX_HOME"]),
            ("opencode", vec!["OPENCODE_CONFIG_DIR"]),
            ("pi", vec!["PI_CODING_AGENT_DIR"]),
            ("grok", vec!["GROK_HOME"]),
            ("deepseek-harness", vec!["DSH_HOME", "DSH_AGENTS_HOME"]),
        ] {
            assert_eq!(
                find_agent(id).unwrap().env_var_names().collect::<Vec<_>>(),
                names,
                "{id}"
            );
        }
        let mut all: Vec<_> = AGENTS.iter().flat_map(|a| a.env_var_names()).collect();
        all.sort_unstable();
        assert_eq!(
            all,
            vec![
                "CLAUDE_CONFIG_DIR",
                "CODEX_HOME",
                "DSH_AGENTS_HOME",
                "DSH_HOME",
                "GROK_HOME",
                "OPENCODE_CONFIG_DIR",
                "PI_CODING_AGENT_DIR",
            ]
        );
    }

    #[test]
    fn project_dirs_match_verified_conventions() {
        let root = std::path::Path::new("/tmp/proj");
        assert_eq!(
            find_agent("claude-code")
                .unwrap()
                .project_root(root)
                .unwrap(),
            root.join(".claude/skills")
        );
        // Codex 项目级是 .agents/skills，不是 .codex/skills
        assert_eq!(
            find_agent("codex").unwrap().project_root(root).unwrap(),
            root.join(".agents/skills")
        );
    }

    #[test]
    fn claude_reserves_the_synced_directory_case_insensitively() {
        let claude = find_agent("claude-code").unwrap();
        assert!(claude.is_reserved_dir("synced"));
        assert!(claude.is_reserved_dir("Synced"));
        assert!(claude.is_reserved_dir("SYNCED"));
        assert!(!claude.is_reserved_dir("my-skill"));
        // Codex 没有保留名
        assert!(!find_agent("codex").unwrap().is_reserved_dir("synced"));
    }
}
