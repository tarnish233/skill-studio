use crate::state::AppState;
use skill_studio_core::models::skill::{LinkMode, LinkReport, Skill};
use skill_studio_core::services::studio::SkillView;
use tauri::State;

#[tauri::command(rename_all = "camelCase")]
pub async fn scan_skills(state: State<'_, AppState>) -> Result<Vec<SkillView>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || skill_studio_service::skills::scan_skills(&state))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn register_skills(
    state: State<'_, AppState>,
    skill_ids: Vec<String>,
    agent_ids: Vec<String>,
    mode: Option<LinkMode>,
    force: Option<bool>,
) -> Result<LinkReport, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::register_skills(&state, skill_ids, agent_ids, mode, force)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn unregister_skills(
    state: State<'_, AppState>,
    skill_ids: Vec<String>,
    agent_ids: Vec<String>,
    force: Option<bool>,
) -> Result<LinkReport, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::unregister_skills(&state, skill_ids, agent_ids, force)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_skill_enabled(
    state: State<'_, AppState>,
    skill_id: String,
    agent_id: String,
    enabled: bool,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::set_skill_enabled(&state, skill_id, agent_id, enabled)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn adopt_to_hub(state: State<'_, AppState>, skill_id: String) -> Result<Skill, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::adopt_to_hub(&state, skill_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_hub_backups(
    state: State<'_, AppState>,
) -> Result<Vec<skill_studio_core::services::hub_backups::HubBackup>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::list_hub_backups(&state)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_hub_backups(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<usize, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::delete_hub_backups(&state, ids)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn prune_missing(state: State<'_, AppState>) -> Result<usize, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::prune_missing(&state)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn release_from_hub(
    state: State<'_, AppState>,
    skill_id: String,
) -> Result<Skill, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::release_from_hub(&state, skill_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn search_catalog_skills(
    query: String,
) -> Result<Vec<skill_studio_core::services::marketplace::CatalogSkill>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::search_catalog_skills(query)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn install_catalog_skill(
    state: State<'_, AppState>,
    source: String,
    skill_id: String,
    repository_path: Option<String>,
    repository_paths: Option<Vec<String>>,
) -> Result<skill_studio_service::skills::CatalogInstallResult, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::install_catalog_skill(
            &state,
            source,
            skill_id,
            repository_path,
            repository_paths,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn discover_local_skills(
    path: String,
) -> Result<Vec<skill_studio_core::services::marketplace::LocalSkill>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::discover_local_skills(path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn import_local_skill(
    state: State<'_, AppState>,
    path: String,
) -> Result<skill_studio_core::services::marketplace::ImportOutcome, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::import_local_skill(&state, path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn read_skill_document(
    state: State<'_, AppState>,
    path: std::path::PathBuf,
) -> Result<skill_studio_core::services::scanner::SkillDocument, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::read_skill_document(&state, path)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 用系统默认程序打开 skill 的 `SKILL.md`（Finder／资源管理器里的“打开方式”默认应用）。
///
/// 路径先按当前列表校一遍再交给系统，避免前端持有刷新前的旧路径时打开到别的文件。
#[tauri::command(rename_all = "camelCase")]
pub async fn open_skill_document(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    path: std::path::PathBuf,
) -> Result<(), String> {
    let state = state.inner().clone();
    let directory = path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::skills::ensure_known_skill(&state, &directory)
    })
    .await
    .map_err(|e| e.to_string())??;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(
            path.join("SKILL.md").to_string_lossy().into_owned(),
            None::<&str>,
        )
        .map_err(|e| format!("无法用默认应用打开：{e}"))
}
