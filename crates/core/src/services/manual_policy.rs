//! Persistent policy for installed skills outside the active combination.
use super::{
    linker, native_toggle, scanner,
    studio::{SkillView, Studio},
    transaction::Transaction,
};
use crate::{
    fs::paths,
    models::{
        agent::{require_agent, AGENTS},
        config::AppConfig,
        group::GroupEntry,
    },
    Error, Result,
};
use std::path::PathBuf;
type Toggle = (String, String, PathBuf, bool);
pub(crate) fn plan(
    config: &AppConfig,
    agent_id: &str,
    ids: &[String],
    views: &[SkillView],
) -> Result<(Vec<GroupEntry>, Vec<Toggle>)> {
    let agent = require_agent(agent_id)?;
    // Never move shared/manual directories or write another tool's config as a
    // substitute for a native toggle. Group-owned copies still reconcile normally.
    if agent
        .toggle_config_path(&config.settings.agent_dir_overrides)
        .is_none()
    {
        return Ok((Vec::new(), Vec::new()));
    }
    // 叠加多个分组时，判定"这个入口归分组所有"要把所有分组记录的入口合起来看
    let owned = config.active_entries(agent_id);
    let previously_suspended: Vec<_> = config
        .policy_suspensions
        .get(agent_id)
        .into_iter()
        .flatten()
        .collect();
    let mut suspended_manual = Vec::new();
    let mut manual_toggles = Vec::new();
    for view in views {
        let state = &view.agents[agent_id];
        if !state.status.is_registered() {
            continue;
        }
        let mut view = view.clone();
        view.skill = super::variants::deployment_source(config, &view.skill, agent_id)?;
        for target in &state.entry_paths {
            if owned.iter().any(|e| e.target_path == *target) {
                continue;
            }
            if !linker::link_status(&view.skill.source_path, target).is_registered()
                && !paths::paths_alias(target, &view.skill.source_path)
                && !super::studio::restored_entry(config, &view.skill, target)
            {
                continue;
            }
            let was_suspended = previously_suspended.iter().any(|e| {
                e.target_path == *target
                    && (paths::paths_alias(&e.source_path, &view.skill.source_path)
                        || super::studio::restored_entry(config, &view.skill, target))
            });
            let document = target.join(scanner::SKILL_FILE);
            let name = if agent_id == "codex" {
                scanner::parse_frontmatter(&document)
                    .name
                    .unwrap_or_else(|| view.skill.name.clone())
            } else {
                target.file_name().unwrap().to_string_lossy().into_owned()
            };
            let disabled = native_toggle::is_skill_disabled_at(
                agent,
                &config.settings.agent_dir_overrides,
                &name,
                &document,
            );
            let keep = config
                .settings
                .disabled_agents
                .iter()
                .any(|id| id == agent_id)
                || config.settings.preserve_manual_skills
                || ids.contains(&view.skill.id);
            if keep {
                if was_suspended && disabled {
                    manual_toggles.push((view.skill.id.clone(), name, document, true));
                }
            } else if was_suspended || !disabled {
                if !disabled {
                    manual_toggles.push((view.skill.id.clone(), name, document, false));
                }
                suspended_manual.push(GroupEntry {
                    skill_id: view.skill.id.clone(),
                    source_path: view.skill.source_path.clone(),
                    target_path: target.clone(),
                });
            }
        }
    }

    Ok((suspended_manual, manual_toggles))
}
impl Studio {
    pub fn reconcile_manual_policy(&self, config: &mut AppConfig, force_save: bool) -> Result<()> {
        let views = self.scan_skills(config)?;
        let mut next = config.clone();
        let mut updates = Vec::new();
        for agent in AGENTS {
            if config
                .settings
                .disabled_agents
                .iter()
                .any(|id| id == agent.id)
            {
                continue;
            }
            let ids = config.active_skill_ids(agent.id);
            let (suspended, toggles) = plan(config, agent.id, &ids, &views)?;
            if suspended.is_empty() {
                next.policy_suspensions.remove(agent.id);
            } else {
                next.policy_suspensions.insert(agent.id.into(), suspended);
            }
            // 只更新已有的启用记录：没有启用分组时不能凭空造出空记录
            if let Some(records) = next.active_groups.get_mut(agent.id) {
                for group in records {
                    group.preserve_manual_skills = config.settings.preserve_manual_skills;
                }
            }
            if !toggles.is_empty() {
                updates.push((agent, toggles));
            }
        }
        // 管理期间关掉 Grok 对 ~/.claude/skills 的兼容读取，退出管理时撤回
        let isolation = super::isolation::plan(config, &mut next);
        if !force_save
            && updates.is_empty()
            && isolation.is_empty()
            && serde_json::to_value(&next).map_err(|source| Error::JsonSerialize { source })?
                == serde_json::to_value(&*config)
                    .map_err(|source| Error::JsonSerialize { source })?
        {
            return Ok(());
        }
        let mut tx = Transaction::begin(self.store().dir().join("manual-policy.json"))?;
        let result = (|| -> Result<()> {
            for (agent, toggles) in updates {
                let path = agent
                    .toggle_config_path(&config.settings.agent_dir_overrides)
                    .ok_or_else(|| Error::invalid("Agent 不支持启停"))?;
                tx.reserve_file_for_update(&path)?;
                for (_, name, document, enabled) in toggles {
                    native_toggle::set_skill_enabled_at(
                        agent,
                        &config.settings.agent_dir_overrides,
                        &name,
                        &document,
                        enabled,
                    )?;
                }
            }
            for change in &isolation {
                let path = tx.reserve_file_for_update(&change.path)?;
                super::isolation::apply(change, &path)?;
            }
            self.store().reserve_config(&mut tx, &next)?;
            self.save_config(&next)?;
            Ok(())
        })();
        if let Err(error) = result {
            tx.rollback()?;
            return Err(error);
        }
        tx.commit()?;
        *config = next;
        Ok(())
    }
}
