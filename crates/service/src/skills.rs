use skill_studio_core::models::skill::{LinkMode, LinkReport, Skill};
use skill_studio_core::services::hub_backups::HubBackup;
use skill_studio_core::services::studio::SkillView;

use crate::state::AppState;

/// 启动时把随应用提供的 skill 同步进 Hub；用户改过的不覆盖
pub fn sync_builtin_skills(
    state: &AppState,
    source: &std::path::Path,
    version: &str,
) -> Result<Vec<Skill>, String> {
    state
        .transactional(|studio, config| studio.sync_builtin_skills(config, source, version))
        .map_err(Into::into)
}

/// 扫描全部 skill 真身及其在各 agent 上的状态
pub fn scan_skills(state: &AppState) -> Result<Vec<SkillView>, String> {
    state.scan_with_policy().map_err(Into::into)
}

/// 把若干 skill 注册到若干 agent。单条失败不影响其余，结果在报告里逐条给出。
pub fn register_skills(
    state: &AppState,
    skill_ids: Vec<String>,
    agent_ids: Vec<String>,
    mode: Option<LinkMode>,
    force: Option<bool>,
) -> Result<LinkReport, String> {
    state
        .register_skills(&skill_ids, &agent_ids, mode, force.unwrap_or(false))
        .map_err(Into::into)
}

/// 取消注册。只移除本工具管理的内容，用户手工放的会作为失败条目返回。
pub fn unregister_skills(
    state: &AppState,
    skill_ids: Vec<String>,
    agent_ids: Vec<String>,
    force: Option<bool>,
) -> Result<LinkReport, String> {
    state
        .transactional(|studio, config| {
            studio.unregister(config, &skill_ids, &agent_ids, force.unwrap_or(false))
        })
        .map_err(Into::into)
}

/// 通过 agent 原生配置启停某个 skill（不动文件）
pub fn set_skill_enabled(
    state: &AppState,
    skill_id: String,
    agent_id: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .with_exclusive(|studio, config| {
            studio.set_skill_enabled(config, &skill_id, &agent_id, enabled)
        })
        .map_err(Into::into)
}

/// 把一个原地 skill 托管到 Hub 集中管理
pub fn adopt_to_hub(state: &AppState, skill_id: String) -> Result<Skill, String> {
    state.adopt_to_hub(&skill_id).map_err(Into::into)
}

/// 清理引用了已消失 skill 的分组成员与注册记录，返回清理条数
pub fn prune_missing(state: &AppState) -> Result<usize, String> {
    state
        .mutate(|studio, config| studio.prune(config))
        .map_err(Into::into)
}

pub fn release_from_hub(state: &AppState, skill_id: String) -> Result<Skill, String> {
    state.release_from_hub(&skill_id).map_err(Into::into)
}

/// 纳入托管、移出 Hub 时保存的完整备份。恢复模式下不列：配置没加载，判断不了哪份
/// 仍被引用，宁可不列，也不能把还原要用的那份标成可删。
pub fn list_hub_backups(state: &AppState) -> Result<Vec<HubBackup>, String> {
    state.ensure_loaded().map_err(String::from)?;
    let config = state.config();
    state.studio().hub_backups(&config).map_err(Into::into)
}

/// 删除未被引用的完整备份。持有配置读锁：托管和移出 Hub 要拿写锁，
/// 不会在判断「是否仍被引用」之后才写下新的引用。
pub fn delete_hub_backups(state: &AppState, ids: Vec<String>) -> Result<usize, String> {
    state.ensure_writable().map_err(String::from)?;
    let config = state.config();
    state
        .studio()
        .delete_hub_backups(&config, &ids)
        .map_err(Into::into)
}

pub fn search_catalog_skills(
    query: String,
) -> Result<Vec<skill_studio_core::services::marketplace::CatalogSkill>, String> {
    skill_studio_core::services::marketplace::search(&query).map_err(String::from)
}

#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum CatalogInstallResult {
    Installed {
        skill: Box<Skill>,
    },
    SelectionRequired {
        candidates: Vec<skill_studio_core::services::marketplace::CatalogCandidate>,
    },
}

pub fn install_catalog_skill(
    state: &AppState,
    source: String,
    skill_id: String,
    repository_path: Option<String>,
    repository_paths: Option<Vec<String>>,
) -> Result<CatalogInstallResult, String> {
    use skill_studio_core::services::marketplace::{prepare_catalog_variants, CatalogPreparation};
    let selected = repository_paths.unwrap_or_else(|| repository_path.into_iter().collect());
    match prepare_catalog_variants(&source, &skill_id, &selected).map_err(String::from)? {
        CatalogPreparation::Ready(prepared) => Ok(CatalogInstallResult::Installed {
            skill: Box::new(
                state
                    .install_catalog_skill(&prepared)
                    .map_err(String::from)?,
            ),
        }),
        CatalogPreparation::SelectionRequired(candidates) => {
            Ok(CatalogInstallResult::SelectionRequired { candidates })
        }
    }
}

pub fn discover_local_skills(
    path: String,
) -> Result<Vec<skill_studio_core::services::marketplace::LocalSkill>, String> {
    skill_studio_core::services::marketplace::discover_local(std::path::Path::new(&path))
        .map_err(String::from)
}

pub fn import_local_skill(
    state: &AppState,
    path: String,
) -> Result<skill_studio_core::services::marketplace::ImportOutcome, String> {
    let prepared =
        skill_studio_core::services::marketplace::prepare_local(std::path::Path::new(&path))
            .map_err(String::from)?;
    state.import_local_skill(&prepared).map_err(String::from)
}

/// 确认路径属于当前列表中的 skill。
///
/// 预览与“用默认应用打开”都会拿到前端持有的旧路径：列表刷新、skill 被移走或换成
/// 另一个目录后，旧路径可能指向别处。统一在这里拦截，避免拿着陈旧路径去读文件或
/// 交给系统打开。
pub fn ensure_known_skill(state: &AppState, path: &std::path::Path) -> Result<(), String> {
    let config = state.config();
    let mut known = state
        .studio()
        .scan_skills(&config)
        .map_err(String::from)?
        .iter()
        .any(|s| {
            s.skill.source_path.as_path() == path
                || s.agents.values().any(|a| {
                    a.target_path.as_path() == path
                        || a.entry_paths.iter().any(|p| p.as_path() == path)
                })
        });
    if !known {
        for p in &config.projects {
            if state
                .studio()
                .project_local_skills(&config, p)
                .map_err(String::from)?
                .iter()
                .any(|s| s.storage_path.as_path() == path)
            {
                known = true;
                break;
            }
        }
    }
    if !known {
        return Err("skill 已变化或不在当前列表中，请刷新后重试".into());
    }
    Ok(())
}

pub fn read_skill_document(
    state: &AppState,
    path: std::path::PathBuf,
) -> Result<skill_studio_core::services::scanner::SkillDocument, String> {
    ensure_known_skill(state, &path)?;
    skill_studio_core::services::scanner::read_skill_document(&path).map_err(Into::into)
}
