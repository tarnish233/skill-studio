//! Collect existing Skill directories by relocating their only source into Hub.
//! Catalog downloads are installations; local/project collection never stages an
//! independent import copy and always leaves a symlink, regardless of deployment mode.
use super::{
    marketplace::ImportOutcome,
    scanner,
    studio::{build_skill, Studio},
    variants,
};
use crate::{
    fs::paths,
    models::{
        agent::AGENTS,
        config::AppConfig,
        skill::{skill_id_for, SkillOrigin},
    },
    Error, Result,
};
use std::{fs, path::Path};

impl Studio {
    /// Resolve collected identity against this Hub, not a lexical path hash of
    /// the alias. This also handles configured directory aliases and variants.
    pub fn discover_local_skills(
        &self,
        config: &AppConfig,
        path: &Path,
    ) -> Result<Vec<super::marketplace::LocalSkill>> {
        let mut found = super::marketplace::discover_local(path)?;
        let views = self.scan_skills(config)?;
        for candidate in &mut found {
            candidate.hub_id = views
                .iter()
                .find(|view| {
                    matches!(view.skill.origin, SkillOrigin::Hub)
                        && view.diagnostics.is_empty()
                        && (paths::paths_alias(&view.skill.source_path, &candidate.path)
                            || variants::entries(config, &view.skill)
                                .iter()
                                .any(|variant| {
                                    variants::payload(&view.skill.source_path, &variant.key)
                                        .is_ok_and(|payload| {
                                            paths::paths_alias(&payload, &candidate.path)
                                        })
                                }))
                })
                .map(|view| view.skill.id.clone());
        }
        Ok(found)
    }

    /// The selected directory is the source, not a temporary copy. Re-selecting
    /// an existing Hub source or a symlink to it is a read-only no-op.
    pub fn import_local_skill(&self, config: &mut AppConfig, path: &Path) -> Result<ImportOutcome> {
        let source = path
            .canonicalize()
            .map_err(|error| Error::io(path, error))?;
        if source
            .join(scanner::COPY_SIDECAR)
            .symlink_metadata()
            .is_ok()
        {
            return Err(Error::invalid(
                "这是部署副本，不能再次收录；请托管其来源原件",
            ));
        }
        let views = self.scan_skills(config)?;
        if let Some(view) = views.iter().find(|view| {
            matches!(view.skill.origin, SkillOrigin::Hub)
                && (paths::paths_alias(&view.skill.source_path, &source)
                    || variants::entries(config, &view.skill)
                        .iter()
                        .any(|variant| {
                            variants::payload(&view.skill.source_path, &variant.key)
                                .is_ok_and(|payload| paths::paths_alias(&payload, &source))
                        }))
        }) {
            if !view.diagnostics.is_empty() {
                return Err(Error::invalid("Hub 来源不可用，请先检查托管目录"));
            }
            return Ok(ImportOutcome {
                skill: view.skill.clone(),
                changed: false,
                original_path: path.to_path_buf(),
            });
        }
        // A referenced external source is not implicitly owned. Selecting its
        // real directory is an explicit relocation; selecting only an alias is not.
        if scanner::is_symlink_or_junction(path) {
            return Err(Error::invalid(
                "此入口只引用外部目录；请明确选择源目录进行迁移托管，不会复制收录",
            ));
        }
        validate_source(&source)?;
        let skill = if let Some(view) = views
            .iter()
            .find(|view| paths::paths_alias(&view.skill.source_path, &source))
        {
            let mut skill = view.skill.clone();
            if matches!(skill.origin, SkillOrigin::External) {
                skill.origin = SkillOrigin::Local;
            }
            skill
        } else {
            build_skill(
                skill_id_for(&source),
                source.file_name().unwrap().to_str().unwrap(),
                source.clone(),
                source.parent().unwrap().to_path_buf(),
                SkillOrigin::Local,
                Some(scanner::parse_frontmatter(
                    &source.join(scanner::SKILL_FILE),
                )),
            )
        };
        let original_path = skill.source_path.clone();
        let skill = self.relocate_hub_skill(config, skill, false)?;
        Ok(ImportOutcome {
            skill,
            changed: true,
            original_path,
        })
    }
}

/// Bound the complete source tree before copying recovery snapshots. Links are
/// preserved, never traversed; deployment sidecars are not independent originals.
pub(super) fn validate_source(source: &Path) -> Result<()> {
    scanner::validate_sync_source(source)?;
    let _name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| {
            !name.is_empty()
                && !name.starts_with('.')
                && !name.contains(['/', '\\', ':'])
                && !AGENTS[0].is_reserved_dir(name)
        })
        .ok_or_else(|| Error::invalid("Skill 目录名称无效或为 Hub 保留名称"))?;
    if source
        .join(scanner::COPY_SIDECAR)
        .symlink_metadata()
        .is_ok()
    {
        return Err(Error::invalid("部署副本不能作为托管原件"));
    }
    if source.join(variants::DIRECTORY).symlink_metadata().is_ok() {
        return Err(Error::invalid(
            "多端安装包须通过仓库安装，不能作为单个本地原件收录",
        ));
    }
    if scanner::parse_frontmatter(&source.join(scanner::SKILL_FILE)).malformed {
        return Err(Error::invalid("SKILL.md 的 YAML 格式无效，请修复后再托管"));
    }
    validate_tree(source)
}

pub(super) fn validate_tree(source: &Path) -> Result<()> {
    fn check(dir: &Path, depth: usize, count: &mut usize, size: &mut u64) -> Result<()> {
        if depth > scanner::MAX_SCAN_DEPTH {
            return Err(Error::invalid("Skill 目录层级过深"));
        }
        for entry in fs::read_dir(dir).map_err(|error| Error::io(dir, error))? {
            let entry = entry.map_err(|error| Error::io(dir, error))?;
            let path = entry.path();
            let meta = path
                .symlink_metadata()
                .map_err(|error| Error::io(&path, error))?;
            *count += 1;
            *size = size.saturating_add(meta.len());
            if *count > 20_000 || *size > 256 * 1024 * 1024 {
                return Err(Error::invalid("Skill 文件数量或大小超过限制"));
            }
            if scanner::is_symlink_or_junction(&path) {
                continue;
            }
            if meta.is_dir() {
                check(&path, depth + 1, count, size)?;
            } else if !meta.is_file() {
                return Err(Error::invalid("Skill 包含不支持的特殊文件"));
            }
        }
        Ok(())
    }
    check(source, 0, &mut 0, &mut 0)
}
