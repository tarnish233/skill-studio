mod commands;
mod error;
mod init_status;
pub mod remote;
mod ssh_config;
mod state;
mod watcher;

pub use error::{AppError, AppResult};
pub use state::AppState;

use skill_studio_core::fs::paths;
use skill_studio_core::services::store::Store;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config_dir = paths::config_dir();

    tauri::Builder::default()
        .manage(remote::RemoteState::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Folder {
                        path: config_dir.join("logs"),
                        file_name: Some("skill-studio".into()),
                    },
                ))
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .level(log::LevelFilter::Info)
                .max_file_size(20 * 1024 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(4))
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::mcp_request,
            commands::list_servers,
            commands::list_ssh_hosts,
            commands::save_servers,
            commands::connect_server,
            commands::disconnect_server,
            commands::remote_request,
            commands::answer_ssh_prompt,
            // 窗口 / 元信息
            commands::set_window_theme,
            commands::get_window_material,
            commands::init_native_page_tools,
            commands::update_native_page_tools,
            commands::destroy_native_page_tools,
            commands::get_init_error,
            commands::get_app_version,
            // agent
            commands::list_agents,
            // skill
            commands::scan_skills,
            commands::read_skill_document,
            commands::register_skills,
            commands::unregister_skills,
            commands::set_skill_enabled,
            commands::adopt_to_hub,
            commands::release_from_hub,
            commands::search_catalog_skills,
            commands::install_catalog_skill,
            commands::discover_local_skills,
            commands::import_local_skill,
            commands::prune_missing,
            // 分组
            commands::list_groups,
            commands::save_agent_group,
            commands::activate_agent_group,
            commands::create_group,
            commands::update_group,
            commands::delete_group,
            commands::set_group_skills,
            commands::reorder_groups,
            commands::apply_group,
            // 项目
            commands::list_projects,
            commands::list_project_skills,
            commands::list_skill_backups,
            commands::collect_project_skill,
            commands::delete_skill_file,
            commands::restore_skill_file,
            commands::purge_skill_file,
            commands::set_project_skill_enabled,
            commands::delete_project_local_skill,
            commands::create_project,
            commands::update_project,
            commands::delete_project,
            commands::apply_project,
            commands::set_project_enabled,
            commands::reorder_projects,
            commands::unapply_project,
            commands::write_project_gitignore,
            commands::pick_directory,
            // 设置
            commands::get_config,
            commands::get_settings,
            commands::update_settings,
            commands::list_backups,
            commands::restore_backup,
            commands::get_config_dir,
            commands::reveal_path,
        ])
        .setup(move |app| {
            // 配置加载失败不能直接 panic —— 用户的分组数据可能只是文件坏了，
            // 要让界面起来并引导去 backups/ 恢复。
            match AppState::bootstrap(Store::new(config_dir.clone())) {
                Ok(state) => {
                    app.manage(state.clone());
                    // watcher 必须被持有，drop 掉就停止监听
                    if let Some(w) = watcher::spawn(app.handle().clone(), state) {
                        app.manage(WatcherHandle(std::sync::Mutex::new(w)));
                    }
                }
                Err(err) => {
                    init_status::set(format!(
                        "配置加载失败：{err}。请在设置中恢复备份后重启应用；历史备份位于 {}。",
                        config_dir.join("backups").display()
                    ));
                    // 兜底：用默认配置让界面能起来，但不落盘覆盖坏文件
                    let fallback = AppState::recovery(Store::new(config_dir.clone()));
                    app.manage(fallback);
                }
            }

            // 窗口在 tauri.conf.json 里是 visible: false，等前端挂载好再显示，
            // 避免深色模式下先闪一帧白底。
            if let Some(window) = app.get_webview_window("main") {
                let material = commands::window_material(&window);
                log::info!("Window material: {material}");
                app.manage(commands::WindowMaterial(material));
                let _ = window.show();
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("构建 Skill Studio 失败")
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            tauri::RunEvent::ExitRequested { .. } => {
                // Cmd+Q exits the desktop app and the separate gateway daemon.
                // Bound shutdown time so an unavailable daemon cannot hold up quitting.
                if let Some(state) = app.try_state::<AppState>() {
                    let dir = state.studio().store().dir().join("mcp");
                    tauri::async_runtime::block_on(async {
                        let _ = tokio::time::timeout(
                            std::time::Duration::from_secs(2),
                            skill_studio_mcp::gateway::request(
                                &dir,
                                "stop",
                                serde_json::Value::Null,
                            ),
                        )
                        .await;
                    });
                }
            }
            _ => {}
        });
}

/// 持有 watcher，保证监听在应用生命周期内不被回收
struct WatcherHandle(#[allow(dead_code)] std::sync::Mutex<watcher::SkillWatcher>);
