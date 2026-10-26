use crate::state::AppState;
use skill_studio_core::models::config::{AppConfig, Settings};
use skill_studio_service::settings::SettingsPatch;
use tauri::State;

#[tauri::command(rename_all = "camelCase")]
pub async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || skill_studio_service::settings::get_config(&state))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::settings::get_settings(&state)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_settings(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    patch: SettingsPatch,
) -> Result<Settings, String> {
    let bundle = if patch.cli_skill_enabled == Some(true) {
        Some(crate::builtin_skills_dir(&app).ok_or("找不到应用内置 skill 资源，不能启用")?)
    } else {
        None
    };
    let changed_mcp = patch.manage_mcp;
    let changed_keep = patch.backup_keep.is_some();
    let mcp_dir = state.studio().store().dir().join("mcp");
    let enabling = changed_mcp == Some(true) && !state.config().settings.manage_mcp;
    let mut started_gateway = false;
    if enabling {
        let catalog = skill_studio_mcp::management::read(&mcp_dir).map_err(|e| e.to_string())?;
        let needs_gateway = catalog
            .entries
            .iter()
            .any(|entry| entry.mode == "gateway" && !entry.bindings.is_empty())
            || catalog
                .active_groups
                .values()
                .flatten()
                .any(|group| group.entries.iter().any(|entry| entry.mode == "gateway"));
        if needs_gateway
            && skill_studio_mcp::gateway::request(&mcp_dir, "list", serde_json::Value::Null)
                .await
                .is_err()
        {
            crate::commands::mcp::start(mcp_dir.clone()).await?;
            started_gateway = true;
        }
    }
    let state = state.inner().clone();
    let updated = tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::settings::update_settings_with_builtin(
            &state,
            patch,
            bundle.as_deref(),
            env!("CARGO_PKG_VERSION"),
        )
    })
    .await
    .map_err(|e| e.to_string())?;
    let settings = match updated {
        Ok(settings) => settings,
        Err(error) => {
            if started_gateway {
                let _ =
                    skill_studio_mcp::gateway::request(&mcp_dir, "stop", serde_json::Value::Null)
                        .await;
            }
            return Err(error);
        }
    };
    if changed_keep {
        // 保留份数与 MCP 配置备份共用：调小后立即清理，不必等下一次写入
        let dir = mcp_dir.clone();
        let keep = settings.backup_keep;
        let _ = tauri::async_runtime::spawn_blocking(move || {
            skill_studio_service::mcp::prune_backups(&dir, keep)
        })
        .await;
    }
    if let Some(enabled) = changed_mcp {
        if enabled {
            let _ =
                skill_studio_mcp::gateway::request(&mcp_dir, "list", serde_json::Value::Null).await;
        } else {
            let _ =
                skill_studio_mcp::gateway::request(&mcp_dir, "stop", serde_json::Value::Null).await;
        }
    }
    Ok(settings)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_backups(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::settings::list_backups(&state)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_backup(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::settings::delete_backup(&state, path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn restore_backup(state: State<'_, AppState>, path: String) -> Result<AppConfig, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::settings::restore_backup(&state, path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_config_dir(state: State<'_, AppState>) -> Result<String, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skill_studio_service::settings::get_config_dir(&state)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub fn reveal_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| format!("打开路径失败: {e}"))
}
