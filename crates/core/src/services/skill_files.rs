//! Recoverable skill deletion and project-local suspension.
use super::{linker, scanner, studio::Studio, transaction::Transaction};
use crate::{
    fs::{atomic, paths},
    models::{agent::AGENTS, config::AppConfig},
    Error, Result,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillBackup {
    pub id: String,
    pub scope: String,
    pub name: String,
    pub original_path: PathBuf,
    pub deleted_at: i64,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub restore_disabled: bool,
}
impl Studio {
    pub fn skill_backups(&self) -> Result<Vec<SkillBackup>> {
        let root = self.store().dir().join("skill-backups");
        if !root.exists() {
            return Ok(vec![]);
        }
        let mut out = vec![];
        for e in fs::read_dir(&root).map_err(|e| Error::io(&root, e))? {
            let e = e.map_err(|e| Error::io(&root, e))?;
            let p = e.path().join("record.json");
            if !p.is_file() || !e.path().join("payload").symlink_metadata().is_ok() {
                continue;
            }
            let r: SkillBackup =
                serde_json::from_slice(&fs::read(&p).map_err(|e| Error::io(&p, e))?)
                    .map_err(|e| Error::json(&p, e))?;
            if e.file_name()
                .to_string_lossy()
                .starts_with(&format!(".{}.backup-", r.id))
            {
                continue;
            }
            if e.file_name().to_string_lossy() != r.id {
                return Err(Error::invalid("备份记录不匹配"));
            }
            out.push(r);
        }
        out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at).then(a.id.cmp(&b.id)));
        Ok(out)
    }
    pub fn backup_payload(&self, r: &SkillBackup) -> PathBuf {
        self.store()
            .dir()
            .join("skill-backups")
            .join(&r.id)
            .join("payload")
    }
    fn backup_record(&self, id: &str) -> Result<SkillBackup> {
        self.skill_backups()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| Error::NotFound("skill 备份".into()))
    }
    fn checked_skill_path(&self, config: &AppConfig, scope: &str, path: &Path) -> Result<()> {
        if !path.is_absolute()
            || path.file_name().is_none()
            || path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(Error::invalid("无效 skill 路径"));
        }
        let parent = path.parent().ok_or_else(|| Error::invalid("无效路径"))?;
        if let Some(id) = scope.strip_prefix("project:") {
            let p = config
                .project(id)
                .ok_or_else(|| Error::NotFound("项目".into()))?;
            if !AGENTS
                .iter()
                .filter_map(|a| a.project_root(&p.root))
                .any(|r| r == parent)
            {
                return Err(Error::invalid("不是项目 skill 路径"));
            }
            let rp = p.root.canonicalize().map_err(|e| Error::io(&p.root, e))?;
            let real = parent.canonicalize().map_err(|e| Error::io(parent, e))?;
            if !real.starts_with(rp) {
                return Err(Error::invalid("项目 skill 目录指向外部"));
            }
            let globals: Vec<_> = AGENTS
                .iter()
                .flat_map(|a| a.resolved_global_roots(&config.settings.agent_dir_overrides))
                .chain(std::iter::once(self.store().hub_dir(config)))
                .collect();
            linker::ensure_distinct_roots(parent, &globals)?;
        } else if let Some(id) = scope.strip_prefix("agent:") {
            let a = crate::models::agent::require_agent(id)?;
            if config.settings.disabled_agents.iter().any(|x| x == id) {
                return Err(Error::invalid("请先开启 Agent 管理"));
            }
            if !a
                .resolved_global_roots(&config.settings.agent_dir_overrides)
                .iter()
                .any(|r| paths::paths_alias(r, parent))
            {
                return Err(Error::invalid("不是 Agent skill 路径"));
            }
        } else if scope != "hub" || !paths::paths_alias(parent, &self.store().hub_dir(config)) {
            return Err(Error::invalid("不是 Hub skill 路径"));
        }
        Ok(())
    }
    pub fn stash_skill(
        &self,
        config: &AppConfig,
        scope: &str,
        path: &Path,
        disabled: bool,
    ) -> Result<()> {
        self.checked_skill_path(config, scope, path)?;
        if let Some(r) = self
            .skill_backups()?
            .into_iter()
            .find(|r| r.disabled && r.scope == scope && r.original_path == path)
        {
            if path.symlink_metadata().is_ok() {
                return Err(Error::invalid("原位置出现同名内容，请先处理冲突"));
            }
            if disabled {
                return Ok(());
            }
            let payload = self.backup_payload(&r);
            if scanner::is_symlink_or_junction(&payload) {
                return self.purge_skill_backup(&r.id);
            }
            let mut next = r;
            next.disabled = false;
            next.restore_disabled = true;
            return atomic::write_json_file(&payload.parent().unwrap().join("record.json"), &next);
        }
        let uses_source = |source: &Path| {
            let base = path.canonicalize().unwrap_or_else(|_| path.into());
            let target = source.canonicalize().unwrap_or_else(|_| {
                scanner::link_destination(source).unwrap_or_else(|_| source.into())
            });
            paths::paths_alias(path, source) || paths::path_is_within(&base, &target)
        };
        if config.active_groups.values().flatten().any(|g| {
            g.entries
                .iter()
                .any(|e| e.target_path == path || uses_source(&e.source_path))
        }) || config.projects.iter().any(|p| {
            p.managed_entries
                .iter()
                .any(|e| e.target_path == path || uses_source(&e.source_path))
        }) {
            return Err(Error::invalid(
                "此 skill 正被分组或项目使用，请先停用或解除绑定",
            ));
        }
        if !scanner::is_symlink_or_junction(path) {
            for agent in AGENTS {
                for root in agent.resolved_global_roots(&config.settings.agent_dir_overrides) {
                    for entry in scanner::scan_root(&root, agent)? {
                        if entry.path != path
                            && scanner::is_symlink_or_junction(&entry.path)
                            && uses_source(&entry.path)
                        {
                            return Err(Error::invalid(
                                "其他 Agent 仍通过软链接使用此 skill，请先移除对应链接",
                            ));
                        }
                    }
                }
            }
        }
        if !path.join("SKILL.md").is_file() && !scanner::is_symlink_or_junction(path) {
            return Err(Error::invalid("skill 已不存在"));
        }
        if disabled && !scope.starts_with("project:") {
            return Err(Error::invalid("仅支持项目暂存"));
        }
        let mut tx = Transaction::begin(self.store().dir().join("skill-files.json"))?;
        let operation = (|| {
            if !disabled && scanner::is_symlink_or_junction(path) {
                tx.reserve(path)?;
                return Ok(());
            }
            let root = self.store().dir().join("skill-backups");
            fs::create_dir_all(&root).map_err(|e| Error::io(&root, e))?;
            let folder = tempfile::tempdir_in(&root)
                .map_err(|e| Error::io(&root, e))?
                .keep();
            let id = folder.file_name().unwrap().to_string_lossy().into_owned();
            tx.reserve(&folder)?;
            fs::create_dir_all(&folder).map_err(|e| Error::io(&folder, e))?;
            let payload = folder.join("payload");
            if scanner::is_symlink_or_junction(path) {
                linker::create_symlink(&scanner::link_destination(path)?, &payload)?;
            } else {
                let before = scanner::backup_content_hash(path)?;
                copy_exact(path, &payload, 0)?;
                if scanner::backup_content_hash(path)? != before
                    || scanner::backup_content_hash(&payload)? != before
                {
                    return Err(Error::invalid("备份时源文件发生变化"));
                }
            }
            atomic::write_json_file(
                &folder.join("record.json"),
                &SkillBackup {
                    id,
                    scope: scope.into(),
                    name: path.file_name().unwrap().to_string_lossy().into_owned(),
                    original_path: path.into(),
                    deleted_at: linker::now_secs(),
                    disabled,
                    restore_disabled: false,
                },
            )?;
            tx.reserve(path)?;
            Ok(())
        })();
        match operation {
            Ok(()) => tx.commit(),
            Err(e) => {
                tx.rollback()?;
                Err(e)
            }
        }
    }
    pub fn restore_skill_backup(&self, config: &AppConfig, id: &str) -> Result<()> {
        let mut record = self.backup_record(id)?;
        self.checked_skill_path(config, &record.scope, &record.original_path)?;
        if record.original_path.symlink_metadata().is_ok() {
            return Err(Error::invalid("原位置已有内容，未覆盖；请先处理冲突"));
        }
        let payload = self.backup_payload(&record);
        if record.restore_disabled {
            record.disabled = true;
            record.restore_disabled = false;
            return atomic::write_json_file(
                &payload.parent().unwrap().join("record.json"),
                &record,
            );
        }
        let mut tx = Transaction::begin(self.store().dir().join("skill-files.json"))?;
        let operation = (|| {
            tx.reserve(&record.original_path)?;
            if scanner::is_symlink_or_junction(&payload) {
                linker::create_symlink(
                    &scanner::link_destination(&payload)?,
                    &record.original_path,
                )?;
            } else {
                copy_exact(&payload, &record.original_path, 0)?;
                if scanner::backup_content_hash(&payload)?
                    != scanner::backup_content_hash(&record.original_path)?
                {
                    return Err(Error::invalid("恢复校验失败"));
                }
            }
            tx.reserve(payload.parent().unwrap())?;
            Ok(())
        })();
        match operation {
            Ok(()) => tx.commit(),
            Err(e) => {
                tx.rollback()?;
                Err(e)
            }
        }
    }
    pub fn purge_skill_backup(&self, id: &str) -> Result<()> {
        let record = self.backup_record(id)?;
        let mut tx = Transaction::begin(self.store().dir().join("skill-files.json"))?;
        tx.reserve(self.backup_payload(&record).parent().unwrap())?;
        tx.commit()
    }
}

// Backups preserve metadata sidecars and link text; deployment copying deliberately does not.
fn copy_exact(source: &Path, target: &Path, depth: usize) -> Result<()> {
    if depth > scanner::MAX_SCAN_DEPTH {
        return Err(Error::invalid("目录层级过深"));
    }
    let meta = source
        .symlink_metadata()
        .map_err(|e| Error::io(source, e))?;
    if scanner::is_symlink_or_junction(source) {
        let link = fs::read_link(source).map_err(|e| Error::io(source, e))?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(link, target).map_err(|e| Error::io(target, e))?;
        #[cfg(windows)]
        if source.is_dir() {
            std::os::windows::fs::symlink_dir(link, target).map_err(|e| Error::io(target, e))?;
        } else {
            std::os::windows::fs::symlink_file(link, target).map_err(|e| Error::io(target, e))?;
        }
    } else if meta.is_dir() {
        fs::create_dir_all(target).map_err(|e| Error::io(target, e))?;
        for entry in fs::read_dir(source).map_err(|e| Error::io(source, e))? {
            let entry = entry.map_err(|e| Error::io(source, e))?;
            copy_exact(&entry.path(), &target.join(entry.file_name()), depth + 1)?;
        }
        fs::set_permissions(target, meta.permissions()).map_err(|e| Error::io(target, e))?;
    } else if meta.is_file() {
        fs::copy(source, target).map_err(|e| Error::io(target, e))?;
    } else {
        return Err(Error::invalid("skill 包含特殊文件，无法备份"));
    }
    Ok(())
}

impl Studio {
    /// Upgrade the earlier project-only backups without discarding their source paths.
    pub fn migrate_project_backups(&self, config: &AppConfig) -> Result<()> {
        let old = self.store().dir().join("deleted-project-skills");
        if !old.is_dir() {
            return Ok(());
        }
        for entry in fs::read_dir(&old).map_err(|e| Error::io(&old, e))? {
            let folder = entry.map_err(|e| Error::io(&old, e))?.path();
            if scanner::is_symlink_or_junction(&folder) {
                continue;
            }
            let Some(original) = atomic::read_json_file::<PathBuf>(&folder.join("origin.json"))?
            else {
                continue;
            };
            let Some(name) = original.file_name() else {
                continue;
            };
            let source = folder.join(name);
            if source.symlink_metadata().is_err() {
                continue;
            }
            let Some(project) = config.projects.iter().find(|p| {
                AGENTS
                    .iter()
                    .filter_map(|a| a.project_root(&p.root))
                    .any(|r| Some(r.as_path()) == original.parent())
            }) else {
                continue;
            };
            let id = format!("legacy-{}", folder.file_name().unwrap().to_string_lossy());
            let target = self.store().dir().join("skill-backups").join(&id);
            if target.exists() {
                continue;
            }
            let mut tx = Transaction::begin(self.store().dir().join("skill-files.json"))?;
            let operation = (|| {
                tx.reserve(&target)?;
                fs::create_dir_all(&target).map_err(|e| Error::io(&target, e))?;
                copy_exact(&source, &target.join("payload"), 0)?;
                atomic::write_json_file(
                    &target.join("record.json"),
                    &SkillBackup {
                        id,
                        scope: format!("project:{}", project.id),
                        name: name.to_string_lossy().into_owned(),
                        original_path: original.clone(),
                        deleted_at: linker::now_secs(),
                        disabled: false,
                        restore_disabled: false,
                    },
                )?;
                tx.reserve(&folder)?;
                Ok(())
            })();
            match operation {
                Ok(()) => tx.commit()?,
                Err(e) => {
                    tx.rollback()?;
                    return Err(e);
                }
            }
        }
        Ok(())
    }
}
