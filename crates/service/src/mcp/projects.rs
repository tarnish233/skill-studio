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
    after_write, command, dir, ensure_managed, ensure_writable, scanned_imports, ImportRef,
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

fn root(state: &AppState, project_id: &str) -> Result<PathBuf, String> {
    state
        .config()
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .map(|p| p.root.clone())
        .ok_or_else(|| "项目不存在".to_string())
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
    let dir = dir(state);
    let root = root(state, project_id)?;
    let overrides = state.config().settings.agent_dir_overrides.clone();
    let imports = scanned_imports(state, &dir, imports).map_err(|e| e.to_string())?;
    let untouched = projects::save(
        &dir,
        project_id,
        projects::Input {
            root,
            entry_ids: members.entry_ids,
            group_ids: members.group_ids,
            claude_local: members.claude_local,
        },
        imports,
        &overrides,
        command(executable),
        expected,
    )
    .map_err(|e| e.to_string())?;
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
    root(state, project_id)?;
    if enabled {
        ensure_managed(state, agent)?;
    }
    let overrides = state.config().settings.agent_dir_overrides.clone();
    let untouched = projects::activate(
        &dir(state),
        project_id,
        agent,
        enabled,
        &overrides,
        command(executable),
    )
    .map_err(|e| e.to_string())?;
    after_write(state);
    Ok(untouched)
}

/// 删除项目的 MCP 记录（成员选择）；还有 Agent 在用时先停用
pub fn remove(state: &AppState, project_id: &str) -> Result<(), String> {
    ensure_writable(state)?;
    projects::remove(&dir(state), project_id).map_err(|e| e.to_string())?;
    after_write(state);
    Ok(())
}

/// 移除 Hub 写在这个项目里的接入（直接纳入管理的那些，不是项目部署的）；被其他程序改写
/// 过的原样留下
pub fn remove_bindings(state: &AppState, project_id: &str) -> Result<Removal, String> {
    ensure_writable(state)?;
    let root = root(state, project_id)?;
    let removal =
        management::remove_project_bindings(&dir(state), &root).map_err(|e| e.to_string())?;
    after_write(state);
    Ok(removal)
}
