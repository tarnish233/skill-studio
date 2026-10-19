//! MCP 项目：成员（MCP 分组 + 单个 MCP）只保存一次，每个 Agent 独立决定是否写进这个项目。
//! 与分组同构，只是写到项目里的 MCP 配置（Claude 可选项目本地），而不是 Agent 的全局配置；
//! 写入、撤回、原样保留被其他程序改写的接入，都和分组共用 [`super::deploy`]。
use super::deploy::{deploy, intact, issue, snapshot, Members, Scope, Untouched};
use super::*;
use std::collections::HashMap;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub root: PathBuf,
    #[serde(default)]
    pub entry_ids: Vec<String>,
    #[serde(default)]
    pub group_ids: Vec<String>,
    /// 选中的原有配置：保存时从扫描结果里记下的引用，从不托管或改动它们（同分组）
    #[serde(default)]
    pub references: Vec<Entry>,
    /// Claude Code 写到项目本地（~/.claude.json 里这个项目的配置，仅自己使用），
    /// 而不是项目里可以随项目分享的 .mcp.json
    #[serde(default)]
    pub claude_local: bool,
    /// agent → 这个项目写进该 Agent 配置的成员快照与接入
    #[serde(default)]
    pub active: BTreeMap<String, Deployment>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deployment {
    pub entries: Vec<Entry>,
    pub bindings: Vec<Binding>,
}
/// 保存时提交的成员与设置
pub struct Input {
    pub root: PathBuf,
    pub entry_ids: Vec<String>,
    pub group_ids: Vec<String>,
    pub claude_local: bool,
}

/// 这个项目要写进某个 Agent 的哪份配置
struct Destination {
    path: PathBuf,
    project: Option<String>,
    checks: Vec<PathBuf>,
}
fn destination(
    agent: &str,
    project: &Project,
    overrides: &HashMap<String, PathBuf>,
) -> Result<Destination> {
    let global = crate::agents::write_path(agent, overrides, None);
    let (path, scope) = if agent == "claude" && project.claude_local {
        let path = crate::agents::write_path(agent, overrides, None)?;
        (path, Some(project.root.to_string_lossy().to_string()))
    } else {
        let path = crate::agents::write_path(agent, overrides, Some(&project.root))?;
        if global.as_ref().is_ok_and(|g| *g == path) {
            bail!(
                "项目目录与 {} 的全局配置重叠，不能写入项目",
                crate::agents::name(agent)
            );
        }
        (path, None)
    };
    // Grok 的全局停用列表只在全局配置里，写进项目前同样要检查
    let checks = match (agent, global) {
        ("grok", Ok(global)) => vec![global],
        _ => vec![],
    };
    Ok(Destination {
        path,
        project: scope,
        checks,
    })
}

/// 项目成员：单个 MCP 在前，再按顺序展开绑定的分组，去重
fn members(catalog: &Catalog, project: &Project) -> Result<Vec<Entry>> {
    let mut members = Members::new("项目");
    for id in &project.entry_ids {
        members.add(catalog, id, &project.references)?;
    }
    for group_id in &project.group_ids {
        let group = catalog
            .groups
            .iter()
            .find(|g| g.id == *group_id)
            .context("项目绑定的分组已不存在，请编辑项目")?;
        for id in &group.entry_ids {
            members.add(catalog, id, &group.references)?;
        }
    }
    Ok(members.entries)
}

/// 为一个 Agent 写入或撤回这个项目，结果记在 `project.active`
#[allow(clippy::too_many_arguments)]
fn switch(
    dir: &Path,
    catalog: &Catalog,
    files: &mut BTreeMap<PathBuf, Change>,
    project: &mut Project,
    agent: &str,
    enabled: bool,
    overrides: &HashMap<String, PathBuf>,
    executable: &Path,
) -> Result<Vec<Untouched>> {
    crate::agents::validate(agent)?;
    let owned_before = project
        .active
        .remove(agent)
        .map(|d| d.bindings)
        .unwrap_or_default();
    if !enabled {
        // 撤回只看记录下的接入，不需要解析写入位置
        let scope = Scope {
            agent,
            path: Path::new(""),
            project: None,
            checks: &[],
        };
        let (_, untouched) = deploy(
            dir,
            catalog,
            files,
            &scope,
            &[],
            &owned_before,
            executable,
            "项目",
        )?;
        return Ok(untouched);
    }
    let entries = members(catalog, project)?;
    if entries.is_empty() {
        bail!("项目还没有选择 MCP");
    }
    let target = destination(agent, project, overrides)?;
    let scope = Scope {
        agent,
        path: &target.path,
        project: target.project.as_deref(),
        checks: &target.checks,
    };
    let (bindings, untouched) = deploy(
        dir,
        catalog,
        files,
        &scope,
        &entries,
        &owned_before,
        executable,
        "项目",
    )?;
    project.active.insert(
        agent.into(),
        Deployment {
            entries: snapshot(&entries),
            bindings,
        },
    );
    Ok(untouched)
}

/// 保存成员和设置，并立即重新写入已启用的 Agent（与 Skill 项目「保存即写入」一致）。
/// 成员清空时，已启用的 Agent 一并撤回。返回原样留下的接入。
pub fn save(
    dir: &Path,
    id: &str,
    input: Input,
    imports: Vec<Entry>,
    overrides: &HashMap<String, PathBuf>,
    executable: &Path,
) -> Result<Vec<Untouched>> {
    if id.is_empty() {
        bail!("缺少项目");
    }
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    let mut project = catalog.projects.get(id).cloned().unwrap_or_default();
    let dedup = |ids: Vec<String>| {
        let mut seen = HashSet::new();
        ids.into_iter()
            .filter(|id| seen.insert(id.clone()))
            .collect::<Vec<_>>()
    };
    project.root = input.root;
    project.entry_ids = dedup(input.entry_ids);
    project.group_ids = dedup(input.group_ids);
    project.claude_local = input.claude_local;
    project.references = super::deploy::references(
        &catalog,
        &project.entry_ids,
        std::mem::take(&mut project.references),
        imports,
        "项目",
    )?;
    let mut files = BTreeMap::new();
    let mut untouched = vec![];
    let empty = project.entry_ids.is_empty() && project.group_ids.is_empty();
    for agent in project.active.keys().cloned().collect::<Vec<_>>() {
        untouched.extend(switch(
            dir,
            &catalog,
            &mut files,
            &mut project,
            &agent,
            !empty,
            overrides,
            executable,
        )?);
    }
    catalog.projects.insert(id.into(), project);
    commit(dir, files, &catalog)?;
    Ok(untouched)
}

/// 为一个 Agent 启用（再次启用即重新应用）或停用这个项目，返回原样留下的接入
pub fn activate(
    dir: &Path,
    id: &str,
    agent: &str,
    enabled: bool,
    overrides: &HashMap<String, PathBuf>,
    executable: &Path,
) -> Result<Vec<Untouched>> {
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    let Some(mut project) = catalog.projects.get(id).cloned() else {
        if enabled {
            bail!("项目还没有选择 MCP");
        }
        return Ok(vec![]);
    };
    let mut files = BTreeMap::new();
    let untouched = switch(
        dir,
        &catalog,
        &mut files,
        &mut project,
        agent,
        enabled,
        overrides,
        executable,
    )?;
    catalog.projects.insert(id.into(), project);
    commit(dir, files, &catalog)?;
    Ok(untouched)
}

/// 删除项目时一并删掉它的 MCP 记录；还有 Agent 在用时先停用
pub fn remove(dir: &Path, id: &str) -> Result<()> {
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    match catalog.projects.get(id) {
        None => return Ok(()),
        Some(project) if !project.active.is_empty() => {
            bail!("请先停用项目里所有 Agent 的 MCP")
        }
        Some(_) => {}
    }
    catalog.projects.remove(id);
    commit(dir, BTreeMap::new(), &catalog)
}

/// 项目 id → Agent → 写进去的接入已被外部修改或移除：只标在出问题的项目和 Agent 上
pub fn issues(catalog: &Catalog) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut texts = Default::default();
    let mut out = BTreeMap::new();
    for (id, project) in &catalog.projects {
        let mut by_agent = BTreeMap::new();
        for (agent, deployment) in &project.active {
            if let Some(b) = deployment.bindings.iter().find(|b| !intact(&mut texts, b)) {
                by_agent.insert(agent.clone(), issue(b));
            }
        }
        if !by_agent.is_empty() {
            out.insert(id.clone(), by_agent);
        }
    }
    out
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;
