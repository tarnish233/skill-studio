//! MCP 项目：成员（MCP 分组 + 单个 MCP）只保存一次，每个 Agent 独立决定是否写进这个项目。
//! 项目本身（名称、目录）属于 Skill 的配置，这里只管它的 MCP 部分。

use std::path::{Path, PathBuf};

use serde::Deserialize;
use skill_studio_mcp::management::{
    self,
    groups::Untouched,
    projects::{self, Snapshot},
    Removal,
};

use super::{
    after_write, command, ensure_managed_in, ensure_writable, scanned_imports, with_write,
    ImportRef,
};
use crate::state::AppState;

/// 保存时提交的成员与设置
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Members {
    #[serde(default)]
    pub entry_ids: Vec<String>,
    #[serde(default)]
    pub group_ids: Vec<String>,
    /// Claude Code 写到项目本地（~/.claude.json 里这个项目的配置），而不是项目里的 .mcp.json
    #[serde(default)]
    pub claude_local: bool,
}

fn root(
    config: &skill_studio_core::models::config::AppConfig,
    project_id: &str,
) -> anyhow::Result<PathBuf> {
    config
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .map(|p| p.root.clone())
        .ok_or_else(|| anyhow::anyhow!("项目不存在"))
}

/// 保存成员和设置，并立即重新写入已启用的 Agent；成员清空时已启用的 Agent 一并撤回。
/// `expected` 是打开详情时的成员与设置，别处改过就拒绝保存。返回原样留下的接入
pub fn save(
    state: &AppState,
    executable: Option<&Path>,
    project_id: &str,
    members: Members,
    imports: &[ImportRef],
    expected: Option<&Snapshot>,
) -> Result<Vec<Untouched>, String> {
    ensure_writable(state)?;
    let untouched = with_write(state, |dir, config| {
        let imports = scanned_imports(config, dir, imports)?;
        projects::save(
            dir,
            project_id,
            projects::Input {
                root: root(config, project_id)?,
                entry_ids: members.entry_ids,
                group_ids: members.group_ids,
                claude_local: members.claude_local,
            },
            imports,
            &config.settings.agent_dir_overrides,
            command(executable),
            expected,
        )
    })?;
    after_write(state);
    Ok(untouched)
}

/// 为一个 Agent 启用（再次启用即重新应用）或停用这个项目
pub fn activate(
    state: &AppState,
    executable: Option<&Path>,
    project_id: &str,
    agent: &str,
    enabled: bool,
) -> Result<Vec<Untouched>, String> {
    ensure_writable(state)?;
    let untouched = with_write(state, |dir, config| {
        root(config, project_id)?;
        if enabled {
            ensure_managed_in(&config.settings, agent).map_err(anyhow::Error::msg)?;
        }
        projects::activate(
            dir,
            project_id,
            agent,
            enabled,
            &config.settings.agent_dir_overrides,
            command(executable),
        )
    })?;
    after_write(state);
    Ok(untouched)
}

/// 删除项目的 MCP 记录（成员选择）；还有 Agent 在用时先停用
pub fn remove(state: &AppState, project_id: &str) -> Result<(), String> {
    ensure_writable(state)?;
    with_write(state, |dir, _| projects::remove(dir, project_id))?;
    after_write(state);
    Ok(())
}

/// 移除 Hub 写在这个项目里的接入（直接纳入管理的那些，不是项目部署的）；被其他程序改写
/// 过的原样留下
pub fn remove_bindings(state: &AppState, project_id: &str) -> Result<Removal, String> {
    ensure_writable(state)?;
    let removal = with_write(state, |dir, config| {
        management::remove_project_bindings(dir, &root(config, project_id)?)
    })?;
    after_write(state);
    Ok(removal)
}
