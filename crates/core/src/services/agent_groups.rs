//! Agent-scoped combinations with reversible suspension of manual skills.
use super::{linker, native_toggle, scanner, studio::Studio, transaction::Transaction};
use crate::{
    fs::{atomic, paths},
    models::{
        agent::{in_shared_root, require_agent},
        config::{AppConfig, Registration},
        group::{ActiveGroup, Group, GroupEntry, UntouchedCopy},
        skill::LinkMode,
    },
    Error, Result,
};
use std::collections::{HashMap, HashSet};

impl Studio {
    /// Create or edit a shared group's name and members. Groups are not owned by
    /// any single agent: `activate_agent_group` lets each agent independently pick
    /// whether this is its current combination.
    pub fn save_group(
        &self,
        config: &mut AppConfig,
        id: String,
        name: &str,
        skill_ids: Vec<String>,
    ) -> Result<Group> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::invalid("分组名不能为空"));
        }
        if config.groups.iter().any(|g| g.name == name && g.id != id) {
            return Err(Error::invalid("已存在同名分组"));
        }
        // 空分组不能启用；正在用的分组被清空时，先让用户停用，避免「已经存下、写入又失败」的半截状态
        if skill_ids.is_empty() {
            let using = config.agents_using_group(&id);
            // 按 Agent 的固定顺序列出，提示文案才稳定
            let names: Vec<&str> = crate::models::agent::AGENTS
                .iter()
                .filter(|agent| using.contains(&agent.id))
                .map(|agent| agent.display_name)
                .collect();
            if !names.is_empty() {
                return Err(Error::invalid(format!(
                    "分组「{name}」正在 {} 上使用，清空成员前请先为这些 Agent 停用",
                    names.join("、")
                )));
            }
        }
        let mut seen = HashSet::new();
        let skill_ids: Vec<_> = skill_ids
            .into_iter()
            .filter(|id| seen.insert(id.clone()))
            .collect();
        // Preserve missing members on edit so users can see and explicitly remove them.
        let mut group = config
            .group(&id)
            .cloned()
            .unwrap_or_else(|| Group::new(id, name.into()));
        group.name = name.into();
        group.skill_ids = skill_ids;
        if let Some(index) = config.groups.iter().position(|g| g.id == group.id) {
            config.groups[index] = group.clone();
        } else {
            // max + 1 rather than len(): after deletions len() can repeat an existing slot.
            group.sort_order = config
                .groups
                .iter()
                .map(|g| g.sort_order + 1)
                .max()
                .unwrap_or(0);
            config.groups.push(group.clone());
        }
        Ok(group)
    }

    /// Stop deployments and persist the management switch in the same transaction.
    pub fn set_agent_management(
        &self,
        config: &mut AppConfig,
        agent_id: &str,
        enabled: bool,
    ) -> Result<()> {
        require_agent(agent_id)?;
        let mut next = config.clone();
        next.settings.disabled_agents.retain(|id| id != agent_id);
        if enabled {
            self.reconcile_manual_policy(&mut next, true)?;
        } else {
            next.settings.disabled_agents.push(agent_id.into());
            self.activate_agent_groups(&mut next, agent_id, &[])?;
            // 撤回管理期间为隔离它而关掉的兼容读取（见 isolation.rs）
            self.reconcile_manual_policy(&mut next, false)?;
        }
        *config = next;
        Ok(())
    }

    /// 设置这个 agent 当前叠加的全部分组（`&[]` 表示全部停用），返回原样留下的副本。
    ///
    /// 文件操作与启用记录在同一个回滚日志里：部署内容是这些分组成员加手动安装 skill 的
    /// 并集，多个分组共用的成员只部署一份，只在所有用到它的分组都停用后才撤下。
    ///
    /// 部署后被修改、替换或被手动停用的副本不会拦住切换：要撤下的不删、不再管理，
    /// 要保留的不覆盖，都原样留着并报告出来。退出应用管理仍然先要求处理好这些副本。
    pub fn activate_agent_groups(
        &self,
        config: &mut AppConfig,
        agent_id: &str,
        group_ids: &[String],
    ) -> Result<Vec<UntouchedCopy>> {
        if !group_ids.is_empty()
            && config
                .settings
                .disabled_agents
                .iter()
                .any(|id| id == agent_id)
        {
            return Err(Error::invalid("请先在设置中开启此 Agent 的管理"));
        }
        let agent = require_agent(agent_id)?;
        let mut groups = Vec::new();
        let mut seen_groups = HashSet::new();
        for id in group_ids {
            if !seen_groups.insert(id.clone()) {
                continue;
            }
            let group = config
                .group(id)
                .cloned()
                .ok_or_else(|| Error::NotFound(format!("分组 {id}")))?;
            if group.skill_ids.is_empty() {
                return Err(Error::invalid(format!("「{}」还没有成员", group.name)));
            }
            groups.push(group);
        }
        // 分组成员并集，保持分组的启用顺序
        let mut seen_skills = HashSet::new();
        let ids: Vec<String> = groups
            .iter()
            .flat_map(|g| g.skill_ids.iter())
            .filter(|id| seen_skills.insert((*id).clone()))
            .cloned()
            .collect();
        let views = self.scan_skills(config)?;
        // 之前部署过的入口：叠加时来自多个分组，按目标路径去重
        let owned: Vec<GroupEntry> = config
            .active_entries(agent_id)
            .into_iter()
            .cloned()
            .collect();
        let (suspended_manual, manual_toggles) =
            super::manual_policy::plan(config, agent_id, &ids, &views)?;
        let leaving = groups.is_empty()
            && config
                .settings
                .disabled_agents
                .iter()
                .any(|id| id == agent_id);
        // Never silently discard local edits: a copy that no longer matches what Studio
        // deployed is left in place. Leaving management still asks the user to sort it out.
        let mut changed: HashMap<std::path::PathBuf, &'static str> = HashMap::new();
        for entry in &owned {
            if entry.target_path.symlink_metadata().is_err() {
                continue;
            }
            let sidecar = (!scanner::is_symlink_or_junction(&entry.target_path))
                .then(|| scanner::read_copy_sidecar(&entry.target_path))
                .flatten()
                .filter(|m| {
                    m.skill_id == entry.skill_id
                        && paths::paths_alias(&m.source_path, &entry.source_path)
                });
            let reason = match sidecar {
                Some(m)
                    if scanner::dir_content_hash(&entry.target_path)
                        .is_ok_and(|hash| hash == m.source_hash) =>
                {
                    continue
                }
                Some(_) => "副本已被修改",
                None => "已被替换",
            };
            if leaving {
                return Err(Error::invalid(format!(
                    "分组副本已被修改或替换，请先处理后再切换：{}",
                    entry.target_path.display()
                )));
            }
            changed.insert(entry.target_path.clone(), reason);
        }
        let name_of = |skill_id: &str| {
            views
                .iter()
                .find(|v| v.skill.id == skill_id)
                .map(|v| v.skill.name.clone())
                .unwrap_or_else(|| skill_id.to_string())
        };
        let mut untouched = Vec::new();
        let mut additions = Vec::new();
        let mut targets = HashSet::new();
        for id in &ids {
            let view = views
                .iter()
                .find(|v| &v.skill.id == id)
                .ok_or_else(|| Error::NotFound(format!("分组成员已不存在：{id}")))?;
            let mut view = view.clone();
            view.skill = super::variants::for_agent(config, &view.skill, agent_id)?;
            scanner::validate_agent_sync_source(&view.skill.source_path, agent)?;
            let state = &view.agents[agent_id];
            // 继续保留的成员：部署后被改过的副本不覆盖；用户在 Agent 里停用了它就保持停用，
            // 不替用户重新启用。两种都继续由分组管理（卡片上显示需要检查）
            let deployed = owned
                .iter()
                .find(|e| e.skill_id == *id && e.target_path.symlink_metadata().is_ok());
            let kept = deployed.and_then(|entry| {
                let reason = changed.get(&entry.target_path).copied().or((entry
                    .target_path
                    .is_dir()
                    && state.disabled)
                    .then_some("已被手动停用"))?;
                Some(UntouchedCopy {
                    skill_id: id.clone(),
                    name: view.skill.name.clone(),
                    path: entry.target_path.clone(),
                    reason: reason.into(),
                    tracked: true,
                })
            });
            if let Some(kept) = kept {
                untouched.push(kept);
                continue;
            }
            let retained = deployed.is_some_and(|e| e.target_path.is_dir());
            if retained && state.status == crate::models::skill::LinkStatus::Copied {
                continue;
            }
            if !retained && state.status != crate::models::skill::LinkStatus::NotLinked {
                use crate::models::skill::LinkStatus::*;
                if !matches!(
                    state.status,
                    Source | Linked | Copied | CopyStale | CopyModified
                ) || (state.disabled
                    && !manual_toggles
                        .iter()
                        .any(|(sid, _, _, enabled)| sid == id && *enabled))
                {
                    return Err(Error::invalid(format!(
                        "{} 在此 Agent 中不可用或被手动停用，请先处理",
                        view.skill.name
                    )));
                }
                // Existing manual skill is usable: do not take ownership or overwrite it.
                continue;
            }
            let dest = owned
                .iter()
                .find(|e| e.skill_id == *id)
                .map(|e| e.target_path.clone())
                .unwrap_or_else(|| {
                    agent
                        .primary_global_root(&config.settings.agent_dir_overrides)
                        .join(&view.skill.name)
                });
            if !targets.insert(dest.clone()) {
                return Err(Error::invalid("组合包含同目录名的多个 skill，请调整成员"));
            }
            let source = view
                .skill
                .source_path
                .canonicalize()
                .map_err(|e| Error::io(&view.skill.source_path, e))?;
            if paths::path_is_within(&source, &dest) || paths::path_is_within(&dest, &source) {
                return Err(Error::invalid("skill 来源与注册目录重叠"));
            }
            additions.push((view.skill.clone(), dest));
        }
        let overrides = &config.settings.agent_dir_overrides;
        let roots = agent.resolved_global_roots(overrides);
        let mut restores = Vec::new();
        let mut removals = Vec::new();
        let mut restored_paths = HashSet::new();
        // 原件只在共享根里出现过的 skill 改还原到这个 agent 自己的根，记下新入口
        let mut redirected = Vec::new();
        if leaving {
            for view in &views {
                if let Some(record) = config.skill_provenance.get(&view.skill.id) {
                    let backup = record.backup_path.join("content");
                    let (shared, own): (Vec<&std::path::PathBuf>, Vec<_>) = record
                        .entry_paths
                        .iter()
                        .chain(std::iter::once(&record.original_path))
                        .filter(|target| {
                            target
                                .parent()
                                .is_some_and(|p| roots.iter().any(|r| paths::paths_alias(p, r)))
                        })
                        .partition(|target| in_shared_root(target, overrides));
                    // 不往共享根里还原：其他仍在管理中的 agent 也会加载它。这个 agent
                    // 自己的根里本来就有入口就只还原那里，否则换到它的写入根。
                    let mut candidates: HashSet<_> = own.into_iter().cloned().collect();
                    if candidates.is_empty() {
                        let name = shared
                            .iter()
                            .find(|t| ***t == record.original_path)
                            .or(shared.first())
                            .and_then(|t| t.file_name());
                        if let Some(name) = name {
                            let private = agent.primary_global_root(overrides).join(name);
                            redirected.push((view.skill.id.clone(), private.clone()));
                            candidates.insert(private);
                        }
                    }
                    for target in candidates {
                        restored_paths.insert(target.clone());
                        if scanner::dir_content_hash(&backup)? != record.original_hash {
                            return Err(Error::invalid("托管备份不完整，无法还原 Agent"));
                        }
                        use crate::models::skill::LinkStatus::*;
                        if target.symlink_metadata().is_ok() {
                            let status = linker::link_status(&view.skill.source_path, &target);
                            if !matches!(status, Linked | Copied | CopyStale) {
                                if !scanner::is_symlink_or_junction(&target)
                                    && scanner::dir_content_hash(&target)
                                        .is_ok_and(|h| h == record.original_hash)
                                {
                                    continue;
                                }
                                return Err(Error::invalid(format!(
                                    "原目录已修改或被占用，无法还原：{}",
                                    target.display()
                                )));
                            }
                        }
                        restores.push((target, backup.clone(), record.original_hash.clone()));
                    }
                }
            }
            // Global registrations outside the active group also belong to Studio.
            for (skill_id, regs) in &config.registrations {
                let Some(reg) = regs.get(agent_id) else {
                    continue;
                };
                let target = &reg.target_path;
                if owned.iter().any(|e| &e.target_path == target) || restored_paths.contains(target)
                {
                    continue;
                }
                if !target
                    .parent()
                    .is_some_and(|p| roots.iter().any(|r| paths::paths_alias(p, r)))
                {
                    return Err(Error::invalid("注册目录已变更，无法安全退出管理"));
                }
                if target.symlink_metadata().is_err() {
                    continue;
                }
                let view = views
                    .iter()
                    .find(|v| &v.skill.id == skill_id)
                    .ok_or_else(|| Error::invalid("注册来源缺失，无法安全退出管理"))?;
                use crate::models::skill::LinkStatus::*;
                if !matches!(
                    linker::link_status(
                        &super::variants::deployment_source(config, &view.skill, agent_id)?
                            .source_path,
                        target
                    ),
                    Linked | Copied | CopyStale
                ) {
                    return Err(Error::invalid(format!(
                        "注册内容已修改，无法移除：{}",
                        target.display()
                    )));
                }
                removals.push(target.clone());
            }
        }
        if leaving {
            for project in &config.projects {
                let Some(root) = agent.project_root(&project.root) else {
                    continue;
                };
                for entry in &project.managed_entries {
                    if entry.target_path.parent() != Some(root.as_path()) {
                        continue;
                    }
                    linker::ensure_distinct_roots(&root, &roots)?;
                    if entry.target_path.symlink_metadata().is_ok() {
                        use crate::models::skill::LinkStatus::*;
                        if !matches!(
                            linker::link_status(&entry.source_path, &entry.target_path),
                            Linked | Copied | CopyStale
                        ) {
                            return Err(Error::invalid(format!(
                                "项目副本已修改，无法退出管理：{}",
                                entry.target_path.display()
                            )));
                        }
                        removals.push(entry.target_path.clone());
                    }
                }
            }
            removals.sort();
            removals.dedup();
        }
        let mut next = config.clone();
        let mut entries = Vec::new();
        let mut tx = Transaction::begin(self.store().dir().join("group-switch.json"))?;
        let operation = (|| -> Result<()> {
            for entry in &owned {
                if let Some(reason) = changed.get(&entry.target_path) {
                    if ids.contains(&entry.skill_id) {
                        entries.push(entry.clone());
                    } else {
                        // 不再需要它的分组都停用了：不删，交还给用户
                        untouched.push(UntouchedCopy {
                            skill_id: entry.skill_id.clone(),
                            name: name_of(&entry.skill_id),
                            path: entry.target_path.clone(),
                            reason: (*reason).into(),
                            tracked: false,
                        });
                        next.remove_registration(&entry.skill_id, agent_id);
                    }
                    continue;
                }
                if ids.contains(&entry.skill_id)
                    && entry.target_path.is_dir()
                    && !additions.iter().any(|(_, dest)| dest == &entry.target_path)
                {
                    entries.push(entry.clone());
                    continue;
                }
                tx.reserve(&entry.target_path)?;
                next.remove_registration(&entry.skill_id, agent_id);
            }
            // Keep the native config bytes in the outer journal before adding path enable rules.
            if !additions.is_empty() || !manual_toggles.is_empty() {
                if let Some(path) = agent.toggle_config_path(&config.settings.agent_dir_overrides) {
                    tx.reserve_file_for_update(&path)?;
                }
            }
            for (_, name, document, enabled) in &manual_toggles {
                native_toggle::set_skill_enabled_at(
                    agent,
                    &config.settings.agent_dir_overrides,
                    name,
                    document,
                    *enabled,
                )?;
            }
            for (skill, dest) in &additions {
                // Recheck before reserving: don't overwrite an entry created during preflight.
                if dest.symlink_metadata().is_ok() {
                    return Err(Error::invalid("目标已存在，停止切换"));
                }
                if !owned.iter().any(|entry| entry.target_path == *dest) {
                    tx.reserve(dest)?;
                }
                linker::copy_tree(&skill.source_path, dest)?;
                let hash = scanner::dir_content_hash(&skill.source_path)?;
                if scanner::dir_content_hash(dest)? != hash {
                    return Err(Error::invalid("分组副本校验失败"));
                }
                atomic::write_json_file(
                    &dest.join(scanner::COPY_SIDECAR),
                    &scanner::CopySidecar {
                        skill_id: skill.id.clone(),
                        source_path: skill.source_path.clone(),
                        source_hash: hash.clone(),
                        copied_at: linker::now_secs(),
                    },
                )?;
                if agent
                    .toggle_config_path(&config.settings.agent_dir_overrides)
                    .is_some()
                {
                    let fm = scanner::parse_frontmatter(&dest.join("SKILL.md"));
                    native_toggle::set_skill_enabled_at(
                        agent,
                        &config.settings.agent_dir_overrides,
                        &if agent_id == "codex" {
                            fm.name.unwrap_or_else(|| skill.name.clone())
                        } else {
                            skill.name.clone()
                        },
                        &dest.join("SKILL.md"),
                        true,
                    )?;
                }
                next.set_registration(
                    &skill.id,
                    agent_id,
                    Registration {
                        mode: LinkMode::Copy,
                        target_path: dest.clone(),
                        registered_at: linker::now_secs(),
                        source_hash_at_copy: Some(hash),
                    },
                );
                entries.push(GroupEntry {
                    skill_id: skill.id.clone(),
                    source_path: skill.source_path.clone(),
                    target_path: dest.clone(),
                });
            }
            if groups.is_empty() {
                next.active_groups.remove(agent_id);
            } else {
                // 每个分组记录自己引入的入口；两个分组共用的成员归前一个，
                // 停用它时只要还有别的分组需要就会被保留（见上面的 retained 判定）
                let mut claimed = HashSet::new();
                let records = groups
                    .iter()
                    .map(|group| ActiveGroup {
                        group_id: group.id.clone(),
                        skill_ids: group.skill_ids.clone(),
                        entries: entries
                            .iter()
                            .filter(|e| {
                                group.skill_ids.contains(&e.skill_id)
                                    && claimed.insert(e.target_path.clone())
                            })
                            .cloned()
                            .collect(),
                        preserve_manual_skills: config.settings.preserve_manual_skills,
                    })
                    .collect();
                next.active_groups.insert(agent_id.into(), records);
            }
            if suspended_manual.is_empty() {
                next.policy_suspensions.remove(agent_id);
            } else {
                next.policy_suspensions
                    .insert(agent_id.into(), suspended_manual);
            }
            if leaving {
                for (skill_id, path) in &redirected {
                    if let Some(record) = next.skill_provenance.get_mut(skill_id) {
                        if !record.entry_paths.contains(path) {
                            record.entry_paths.push(path.clone());
                        }
                    }
                }
                for target in &removals {
                    tx.reserve(target)?;
                }
                for (target, backup, hash) in &restores {
                    tx.reserve(target)?;
                    linker::copy_tree(backup, target)?;
                    if scanner::dir_content_hash(target)? != *hash {
                        return Err(Error::invalid("Agent 原目录还原校验失败"));
                    }
                }
                for project in &mut next.projects {
                    if let Some(root) = agent.project_root(&project.root) {
                        project
                            .managed_entries
                            .retain(|e| e.target_path.parent() != Some(root.as_path()));
                    }
                }
                for regs in next.registrations.values_mut() {
                    regs.remove(agent_id);
                }
                next.registrations.retain(|_, regs| !regs.is_empty());
            }
            self.store().reserve_config(&mut tx, &next)?;
            self.save_config(&next)?;
            Ok(())
        })();
        if let Err(e) = operation {
            if let Err(recovery) = tx.rollback() {
                return Err(Error::Other(format!(
                    "{e}；回滚未完成，请重启恢复：{recovery}"
                )));
            }
            return Err(e);
        }
        tx.commit()?;
        *config = next;
        Ok(untouched)
    }
}
