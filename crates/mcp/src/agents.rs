//! MCP paths are client-specific. Write only native, agent-owned configuration.
use anyhow::{bail, Result};
use skill_studio_core::{fs::paths, models::agent::require_agent};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub const IDS: &[&str] = &["claude", "codex", "opencode", "pi", "grok"];
pub const PROJECT_FILES: &[&str] = &[
    ".mcp.json",
    ".codex/config.toml",
    "opencode.json",
    "opencode.jsonc",
    ".opencode/opencode.json",
    ".opencode/opencode.jsonc",
    ".pi/mcp.json",
    ".grok/config.toml",
];
/// 提示里用的 Agent 名称
pub fn name(agent: &str) -> &str {
    match agent {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "opencode" => "OpenCode",
        "pi" => "Pi",
        "grok" => "Grok Build",
        other => other,
    }
}
pub fn validate(agent: &str) -> Result<()> {
    if !IDS.contains(&agent) {
        bail!("不支持的 MCP Agent");
    }
    Ok(())
}
fn opencode_files(root: &Path) -> Vec<PathBuf> {
    vec![root.join("opencode.json"), root.join("opencode.jsonc")]
}
pub fn global_files(agent: &str, overrides: &HashMap<String, PathBuf>) -> Result<Vec<PathBuf>> {
    validate(agent)?;
    if agent == "claude" {
        if overrides.contains_key("claude-code")
            || paths::env_dir_override("CLAUDE_CONFIG_DIR").is_some()
        {
            bail!("自定义 Claude 配置目录暂不支持全局 MCP，请选择项目配置");
        }
        return Ok(vec![paths::home_dir().join(".claude.json")]);
    }
    let root = require_agent(agent)?.resolved_config_dir(overrides);
    Ok(match agent {
        "opencode" => {
            let mut files = opencode_files(&root);
            if !overrides.contains_key(agent) {
                if let Some(extra) = paths::env_dir_override("OPENCODE_CONFIG_DIR") {
                    files.extend(opencode_files(&extra));
                }
                if let Some(file) = paths::env_dir_override("OPENCODE_CONFIG") {
                    files.push(file);
                }
            }
            files
        }
        "pi" => vec![root.join("mcp.json")],
        _ => vec![root.join("config.toml")],
    })
}
pub fn project_files(agent: &str, root: &Path) -> Result<Vec<PathBuf>> {
    validate(agent)?;
    Ok(match agent {
        "claude" => vec![root.join(".mcp.json")],
        "codex" => vec![root.join(".codex/config.toml")],
        "grok" => vec![root.join(".grok/config.toml")],
        "pi" => vec![root.join(".pi/mcp.json")],
        "opencode" => {
            let mut files = opencode_files(root);
            files.extend(opencode_files(&root.join(".opencode")));
            files
        }
        _ => unreachable!(),
    })
}
pub fn write_path(
    agent: &str,
    overrides: &HashMap<String, PathBuf>,
    project: Option<&Path>,
) -> Result<PathBuf> {
    let files = match project {
        Some(root) => project_files(agent, root)?,
        None => global_files(agent, overrides)?,
    };
    // Prefer the highest-precedence existing file. Never create a competing config.
    Ok(files
        .iter()
        .rev()
        .find(|p| p.exists())
        .unwrap_or(&files[0])
        .clone())
}
