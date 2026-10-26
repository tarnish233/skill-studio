//! 随应用内置的 skill：启动时同步进 Hub。
//!
//! 内置 skill 和用户自己装进 Hub 的 skill 放在一起，区别只记在
//! `AppConfig::builtin_skills`（id → 上次写入的内容哈希）。同步规则：
//!
//! - Hub 里没有：安装；
//! - 有，但用户改过（当前哈希和记录不符）：原样保留，不覆盖；
//! - 有，且上次安装后没人改过：内容变了就更新，内容没变只更新记录里的版本；
//! - 有，但没有内置记录（用户自己建的同名 skill）：不接管。
//! - 关闭 CLI 与内置 skill 时只删除未修改、未被引用或链接的副本；其余留在 Hub 并说明原因。
//!
//! 文件替换走 `Transaction`：写到一半退出时，下次加载配置会按日志回滚，
//! 不会留下半个 skill 或半份配置。

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::{
    fs::paths,
    models::{
        agent::AGENTS,
        config::{AppConfig, BuiltinSkill},
        skill::{skill_id_for, Skill},
    },
    Error, Result,
};

use super::{
    linker,
    scanner::{self, EntryKind},
    studio::Studio,
    transaction::Transaction,
};

impl Studio {
    /// 把 `source` 下直接含 `SKILL.md` 的目录同步进 Hub，返回这次安装或更新的 skill。
    /// `version` 是写入它们时用的应用版本
    pub fn sync_builtin_skills(
        &self,
        config: &mut AppConfig,
        source: &Path,
        version: &str,
    ) -> Result<Vec<Skill>> {
        if !config.settings.cli_skill_enabled {
            // The preference may have been changed in another process; retry removing
            // safe leftovers at startup, but never reinstall them while disabled.
            self.disable_builtin_skills(config)?;
            return Ok(vec![]);
        }
        self.sync_enabled_builtin_skills(config, source, version, false)
    }

    /// Called under the configuration write lock. Enabling reinstalls from the
    /// desktop's bundle as part of the same filesystem/config transaction.
    pub fn set_cli_skill_enabled(
        &self,
        config: &mut AppConfig,
        enabled: bool,
        source: Option<&Path>,
        version: &str,
    ) -> Result<()> {
        if !enabled {
            return self.disable_builtin_skills(config);
        }
        if let Some(source) = source {
            self.sync_enabled_builtin_skills(config, source, version, true)?;
        } else if !config.settings.cli_skill_enabled {
            // Remote helpers have no bundled resource; the desktop synchronizes
            // the skill the next time it opens this configuration.
            let mut next = config.clone();
            next.settings.cli_skill_enabled = true;
            self.save_config(&next)?;
            *config = next;
        }
        Ok(())
    }

    fn sync_enabled_builtin_skills(
        &self,
        config: &mut AppConfig,
        source: &Path,
        version: &str,
        enable_setting: bool,
    ) -> Result<Vec<Skill>> {
        let hub = self.store().hub_dir(config);
        let mut next = config.clone();
        if enable_setting {
            next.settings.cli_skill_enabled = true;
        }
        let mut plan: Vec<Planned> = Vec::new();
        let mut records_changed =
            next.settings.cli_skill_enabled != config.settings.cli_skill_enabled;

        for dir in builtin_sources(source)? {
            let name = dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.is_empty() || name.starts_with('.') {
                continue;
            }
            scanner::validate_sync_source(&dir)?;
            let source_hash = scanner::dir_content_hash(&dir)?;
            let target = hub.join(&name);
            let id = skill_id_for(&target);
            let record = next.builtin_skills.get(&id).cloned();
            match target.symlink_metadata() {
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    plan.push(Planned {
                        source: dir,
                        target,
                        id,
                        hash: source_hash,
                        installed_at: linker::now_secs(),
                    });
                    continue;
                }
                Err(error) => return Err(Error::io(&target, error)),
                Ok(_) if scanner::is_symlink_or_junction(&target) => {
                    // A link replacing our directory is user-controlled even when its
                    // contents happen to match the old builtin byte for byte.
                    continue;
                }
                Ok(_) => {}
            }
            let current = match scanner::dir_content_hash(&target) {
                Ok(hash) => hash,
                Err(error) => {
                    log::warn!("无法读取内置 skill {}：{error}", target.display());
                    continue;
                }
            };
            match record {
                // 用户改过：保留他的版本
                Some(record) if record.installed_hash != current => {}
                // 上次装的还在：内容变了才更新；没变就只记版本
                Some(record) if current != source_hash => plan.push(Planned {
                    source: dir,
                    target,
                    id,
                    hash: source_hash,
                    installed_at: record.installed_at,
                }),
                Some(record) if record.version != version => {
                    next.builtin_skills.insert(
                        id,
                        BuiltinSkill {
                            version: version.into(),
                            installed_hash: current,
                            installed_at: record.installed_at,
                        },
                    );
                    records_changed = true;
                }
                // 不是内置装的同名 skill：不接管
                Some(_) | None => {}
            }
        }

        if plan.is_empty() {
            if records_changed {
                self.save_config(&next)?;
                *config = next;
            }
            return Ok(vec![]);
        }

        let mut tx = Transaction::begin(self.store().dir().join("builtin-skills.json"))?;
        let operation = (|| -> Result<Vec<Skill>> {
            fs::create_dir_all(&hub).map_err(|e| Error::io(&hub, e))?;
            for item in &plan {
                tx.reserve(&item.target)?;
                linker::copy_tree(&item.source, &item.target)?;
                if scanner::dir_content_hash(&item.target)? != item.hash {
                    return Err(Error::invalid(format!(
                        "内置 skill「{}」写入后内容不一致",
                        item.id
                    )));
                }
                next.builtin_skills.insert(
                    item.id.clone(),
                    BuiltinSkill {
                        version: version.into(),
                        installed_hash: item.hash.clone(),
                        installed_at: item.installed_at,
                    },
                );
            }
            self.store().reserve_config(&mut tx, &next)?;
            self.save_config(&next)?;
            let views = self.scan_skills(&next)?;
            Ok(plan
                .iter()
                .filter_map(|item| {
                    views
                        .iter()
                        .find(|view| view.skill.id == item.id)
                        .map(|view| view.skill.clone())
                })
                .collect())
        })();
        match operation {
            Ok(skills) => {
                tx.commit()?;
                *config = next;
                Ok(skills)
            }
            Err(error) => {
                tx.rollback()
                    .map_err(|e| Error::Other(format!("{error}；回滚未完成：{e}")))?;
                Err(error)
            }
        }
    }

    /// The reason each protected builtin must stay in the Hub. Called when the
    /// switch is off so the warning survives a page reload and another launch.
    pub fn builtin_retained(&self, config: &AppConfig) -> Result<Vec<RetainedBuiltin>> {
        let mut plan = self.builtin_cleanup_plan(config)?;
        if !config.settings.cli_skill_enabled {
            // References may be removed while Studio is running. Until another
            // reconciliation actually removes the file, keep warning about it.
            plan.retained
                .extend(plan.removable.into_iter().map(|(_, path)| {
                    RetainedBuiltin {
                        name: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        reason: "已无引用，将在下次启动时安全移除".into(),
                    }
                }));
            plan.retained.sort_by(|a, b| a.name.cmp(&b.name));
        }
        Ok(plan.retained)
    }

    fn disable_builtin_skills(&self, config: &mut AppConfig) -> Result<()> {
        let plan = self.builtin_cleanup_plan(config)?;
        let mut next = config.clone();
        next.settings.cli_skill_enabled = false;
        for id in &plan.missing {
            next.builtin_skills.remove(id);
        }
        for (id, _) in &plan.removable {
            next.builtin_skills.remove(id);
        }
        if plan.removable.is_empty() {
            if config.settings.cli_skill_enabled || !plan.missing.is_empty() {
                self.save_config(&next)?;
                *config = next;
            }
            return Ok(());
        }
        let mut tx = Transaction::begin(self.store().dir().join("builtin-skills.json"))?;
        let operation = (|| -> Result<()> {
            for (id, target) in &plan.removable {
                // Recheck before reserving: an edit since planning must be kept.
                let record = config.builtin_skills.get(id).expect("planned builtin");
                if scanner::is_symlink_or_junction(target)
                    || scanner::dir_content_hash(target)? != record.installed_hash
                {
                    return Err(Error::invalid(format!(
                        "内置 skill「{}」已变化，请重试关闭开关",
                        target.display()
                    )));
                }
                tx.reserve(target)?;
            }
            self.store().reserve_config(&mut tx, &next)?;
            self.save_config(&next)?;
            Ok(())
        })();
        match operation {
            Ok(()) => {
                tx.commit()?;
                *config = next;
                Ok(())
            }
            Err(error) => {
                tx.rollback()
                    .map_err(|e| Error::Other(format!("{error}；回滚未完成：{e}")))?;
                Err(error)
            }
        }
    }

    fn builtin_cleanup_plan(&self, config: &AppConfig) -> Result<CleanupPlan> {
        let hub = self.store().hub_dir(config);
        let entries: Vec<PathBuf> = match fs::read_dir(&hub) {
            Ok(entries) => entries
                .map(|entry| {
                    entry
                        .map(|entry| entry.path())
                        .map_err(|e| Error::io(&hub, e))
                })
                .collect::<Result<_>>()?,
            Err(error) if error.kind() == ErrorKind::NotFound => vec![],
            Err(error) => return Err(Error::io(&hub, error)),
        };
        let mut plan = CleanupPlan::default();
        for (id, record) in &config.builtin_skills {
            let Some(target) = entries.iter().find(|path| skill_id_for(path) == *id) else {
                plan.missing.push(id.clone());
                continue;
            };
            let name = target
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let mut reasons = Vec::new();
            if config.groups.iter().any(|g| g.contains(id))
                || config.projects.iter().any(|p| {
                    p.skill_ids.contains(id) || p.managed_entries.iter().any(|e| &e.skill_id == id)
                })
                || config.active_groups.values().flatten().any(|g| {
                    g.skill_ids.contains(id) || g.entries.iter().any(|e| &e.skill_id == id)
                })
                || config
                    .policy_suspensions
                    .values()
                    .flatten()
                    .any(|e| &e.skill_id == id)
                || config.registrations.contains_key(id)
            {
                reasons.push("仍被分组、项目或 Agent 引用".to_string());
            }
            match target.symlink_metadata() {
                Ok(meta) if !meta.is_dir() || scanner::is_symlink_or_junction(target) => {
                    reasons.push("目录已被替换，不能自动移除".into());
                }
                Ok(_) => match scanner::dir_content_hash(target) {
                    Ok(hash) if hash == record.installed_hash => {}
                    Ok(_) => reasons.push("内容已被修改".into()),
                    Err(error) => reasons.push(format!("无法核对文件：{error}")),
                },
                Err(error) => reasons.push(format!("无法检查目录：{error}")),
            }
            if reasons.is_empty() {
                match linked_by_agent_or_project(config, target) {
                    Ok(true) => reasons.push("其他 Agent 或项目仍有指向它的链接".into()),
                    Ok(false) => {}
                    Err(error) => reasons.push(format!("无法检查 Agent / 项目链接：{error}")),
                }
            }
            if reasons.is_empty() {
                plan.removable.push((id.clone(), target.clone()));
            } else {
                plan.retained.push(RetainedBuiltin {
                    name,
                    reason: reasons.join("；"),
                });
            }
        }
        plan.retained.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(plan)
    }
}

#[derive(Debug)]
pub struct RetainedBuiltin {
    pub name: String,
    pub reason: String,
}

#[derive(Default)]
struct CleanupPlan {
    removable: Vec<(String, PathBuf)>,
    missing: Vec<String>,
    retained: Vec<RetainedBuiltin>,
}

fn linked_by_agent_or_project(config: &AppConfig, target: &Path) -> Result<bool> {
    let overrides = &config.settings.agent_dir_overrides;
    for agent in AGENTS {
        let roots = agent.resolved_global_roots(overrides).into_iter().chain(
            config
                .projects
                .iter()
                .filter_map(|project| agent.project_root(&project.root)),
        );
        for root in roots {
            if scanner::scan_root(&root, agent)?.iter().any(|entry| {
                matches!(entry.kind, EntryKind::Link { .. })
                    && scanner::link_destination(&entry.path)
                        .is_ok_and(|path| paths::paths_alias(&path, target))
            }) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

struct Planned {
    source: PathBuf,
    target: PathBuf,
    id: String,
    hash: String,
    installed_at: i64,
}

/// `source` 下直接含 `SKILL.md` 的目录，按名称排序
fn builtin_sources(source: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(source)
        .map_err(|e| Error::io(source, e))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.join("SKILL.md").is_file())
        .collect();
    dirs.sort();
    Ok(dirs)
}
