use std::path::PathBuf;

use skill_studio_core::models::project::ProjectBinding;
use skill_studio_core::models::skill::{LinkMode, LinkReport};
use skill_studio_core::Error;

use crate::state::AppState;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    #[serde(flatten)]
    project: ProjectBinding,
    enabled_agent_ids: Vec<String>,
    enabled_skill_ids: Vec<String>,
    enabled_group_ids: Vec<String>,
    uncollected_skill_count: usize,
    /// 按 Agent 的部署明细：卡片上的 Agent 图标、有待应用修改与需要检查都来自这里
    deployments: Vec<skill_studio_core::models::project::ProjectDeployment>,
}

pub fn list_project_skills(
    state: &AppState,
    project_id: String,
) -> Result<Vec<skill_studio_core::services::project_local::ProjectLocalSkill>, String> {
    let config = state.config();
    let project = config
        .project(&project_id)
        .ok_or_else(|| format!("项目 {project_id} 不存在"))?;
    state
        .studio()
        .project_local_skills(&config, project)
        .map_err(Into::into)
}

pub fn delete_project_local_skill(
    state: &AppState,
    project_id: String,
    path: PathBuf,
) -> Result<(), String> {
    state.ensure_writable().map_err(String::from)?;
    state
        .studio()
        .delete_project_local_skill(&state.config(), &project_id, &path)
        .map_err(Into::into)
}

pub fn list_projects(state: &AppState) -> Result<Vec<ProjectView>, String> {
    let config = state.config();
    config
        .projects
        .iter()
        .map(|project| {
            let uncollected_skill_count = state
                .studio()
                .project_local_skills(&config, project)
                .map_err(String::from)?
                .iter()
                .filter(|s| !s.collected && !s.managed && s.entry.frontmatter.is_some())
                .count();
            let mut by_agent = std::collections::HashMap::new();
            for agent in skill_studio_core::models::agent::AGENTS {
                let Some(root) = agent.project_root(&project.root) else {
                    continue;
                };
                let ids: std::collections::HashSet<_> = project
                    .managed_entries
                    .iter()
                    .filter(|entry| {
                        entry.target_path.parent() == Some(root.as_path())
                            && entry.target_path.join("SKILL.md").is_file()
                            && skill_studio_core::services::linker::link_status(
                                &entry.source_path,
                                &entry.target_path,
                            )
                            .is_registered()
                    })
                    .map(|entry| entry.skill_id.clone())
                    .collect();
                if !ids.is_empty() {
                    by_agent.insert(agent.id.to_string(), ids);
                }
            }
            let mut enabled_agent_ids: Vec<_> = by_agent.keys().cloned().collect();
            enabled_agent_ids.sort();
            let mut enabled_skill_ids: Vec<_> = by_agent.values().flatten().cloned().collect();
            enabled_skill_ids.sort();
            enabled_skill_ids.dedup();
            let mut enabled_group_ids: Vec<_> = project
                .group_ids
                .iter()
                .filter(|id| {
                    config.group(id).is_some_and(|group| {
                        !group.skill_ids.is_empty()
                            && by_agent
                                .values()
                                .any(|skills| group.skill_ids.iter().all(|id| skills.contains(id)))
                    })
                })
                .cloned()
                .collect();
            enabled_group_ids.sort();
            enabled_group_ids.dedup();
            Ok(ProjectView {
                project: project.clone(),
                enabled_agent_ids,
                enabled_skill_ids,
                enabled_group_ids,
                uncollected_skill_count,
                deployments: skill_studio_core::services::project_write::project_deployments(
                    project,
                ),
            })
        })
        .collect()
}

pub fn create_project(
    state: &AppState,
    name: String,
    root: PathBuf,
) -> Result<ProjectBinding, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("项目名不能为空".into());
    }
    if !root.is_dir() {
        return Err(format!("项目目录不存在: {}", root.display()));
    }
    state
        .mutate(|_, config| {
            if config
                .projects
                .iter()
                .any(|p| skill_studio_core::fs::paths::is_same_path(&p.root, &root))
            {
                return Err(Error::invalid(format!(
                    "该目录已绑定为项目: {}",
                    root.display()
                )));
            }
            let project = ProjectBinding::new(uuid_v4(), name, root);
            config.projects.push(project.clone());
            Ok(project)
        })
        .map_err(Into::into)
}

pub fn update_project(
    state: &AppState,
    project_id: String,
    name: Option<String>,
    agent_ids: Option<Vec<String>>,
    skill_ids: Option<Vec<String>>,
    group_ids: Option<Vec<String>>,
    link_mode: Option<LinkMode>,
) -> Result<ProjectBinding, String> {
    state
        .mutate(|_, config| {
            let project = config
                .project_mut(&project_id)
                .ok_or_else(|| Error::NotFound(format!("项目 {project_id}")))?;
            if let Some(n) = name {
                let n = n.trim().to_string();
                if n.is_empty() {
                    return Err(Error::invalid("项目名不能为空"));
                }
                project.name = n;
            }
            if let Some(a) = agent_ids {
                project.agent_ids = a;
            }
            if let Some(s) = skill_ids {
                project.skill_ids = s;
            }
            if let Some(g) = group_ids {
                project.group_ids = g;
            }
            if let Some(m) = link_mode {
                project.link_mode = m;
            }
            Ok(project.clone())
        })
        .map_err(Into::into)
}

/// 删除项目绑定。只删元数据，已写进项目目录的 skill 不动 ——
/// 那些文件可能已经提交进 git 了，替用户决定删除是越权。
/// 它的 MCP 记录一并清掉；还有 Agent 在用项目的 MCP 时拒绝，免得项目没了、写进去的 MCP
/// 却再也没有地方停用
pub fn delete_project(state: &AppState, project_id: String) -> Result<(), String> {
    if !state.config().projects.iter().any(|p| p.id == project_id) {
        return Err(Error::NotFound(format!("项目 {project_id}")).into());
    }
    crate::mcp::projects::forget(state, &project_id)?;
    state
        .mutate(|_, config| {
            let before = config.projects.len();
            config.projects.retain(|p| p.id != project_id);
            if config.projects.len() == before {
                return Err(Error::NotFound(format!("项目 {project_id}")));
            }
            Ok(())
        })
        .map_err(Into::into)
}

/// 把项目绑定写进各 agent 的项目目录
pub fn apply_project(
    state: &AppState,
    project_id: String,
    selection: Option<skill_studio_core::models::project::ProjectSelection>,
) -> Result<LinkReport, String> {
    state
        .write_project(&project_id, selection)
        .map_err(Into::into)
}

pub fn unapply_project(
    state: &AppState,
    project_id: String,
    skill_ids: Vec<String>,
    force: Option<bool>,
) -> Result<LinkReport, String> {
    state
        .transactional(|studio, config| {
            studio.unapply_project(config, &project_id, &skill_ids, force.unwrap_or(false))
        })
        .map_err(Into::into)
}

/// 往项目的 .gitignore 追加部署痕迹文件。
///
/// 项目级默认 Copy，会在每个 skill 目录里留下 `.skill-studio-copy.json`。
/// 那是本机溯源信息，不该进版本库。
pub fn write_project_gitignore(state: &AppState, project_id: String) -> Result<bool, String> {
    state.ensure_writable().map_err(String::from)?;
    let root = state
        .config()
        .project(&project_id)
        .map(|p| p.root.clone())
        .ok_or_else(|| format!("项目 {project_id} 不存在"))?;

    write_skill_gitignore(&root).map_err(|e| e.to_string())
}

fn write_skill_gitignore(root: &std::path::Path) -> skill_studio_core::Result<bool> {
    const ENTRIES: &[&str] = &[
        "**/.skill-studio-copy.json",
        "**/.skill-studio-replace-*.json.lock",
        "**/.skill-studio-replace-*.json",
    ];
    let path = root.join(".gitignore");
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Error::io(&path, e)),
    };
    let missing: Vec<_> = ENTRIES
        .iter()
        .filter(|entry| !existing.lines().any(|line| line.trim() == **entry))
        .collect();
    if missing.is_empty() {
        return Ok(false);
    }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    if !next.is_empty() {
        next.push('\n');
    }
    next.push_str("# Skill Studio 的本机溯源信息与事务记录，不应进版本库\n");
    for entry in missing {
        next.push_str(entry);
        next.push('\n');
    }
    skill_studio_core::fs::atomic::write_text_file(&path, &next)?;
    Ok(true)
}

#[cfg(test)]
mod gitignore_tests {
    use super::write_skill_gitignore;
    #[test]
    fn upgrades_existing_rules_without_duplicates_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".gitignore");
        std::fs::write(&path, "node_modules/\n**/.skill-studio-copy.json\n").unwrap();
        assert!(write_skill_gitignore(dir.path()).unwrap());
        let result = std::fs::read_to_string(&path).unwrap();
        assert!(result.starts_with("node_modules/\n"));
        assert_eq!(result.matches("**/.skill-studio-copy.json").count(), 1);
        assert!(result.contains("**/.skill-studio-replace-*.json.lock"));
        assert!(result.contains("**/.skill-studio-replace-*.json\n"));
        assert!(!write_skill_gitignore(dir.path()).unwrap());
        assert_eq!(std::fs::read_to_string(path).unwrap(), result);
    }
    #[test]
    fn unreadable_text_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".gitignore");
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(write_skill_gitignore(dir.path()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), [0xff, 0xfe]);
    }
}

fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn set_project_enabled(
    state: &AppState,
    project_id: String,
    enabled: bool,
) -> Result<LinkReport, String> {
    state
        .set_project_enabled(&project_id, enabled)
        .map_err(Into::into)
}

/// 为一个 Agent 启用或停用项目，成员用已保存的；已经是目标状态时什么也不做
pub fn set_project_agent(
    state: &AppState,
    project_id: String,
    agent_id: String,
    enabled: bool,
) -> Result<LinkReport, String> {
    state
        .set_project_agent(&project_id, &agent_id, enabled)
        .map_err(Into::into)
}

/// 按已保存的成员重新写入项目已启用的全部 Agent（应用修改）
pub fn reapply_project(state: &AppState, project_id: String) -> Result<LinkReport, String> {
    state.reapply_project(&project_id).map_err(Into::into)
}

/// 保存成员并写入已启用的 Agent；`expected` 是打开详情时的成员，别处改过就不覆盖
pub fn save_project(
    state: &AppState,
    project_id: String,
    members: crate::state::ProjectMembers,
    expected: Option<crate::state::ProjectMembers>,
) -> Result<LinkReport, String> {
    state
        .save_project(&project_id, members, expected)
        .map_err(Into::into)
}

pub fn reorder_projects(
    state: &AppState,
    project_ids: Vec<String>,
) -> Result<Vec<ProjectBinding>, String> {
    state
        .mutate(|_, config| {
            reorder_bindings(&mut config.projects, &project_ids)?;
            Ok(config.projects.clone())
        })
        .map_err(Into::into)
}

fn reorder_bindings(projects: &mut [ProjectBinding], project_ids: &[String]) -> Result<(), Error> {
    let ids: std::collections::HashSet<_> = project_ids.iter().collect();
    if ids.len() != projects.len()
        || ids.len() != project_ids.len()
        || projects.iter().any(|p| !ids.contains(&p.id))
    {
        return Err(Error::invalid("项目列表已变化，请刷新后重试"));
    }
    projects.sort_by_key(|p| project_ids.iter().position(|id| id == &p.id).unwrap());
    Ok(())
}

#[cfg(test)]
mod project_order_tests {
    use super::*;
    #[test]
    fn order_roundtrips_and_stale_or_duplicate_lists_do_not_change_it() {
        let mut projects = vec![
            ProjectBinding::new("a".into(), "A".into(), "/a".into()),
            ProjectBinding::new("b".into(), "B".into(), "/b".into()),
        ];
        reorder_bindings(&mut projects, &["b".into(), "a".into()]).unwrap();
        let saved = serde_json::to_string(&projects).unwrap();
        let loaded: Vec<ProjectBinding> = serde_json::from_str(&saved).unwrap();
        assert_eq!(loaded[0].id, "b");
        for ids in [
            vec!["a".into()],
            vec!["a".into(), "a".into()],
            vec!["a".into(), "new".into()],
        ] {
            assert!(reorder_bindings(&mut projects, &ids).is_err());
            assert_eq!(serde_json::to_string(&projects).unwrap(), saved);
        }
    }
}

pub fn list_skill_backups(
    state: &AppState,
) -> Result<Vec<skill_studio_core::services::skill_files::SkillBackup>, String> {
    state.ensure_loaded().map_err(String::from)?;
    // 旧版本留下的备份目录在可写时顺带迁移；只读时只列出
    if state.ensure_writable().is_ok() {
        state
            .studio()
            .migrate_project_backups(&state.config())
            .map_err(String::from)?;
    }
    state
        .studio()
        .skill_backups()
        .map(|v| v.into_iter().filter(|r| !r.disabled).collect())
        .map_err(Into::into)
}
pub fn delete_skill_file(state: &AppState, scope: String, path: PathBuf) -> Result<(), String> {
    state.ensure_writable().map_err(String::from)?;
    state
        .studio()
        .stash_skill(&state.config(), &scope, &path, false)
        .map_err(Into::into)
}
pub fn restore_skill_file(state: &AppState, id: String) -> Result<(), String> {
    state.ensure_writable().map_err(String::from)?;
    state
        .studio()
        .restore_skill_backup(&state.config(), &id)
        .map_err(Into::into)
}
pub fn purge_skill_file(state: &AppState, id: String) -> Result<(), String> {
    state.ensure_writable().map_err(String::from)?;
    let record = state
        .studio()
        .skill_backups()
        .map_err(String::from)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("备份不存在")?;
    if record.disabled {
        return Err("不能直接清除停用文件，请先删除 skill".into());
    }
    state.studio().purge_skill_backup(&id).map_err(Into::into)
}
pub fn set_project_skill_enabled(
    state: &AppState,
    project_id: String,
    path: PathBuf,
    enabled: bool,
) -> Result<(), String> {
    state.ensure_writable().map_err(String::from)?;
    let scope = format!("project:{project_id}");
    if enabled {
        let r = state
            .studio()
            .skill_backups()
            .map_err(String::from)?
            .into_iter()
            .find(|r| r.disabled && r.scope == scope && r.original_path == path)
            .ok_or("停用记录不存在")?;
        state
            .studio()
            .restore_skill_backup(&state.config(), &r.id)
            .map_err(Into::into)
    } else {
        state
            .studio()
            .stash_skill(&state.config(), &scope, &path, true)
            .map_err(Into::into)
    }
}

pub fn collect_project_skill(
    state: &AppState,
    project_id: String,
    path: PathBuf,
) -> Result<skill_studio_core::models::skill::Skill, String> {
    let entry = {
        let config = state.config();
        let project = config.project(&project_id).ok_or("项目不存在")?;
        state
            .studio()
            .project_local_skills(&config, project)
            .map_err(String::from)?
            .into_iter()
            .find(|s| s.entry.path == path)
            .ok_or("项目 skill 已变化")?
    };
    if entry.managed {
        return Err("此 skill 已部署".into());
    }
    let mut prepared = skill_studio_core::services::marketplace::prepare_local(&entry.storage_path)
        .map_err(String::from)?;
    prepared.source = format!("local:{}", path.display());
    prepared.skill_id = entry.entry.name;
    state.install_catalog_skill(&prepared).map_err(Into::into)
}
