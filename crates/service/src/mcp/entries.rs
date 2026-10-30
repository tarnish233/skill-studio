//! MCP Hub 里的条目：保存（含纳入管理原有配置）、删除，以及删除扫描到的原有配置。

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;
use skill_studio_mcp::{
    agents as mcp_agents, discovery,
    management::{self, Catalog, Entry, Removal, Target},
    native,
};

use super::{after_write, command, ensure_writable, with_write};
use crate::state::AppState;

/// 扫描到的一处原有配置：按 id 重新扫描定位，定义必须和提交时一样
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub id: String,
    pub definition: Value,
}

/// 保存时要写到哪些地方
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    /// 保留的已有接入（其余的撤回）
    #[serde(default)]
    pub binding_ids: Vec<String>,
    /// 纳入管理的原有配置：保留原位置
    #[serde(default)]
    pub sources: Vec<SourceRef>,
    /// 新写入的 Agent
    #[serde(default)]
    pub agents: Vec<String>,
    /// user（全局）、project（项目配置）、local（Claude Code 项目本地）
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
}

/// 保存一个 MCP 并按 `placement` 写入。`expected` 是打开编辑时的样子（新建为 `None`），
/// 别处改过就拒绝保存
pub fn save(
    state: &AppState,
    executable: Option<&Path>,
    entry: Entry,
    expected: Option<Entry>,
    placement: &Placement,
) -> Result<(), String> {
    ensure_writable(state)?;
    with_write(state, |dir, config| {
        let catalog = management::read(dir)?;
        let targets = targets(config, dir, placement, &entry, &catalog)?;
        management::save(dir, entry, targets, command(executable), expected.as_ref())
    })?;
    after_write(state);
    Ok(())
}

/// 删除 Hub 里的 MCP。`restore` 时把接入还原成原来的配置，否则直接撤掉；被其他程序改写过的
/// 接入原样留下（见返回值的 `foreign`）
pub fn remove(state: &AppState, id: &str, restore: bool) -> Result<Removal, String> {
    ensure_writable(state)?;
    let removal = with_write(state, |dir, _| management::remove(dir, id, restore))?;
    after_write(state);
    Ok(removal)
}

/// 删除扫描到的原有配置（不在 Hub 里的），不动配置文件里的其他内容
pub fn remove_sources(state: &AppState, sources: &[SourceRef]) -> Result<(), String> {
    ensure_writable(state)?;
    let placement = Placement {
        sources: sources.to_vec(),
        ..Default::default()
    };
    with_write(state, |dir, config| {
        let catalog = management::read(dir)?;
        let targets = targets(config, dir, &placement, &Entry::default(), &catalog)?;
        management::remove_sources(dir, targets)
    })?;
    after_write(state);
    Ok(())
}

/// 按 `placement` 算出要写的位置
fn targets(
    config: &skill_studio_core::models::config::AppConfig,
    dir: &Path,
    placement: &Placement,
    entry: &Entry,
    catalog: &Catalog,
) -> anyhow::Result<Vec<Target>> {
    use anyhow::{bail, Context};
    let mut targets = vec![];
    for id in &placement.binding_ids {
        let binding = catalog
            .entries
            .iter()
            .find(|e| e.id == entry.id)
            .and_then(|e| e.bindings.iter().find(|b| b.id == *id))
            .context("接入记录已变化，请刷新后重试")?;
        targets.push(Target {
            agent: binding.agent.clone(),
            path: binding.path.clone(),
            project: binding.project.clone(),
            key: binding.key.clone(),
            expected: Some(binding.installed.clone()),
        });
    }
    if !placement.sources.is_empty() {
        let (files, _) = discovery::scan_files(config);
        let scanned = discovery::scan(&files, &[], dir);
        for source in &placement.sources {
            let found = scanned
                .discovered
                .iter()
                .flat_map(|d| &d.sources)
                .find(|s| s.id == source.id)
                .context("来源不存在，请刷新后重试")?;
            if found.definition != source.definition {
                bail!("来源配置已变化，请刷新后重试");
            }
            if found.gateway {
                bail!("不能再次接管网关转发入口");
            }
            targets.push(Target {
                agent: found.agent.clone(),
                path: found.path.clone(),
                project: found.project.clone(),
                key: found.key.clone(),
                expected: Some(found.definition.clone()),
            });
        }
    }
    let overrides = &config.settings.agent_dir_overrides;
    let scope = placement.scope.as_deref().unwrap_or("user");
    if !matches!(scope, "user" | "project" | "local") {
        bail!("无效的安装作用域");
    }
    let project_id = placement.project_id.as_deref().filter(|s| !s.is_empty());
    if scope != "user" && project_id.is_none() {
        bail!("请选择安装项目");
    }
    for agent in placement.agents.iter().map(String::as_str) {
        let mut target_project = None;
        let path = if let Some(id) = project_id {
            let project = config
                .projects
                .iter()
                .find(|p| p.id == id)
                .context("项目不存在")?;
            if scope == "local" {
                if agent != "claude" {
                    bail!("项目本地作用域仅支持 Claude Code，请选择全局或项目配置");
                }
                target_project = Some(project.root.to_string_lossy().to_string());
                mcp_agents::write_path(agent, overrides, None)?
            } else {
                mcp_agents::write_path(agent, overrides, Some(&project.root))?
            }
        } else {
            mcp_agents::write_path(agent, overrides, None)?
        };
        if !targets
            .iter()
            .any(|t| t.agent == agent && t.path == path && t.project == target_project)
        {
            targets.push(Target {
                agent: agent.into(),
                path,
                project: target_project,
                key: entry.name.trim().into(),
                expected: None,
            });
        }
    }
    if targets.iter().any(|target| target.agent == "grok") {
        let path = mcp_agents::write_path("grok", overrides, None)?;
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e).context("无法检查 Grok 全局停用状态"),
        };
        for target in targets.iter().filter(|target| target.agent == "grok") {
            native::check_activation(&text, "grok", &target.key)?;
        }
    }
    Ok(targets)
}
