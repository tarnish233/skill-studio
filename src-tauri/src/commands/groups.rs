use crate::state::AppState;
use skill_studio_core::models::group::{Group, GroupApplyMode};
use skill_studio_core::models::skill::LinkReport;
use tauri::State;

#[tauri::command(rename_all = "camelCase")]
pub async fn list_groups(state: State<'_, AppState>) -> Result<Vec<Group>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || skill_studio_service::groups::list_groups(&state))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn create_group(
    state: State<'_, AppState>,
    name: String,
    description: Option<String>,
    icon: Option<String>,
) -> Result<Group, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::groups::create_group(&state, name, description, icon)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_group(
    state: State<'_, AppState>,
    group_id: String,
    name: Option<String>,
    description: Option<String>,
    icon: Option<String>,
) -> Result<Group, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::groups::update_group(&state, group_id, name, description, icon)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_group(state: State<'_, AppState>, group_id: String) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::groups::delete_group(&state, group_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_group_skills(
    state: State<'_, AppState>,
    group_id: String,
    skill_ids: Vec<String>,
) -> Result<Group, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::groups::set_group_skills(&state, group_id, skill_ids)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn reorder_groups(
    state: State<'_, AppState>,
    group_ids: Vec<String>,
) -> Result<Vec<Group>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::groups::reorder_groups(&state, group_ids)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn apply_group(
    state: State<'_, AppState>,
    group_id: String,
    agent_ids: Vec<String>,
    mode: GroupApplyMode,
    force: Option<bool>,
) -> Result<LinkReport, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::groups::apply_group(&state, group_id, agent_ids, mode, force)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn save_group(
    state: State<'_, AppState>,
    group_id: Option<String>,
    name: String,
    skill_ids: Vec<String>,
) -> Result<Group, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::groups::save_group(&state, group_id, name, skill_ids)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn activate_agent_group(
    state: State<'_, AppState>,
    agent_id: String,
    group_id: Option<String>,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::groups::activate_agent_group(&state, agent_id, group_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
