use crate::state::AppState;
use skill_studio_core::models::group::Group;
use tauri::State;

#[tauri::command(rename_all = "camelCase")]
pub async fn list_groups(state: State<'_, AppState>) -> Result<Vec<Group>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || skill_studio_service::groups::list_groups(&state))
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
pub async fn save_group(
    state: State<'_, AppState>,
    group_id: Option<String>,
    name: String,
    skill_ids: Vec<String>,
    expected: Option<skill_studio_service::groups::GroupSnapshot>,
) -> Result<Group, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::groups::save_group(&state, group_id, name, skill_ids, expected)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_agent_group(
    state: State<'_, AppState>,
    agent_id: String,
    group_id: String,
    enabled: bool,
) -> Result<Vec<skill_studio_core::models::group::UntouchedCopy>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::groups::set_agent_group(&state, agent_id, group_id, enabled)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn reapply_agent_groups(
    state: State<'_, AppState>,
    agent_id: String,
) -> Result<Vec<skill_studio_core::models::group::UntouchedCopy>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::groups::reapply_agent_groups(&state, agent_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn activate_agent_group(
    state: State<'_, AppState>,
    agent_id: String,
    group_ids: Vec<String>,
) -> Result<Vec<skill_studio_core::models::group::UntouchedCopy>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::groups::activate_agent_groups(&state, agent_id, group_ids)
    })
    .await
    .map_err(|e| e.to_string())?
}
