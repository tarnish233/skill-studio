use crate::state::AppState;
use skill_studio_core::services::plugins::PluginPackage;
use skill_studio_service::plugins::{
    PluginCapability, PluginInventory, PluginOperation, PluginOperationResult,
};
use std::path::PathBuf;
use tauri::{Emitter, State};

#[tauri::command(rename_all = "camelCase")]
pub async fn scan_plugins(state: State<'_, AppState>) -> Result<PluginInventory, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::plugins::scan_plugins(&state)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn inspect_plugin(
    state: State<'_, AppState>,
    path: PathBuf,
    agent_id: Option<String>,
) -> Result<PluginPackage, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::plugins::inspect_plugin(&state, path, agent_id)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn probe_plugin_cli(state: State<'_, AppState>) -> Result<Vec<PluginCapability>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::plugins::probe_plugin_cli(&state)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn plugin_operation(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    input: PluginOperation,
) -> Result<PluginOperationResult, String> {
    let state = state.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::plugins::plugin_operation(&state, input)
    })
    .await
    .map_err(|error| error.to_string())?;
    // A failed native command may already have changed a declaration or marketplace.
    let _ = app.emit("plugins-changed", ());
    result
}

#[tauri::command(rename_all = "camelCase")]
pub async fn recover_plugin_write(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    confirmed: bool,
) -> Result<PluginOperationResult, String> {
    let state = state.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::plugins::recover_plugin_write(&state, confirmed)
    })
    .await
    .map_err(|error| error.to_string())?;
    let _ = app.emit("plugins-changed", ());
    result
}
