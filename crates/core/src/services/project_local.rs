use super::{scanner, studio::Studio};
use crate::{
    fs::paths,
    models::{config::AppConfig, project::ProjectBinding, skill::Skill},
    Error, Result,
};
use std::path::{Path, PathBuf};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLocalSkill {
    #[serde(flatten)]
    pub entry: scanner::ScannedEntry,
    pub collected: bool,
    pub managed: bool,
    pub disabled: bool,
    pub storage_path: PathBuf,
    pub tokens: crate::models::skill::TokenEstimate,
}

impl Studio {
    pub fn project_local_skills(
        &self,
        config: &AppConfig,
        project: &ProjectBinding,
    ) -> Result<Vec<ProjectLocalSkill>> {
        let hub = self.store().hub_dir(config);
        let mut entries = scanner::scan_project(&project.root)?;
        let suspended: Vec<_> = self
            .skill_backups()?
            .into_iter()
            .filter(|r| r.disabled && r.scope == format!("project:{}", project.id))
            .collect();
        for r in &suspended {
            let payload = self.backup_payload(r);
            let agent = crate::models::agent::AGENTS
                .iter()
                .find(|a| a.project_root(&project.root).as_deref() == r.original_path.parent());
            if let Some(a) = agent {
                entries.push(scanner::ScannedEntry {
                    name: r.name.clone(),
                    path: r.original_path.clone(),
                    root: r.original_path.parent().unwrap().into(),
                    agent_id: a.id.into(),
                    kind: scanner::classify_entry(&payload),
                    frontmatter: payload
                        .join("SKILL.md")
                        .is_file()
                        .then(|| scanner::parse_frontmatter(&payload.join("SKILL.md"))),
                });
            }
        }
        entries
            .into_iter()
            .map(|entry| {
                let managed = project
                    .managed_entries
                    .iter()
                    .any(|e| paths::paths_alias(&e.target_path, &entry.path));
                let collected = match &entry.kind {
                    scanner::EntryKind::Link { .. } => {
                        entry.path.canonicalize().is_ok_and(|path| {
                            hub.canonicalize()
                                .is_ok_and(|hub| paths::path_is_within(&hub, &path))
                        })
                    }
                    scanner::EntryKind::ManagedCopy { source_path, .. } => {
                        source_path.canonicalize().is_ok_and(|path| {
                            hub.canonicalize()
                                .is_ok_and(|hub| paths::path_is_within(&hub, &path))
                        })
                    }
                    _ => false,
                };
                let record = suspended.iter().find(|r| r.original_path == entry.path);
                let storage_path = record
                    .map(|r| self.backup_payload(r))
                    .unwrap_or_else(|| entry.path.clone());
                Ok(ProjectLocalSkill {
                    tokens: super::tokens::estimate_skill(&storage_path),
                    disabled: record.is_some(),
                    storage_path,
                    entry,
                    collected,
                    managed,
                })
            })
            .collect()
    }

    /// Project collection shares the same relocation journal as global/local
    /// collection. A stashed disabled file is a backup, not an active source.
    pub fn collect_project_skill(
        &self,
        config: &mut AppConfig,
        project_id: &str,
        path: &Path,
    ) -> Result<Skill> {
        self.checked_skill_path(config, &format!("project:{project_id}"), path)?;
        let project = config
            .project(project_id)
            .ok_or_else(|| Error::NotFound("项目不存在".into()))?;
        let entry = self
            .project_local_skills(config, project)?
            .into_iter()
            .find(|entry| entry.entry.path == path)
            .ok_or_else(|| Error::invalid("项目 skill 已变化，请刷新后重试"))?;
        if entry.managed {
            return Err(Error::invalid("这是项目部署副本，请管理其 Hub 来源"));
        }
        if entry.disabled {
            return Err(Error::invalid(
                "项目原件已移出目录停用，请先恢复原入口再迁移收录；不会复制停用备份",
            ));
        }
        Ok(self.import_local_skill(config, path)?.skill)
    }

    pub fn delete_project_local_skill(
        &self,
        config: &AppConfig,
        project_id: &str,
        path: &Path,
    ) -> Result<()> {
        self.stash_skill(config, &format!("project:{project_id}"), path, false)
    }
}
