//! 随应用内置的 skill：启动时同步进 Hub。
//!
//! 内置 skill 和用户自己装进 Hub 的 skill 放在一起，区别只记在
//! `AppConfig::builtin_skills`（id → 上次写入的内容哈希）。同步规则：
//!
//! - Hub 里没有：安装；
//! - 有，但用户改过（当前哈希和记录不符）：原样保留，不覆盖；
//! - 有，且上次安装后没人改过：内容变了就更新，内容没变只更新记录里的版本；
//! - 有，但没有内置记录（用户自己建的同名 skill）：不接管。
//!
//! 文件替换走 `Transaction`：写到一半退出时，下次加载配置会按日志回滚，
//! 不会留下半个 skill 或半份配置。

use std::fs;
use std::path::{Path, PathBuf};

use crate::{
    models::{
        config::{AppConfig, BuiltinSkill},
        skill::{skill_id_for, Skill},
    },
    Error, Result,
};

use super::{linker, scanner, studio::Studio, transaction::Transaction};

impl Studio {
    /// 把 `source` 下直接含 `SKILL.md` 的目录同步进 Hub，返回这次安装或更新的 skill。
    /// `version` 是写入它们时用的应用版本
    pub fn sync_builtin_skills(
        &self,
        config: &mut AppConfig,
        source: &Path,
        version: &str,
    ) -> Result<Vec<Skill>> {
        let hub = self.store().hub_dir(config);
        let mut next = config.clone();
        let mut plan: Vec<Planned> = Vec::new();
        let mut records_changed = false;

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
            if target.symlink_metadata().is_err() {
                plan.push(Planned {
                    source: dir,
                    target,
                    id,
                    hash: source_hash,
                    installed_at: linker::now_secs(),
                });
                continue;
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
