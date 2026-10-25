use crate::state::AppState;
use skill_studio_core::models::project::ProjectBinding;
use skill_studio_core::models::skill::{LinkMode, LinkReport};
use skill_studio_service::projects::ProjectView;
use std::path::PathBuf;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

#[tauri::command(rename_all = "camelCase")]
pub async fn list_project_skills(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<skill_studio_core::services::project_local::ProjectLocalSkill>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::list_project_skills(&state, project_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_project_local_skill(
    state: State<'_, AppState>,
    project_id: String,
    path: PathBuf,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::delete_project_local_skill(&state, project_id, path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectView>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::list_projects(&state)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn create_project(
    state: State<'_, AppState>,
    name: String,
    root: PathBuf,
) -> Result<ProjectBinding, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::create_project(&state, name, root)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_project(
    state: State<'_, AppState>,
    project_id: String,
    name: Option<String>,
    agent_ids: Option<Vec<String>>,
    skill_ids: Option<Vec<String>>,
    group_ids: Option<Vec<String>>,
    link_mode: Option<LinkMode>,
) -> Result<ProjectBinding, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::update_project(
            &state, project_id, name, agent_ids, skill_ids, group_ids, link_mode,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_project(state: State<'_, AppState>, project_id: String) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::delete_project(&state, project_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn apply_project(
    state: State<'_, AppState>,
    project_id: String,
    selection: Option<skill_studio_core::models::project::ProjectSelection>,
) -> Result<LinkReport, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::apply_project(&state, project_id, selection)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn unapply_project(
    state: State<'_, AppState>,
    project_id: String,
    skill_ids: Vec<String>,
    force: Option<bool>,
) -> Result<LinkReport, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::unapply_project(&state, project_id, skill_ids, force)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn write_project_gitignore(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<bool, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::write_project_gitignore(&state, project_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_project_agent(
    state: State<'_, AppState>,
    project_id: String,
    agent_id: String,
    enabled: bool,
) -> Result<LinkReport, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::set_project_agent(&state, project_id, agent_id, enabled)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn reapply_project(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<LinkReport, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::reapply_project(&state, project_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn save_project(
    state: State<'_, AppState>,
    project_id: String,
    members: skill_studio_service::state::ProjectMembers,
    expected: Option<skill_studio_service::state::ProjectMembers>,
) -> Result<LinkReport, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::save_project(&state, project_id, members, expected)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn reorder_projects(
    state: State<'_, AppState>,
    project_ids: Vec<String>,
) -> Result<Vec<ProjectBinding>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::reorder_projects(&state, project_ids)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_skill_backups(
    state: State<'_, AppState>,
) -> Result<Vec<skill_studio_core::services::skill_files::SkillBackup>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::list_skill_backups(&state)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_skill_file(
    state: State<'_, AppState>,
    scope: String,
    path: PathBuf,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::delete_skill_file(&state, scope, path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn restore_skill_file(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::restore_skill_file(&state, id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn purge_skill_file(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::purge_skill_file(&state, id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_project_skill_enabled(
    state: State<'_, AppState>,
    project_id: String,
    path: PathBuf,
    enabled: bool,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::set_project_skill_enabled(&state, project_id, path, enabled)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn collect_project_skill(
    state: State<'_, AppState>,
    project_id: String,
    path: PathBuf,
) -> Result<skill_studio_core::models::skill::Skill, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::projects::collect_project_skill(&state, project_id, path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn pick_directory(
    app: tauri::AppHandle,
    default_path: Option<String>,
) -> Result<Option<String>, String> {
    let initial = default_path
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    // State 不是 Send，不能跨 await；这里只用 AppHandle
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let mut builder = app.dialog().file();
        if let Some(dir) = initial {
            builder = builder.set_directory(dir);
        }
        builder.blocking_pick_folder()
    })
    .await
    .map_err(|e| format!("目录选择器执行失败: {e}"))?;

    Ok(picked.map(|p| p.to_string()))
}
