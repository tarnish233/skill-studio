//! Save the selection and reconcile only this project's owned deployments in one transaction.
use super::{
    linker, scanner,
    studio::Studio,
    transaction::{self, Transaction},
};
use crate::{
    fs::{atomic, paths},
    models::{
        agent::{require_agent, AGENTS},
        config::AppConfig,
        group::UntouchedCopy,
        project::{ProjectBinding, ProjectDeployment, ProjectEntry, ProjectSelection},
        skill::{LinkMode, LinkReport, LinkResult, LinkStatus},
    },
    Error, Result,
};
use std::{collections::HashSet, fs};

impl Studio {
    pub fn write_project(
        &self,
        config: &mut AppConfig,
        id: &str,
        selection: Option<ProjectSelection>,
    ) -> Result<LinkReport> {
        self.reconcile_project(config, id, selection, true)
    }

    pub fn set_project_enabled(
        &self,
        config: &mut AppConfig,
        id: &str,
        enabled: bool,
    ) -> Result<LinkReport> {
        self.reconcile_project(config, id, None, enabled)
    }

    fn reconcile_project(
        &self,
        config: &mut AppConfig,
        id: &str,
        selection: Option<ProjectSelection>,
        enabled: bool,
    ) -> Result<LinkReport> {
        let mut next = config.clone();
        let project = next
            .project_mut(id)
            .ok_or_else(|| Error::NotFound(format!("项目 {id}")))?;
        if let Some(s) = selection {
            project.agent_ids = s.agent_ids;
            project.skill_ids = s.skill_ids;
            project.group_ids = s.group_ids;
            project.link_mode = s.link_mode;
        }
        let project = project.clone();
        if enabled
            && project
                .agent_ids
                .iter()
                .any(|id| config.settings.disabled_agents.contains(id))
        {
            return Err(Error::invalid(
                "项目包含已退出管理的 Agent，请取消勾选或先开启该应用",
            ));
        }
        if !project.root.is_dir() {
            return Err(Error::invalid("项目目录不存在"));
        }
        let globals: Vec<_> = AGENTS
            .iter()
            .flat_map(|a| a.resolved_global_roots(&config.settings.agent_dir_overrides))
            .chain(std::iter::once(self.store().hub_dir(config)))
            .collect();
        let views = self.scan_skills(config)?;
        let mut plan = Vec::new();
        let mut targets = HashSet::new();
        // 部署后被修改或替换的副本：仍要保留的不覆盖、继续记为项目的部署；不再需要的不删、
        // 交还给用户。规则与分组一致，见 activate_agent_groups
        let mut kept: Vec<ProjectEntry> = Vec::new();
        let mut untouched = Vec::new();
        for agent_id in project.agent_ids.iter().filter(|_| enabled) {
            let agent = require_agent(agent_id)?;
            let root = agent
                .project_root(&project.root)
                .ok_or_else(|| Error::invalid("Agent 不支持项目 skill"))?;
            linker::ensure_distinct_roots(&root, &globals)?;
            let mut ids = project.skill_ids.clone();
            for gid in &project.group_ids {
                let group = config
                    .group(gid)
                    .ok_or_else(|| Error::NotFound(format!("分组 {gid}")))?;
                ids.extend(group.skill_ids.clone());
            }
            let mut seen = HashSet::new();
            for sid in ids.into_iter().filter(|sid| seen.insert(sid.clone())) {
                let view = views
                    .iter()
                    .find(|v| v.skill.id == sid)
                    .ok_or_else(|| Error::NotFound(format!("skill {sid}")))?;
                let resolved = super::variants::for_agent(config, &view.skill, agent_id)?;
                let skill = &resolved;
                scanner::validate_sync_source(&skill.source_path)?;
                let dest = root.join(&skill.name);
                if self
                    .skill_backups()?
                    .iter()
                    .any(|r| r.disabled && r.original_path == dest)
                {
                    return Err(Error::invalid(
                        "目标存在已停用的项目手动 skill，请先恢复或删除该项",
                    ));
                }
                let key = paths::normalize_path_lexically(&dest);
                if !targets.insert(key) {
                    return Err(Error::invalid("项目包含同目录名的多个 skill"));
                }
                let source = skill
                    .source_path
                    .canonicalize()
                    .map_err(|e| Error::io(&skill.source_path, e))?;
                linker::ensure_distinct_roots(&source, std::slice::from_ref(&dest))?;
                match linker::link_status(&source, &dest) {
                    LinkStatus::NotLinked
                    | LinkStatus::Linked
                    | LinkStatus::Copied
                    | LinkStatus::CopyStale => {}
                    status => {
                        // Studio 写过的副本被改了：原样保留；别人放在这里的内容仍然不覆盖
                        let Some(entry) = project
                            .managed_entries
                            .iter()
                            .find(|e| e.target_path == dest)
                        else {
                            return Err(Error::invalid(format!(
                                "目标存在冲突或本地修改：{}",
                                dest.display()
                            )));
                        };
                        untouched.push(UntouchedCopy {
                            skill_id: skill.id.clone(),
                            name: skill.name.clone(),
                            path: dest.clone(),
                            reason: changed_reason(status).into(),
                            tracked: true,
                        });
                        kept.push(entry.clone());
                        continue;
                    }
                }
                plan.push((skill.clone(), source, dest, agent_id.clone()));
            }
        }
        let mut removals = Vec::new();
        for entry in &project.managed_entries {
            if plan
                .iter()
                .any(|(_, _, dest, _)| *dest == entry.target_path)
                || kept.iter().any(|k| k.target_path == entry.target_path)
            {
                continue;
            }
            let parent = entry
                .target_path
                .parent()
                .ok_or_else(|| Error::invalid("无效的项目部署路径"))?;
            if !AGENTS
                .iter()
                .filter_map(|a| a.project_root(&project.root))
                .any(|root| root == parent)
            {
                return Err(Error::invalid("项目部署路径不属于此项目"));
            }
            linker::ensure_distinct_roots(parent, &globals)?;
            // Validate bytes against the original sidecar even when the Hub source has disappeared.
            match entry.target_path.symlink_metadata() {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(Error::io(&entry.target_path, e)),
            }
            let own_copy = !scanner::is_symlink_or_junction(&entry.target_path)
                && scanner::read_copy_sidecar(&entry.target_path).is_some_and(|m| {
                    m.skill_id == entry.skill_id
                        && paths::paths_alias(&m.source_path, &entry.source_path)
                        && scanner::dir_content_hash(&entry.target_path)
                            .is_ok_and(|h| h == m.source_hash)
                });
            let own_link = scanner::is_symlink_or_junction(&entry.target_path)
                && scanner::link_destination(&entry.target_path)
                    .is_ok_and(|p| paths::paths_alias(&p, &entry.source_path));
            if !own_copy && !own_link {
                // 不删：交还给用户，不再记为项目的部署
                let modified = !scanner::is_symlink_or_junction(&entry.target_path)
                    && scanner::read_copy_sidecar(&entry.target_path)
                        .is_some_and(|m| m.skill_id == entry.skill_id);
                untouched.push(UntouchedCopy {
                    skill_id: entry.skill_id.clone(),
                    name: entry
                        .target_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| entry.skill_id.clone()),
                    path: entry.target_path.clone(),
                    reason: if modified {
                        "副本已被修改"
                    } else {
                        "已被替换"
                    }
                    .into(),
                    tracked: false,
                });
                continue;
            }
            removals.push(entry.target_path.clone());
        }
        let mut tx = Transaction::begin(self.store().dir().join("project-write.json"))?;
        let operation = (|| -> Result<LinkReport> {
            for dest in &removals {
                tx.reserve(dest)?;
            }
            let mut entries = kept.clone();
            let mut report = LinkReport {
                untouched: untouched.clone(),
                ..LinkReport::default()
            };
            for (skill, source, dest, agent_id) in &plan {
                // Recheck just before replacement, preserving external edits.
                match linker::link_status(source, dest) {
                    LinkStatus::NotLinked
                    | LinkStatus::Linked
                    | LinkStatus::Copied
                    | LinkStatus::CopyStale => {}
                    _ => return Err(Error::invalid("目标发生变化，请刷新后重试")),
                }
                let real_dir = dest.is_dir() && !scanner::is_symlink_or_junction(dest);
                let stage = transaction::sibling(dest, "staging");
                if let Some(parent) = stage.parent() {
                    fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
                }
                tx.reserve(&stage)?;
                let linked = if project.link_mode == LinkMode::Copy
                    || (project.link_mode == LinkMode::Auto && real_dir)
                {
                    false
                } else {
                    match linker::create_symlink(source, &stage) {
                        Ok(()) => true,
                        Err(e) if project.link_mode == LinkMode::Symlink => return Err(e),
                        Err(_) => false,
                    }
                };
                if !linked {
                    let hash = scanner::dir_content_hash(source)?;
                    linker::copy_tree(source, &stage)?;
                    if scanner::dir_content_hash(source)? != hash
                        || scanner::dir_content_hash(&stage)? != hash
                    {
                        return Err(Error::invalid("复制期间源内容发生变化"));
                    }
                    atomic::write_json_file(
                        &stage.join(scanner::COPY_SIDECAR),
                        &scanner::CopySidecar {
                            skill_id: skill.id.clone(),
                            source_path: source.clone(),
                            source_hash: hash,
                            copied_at: linker::now_secs(),
                        },
                    )?;
                }
                // Detect destination edits made while staging.
                match linker::link_status(source, dest) {
                    LinkStatus::NotLinked
                    | LinkStatus::Linked
                    | LinkStatus::Copied
                    | LinkStatus::CopyStale => {}
                    _ => return Err(Error::invalid("目标发生变化，请刷新后重试")),
                }
                tx.reserve(dest)?;
                fs::rename(&stage, dest).map_err(|e| Error::io(dest, e))?;
                entries.push(ProjectEntry {
                    skill_id: skill.id.clone(),
                    source_path: source.clone(),
                    target_path: dest.clone(),
                });
                report.push_ok(LinkResult {
                    skill_id: skill.id.clone(),
                    skill_name: skill.name.clone(),
                    agent_id: agent_id.clone(),
                    status: if linked {
                        LinkStatus::Linked
                    } else {
                        LinkStatus::Copied
                    },
                    message: None,
                });
            }
            next.project_mut(id).unwrap().managed_entries = entries;
            self.store().reserve_config(&mut tx, &next)?;
            self.save_config(&next)?;
            Ok(report)
        })();
        match operation {
            Ok(report) => {
                tx.commit()?;
                *config = next;
                Ok(report)
            }
            Err(e) => {
                tx.rollback()?;
                Err(e)
            }
        }
    }
}

/// 写入时发现 Studio 写过的副本变了：提示里的原因
fn changed_reason(status: LinkStatus) -> &'static str {
    match status {
        LinkStatus::CopyModified | LinkStatus::CopyConflict => "副本已被修改",
        LinkStatus::CopyDamaged => "副本已损坏",
        _ => "已被替换",
    }
}

/// 按 Agent 汇总项目的部署：只列出有部署记录的 Agent。按记录逐个核对磁盘上的副本——
/// 过期的只需重新写入（`stale`），被修改、被替换或已不存在的需要用户处理（`issue`）。
pub fn project_deployments(project: &ProjectBinding) -> Vec<ProjectDeployment> {
    AGENTS
        .iter()
        .filter_map(|agent| {
            let root = agent.project_root(&project.root)?;
            let entries: Vec<_> = project
                .managed_entries
                .iter()
                .filter(|e| e.target_path.parent() == Some(root.as_path()))
                .collect();
            if entries.is_empty() {
                return None;
            }
            let mut skill_ids: Vec<String> = Vec::new();
            let mut stale = false;
            let mut problems = Vec::new();
            for entry in entries {
                if !skill_ids.contains(&entry.skill_id) {
                    skill_ids.push(entry.skill_id.clone());
                }
                let name = entry
                    .target_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| entry.skill_id.clone());
                if entry.target_path.symlink_metadata().is_err() {
                    problems.push(format!("{name} 已不存在"));
                    continue;
                }
                match linker::link_status(&entry.source_path, &entry.target_path) {
                    LinkStatus::Linked | LinkStatus::Copied => {}
                    LinkStatus::CopyStale => stale = true,
                    status => problems.push(match changed_reason(status) {
                        "已被替换" => format!("{name} 已被替换"),
                        reason => format!("{name} 的{reason}"),
                    }),
                }
            }
            Some(ProjectDeployment {
                agent_id: agent.id.to_string(),
                skill_ids,
                stale,
                issue: (!problems.is_empty()).then(|| problems.join("；")),
            })
        })
        .collect()
}
