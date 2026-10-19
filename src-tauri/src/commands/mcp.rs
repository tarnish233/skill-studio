use crate::AppState;
use serde_json::{json, Value};
use skill_studio_mcp::{
    agents as mcp_agents, config,
    discovery::{self, ScanFile},
    gateway, management, native, registry,
};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tauri::{Manager, State};
fn dir(state: &AppState) -> PathBuf {
    state.studio().store().dir().join("mcp")
}

/// 按「配置备份保留份数」清理最旧的 MCP 备份。清理失败不影响刚完成的写入。
pub async fn prune_backups(dir: PathBuf, keep: usize) {
    match tokio::task::spawn_blocking(move || management::backups::prune(&dir, keep)).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => log::warn!("清理 MCP 配置备份失败: {error:#}"),
        Err(error) => log::warn!("清理 MCP 配置备份失败: {error}"),
    }
}

/// 找旧备份文件时要看的 Agent 配置文件
fn backup_configs(state: &AppState) -> Vec<PathBuf> {
    scan_files(state)
        .0
        .into_iter()
        .map(|file| file.path)
        .collect()
}

pub async fn start(dir: PathBuf) -> Result<(), String> {
    if gateway::request(&dir, "list", Value::Null).await.is_ok() {
        return Ok(());
    }
    config::read(&dir).map_err(|e| e.to_string())?;
    let mut command =
        std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command
        .arg("--mcp-daemon")
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00000008);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if gateway::request(&dir, "list", Value::Null).await.is_ok() {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!(
                "网关启动失败（{status}），请检查端口占用或配置文件"
            ));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    Err("网关启动超时".into())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mcp_request(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    method: String,
    params: Value,
) -> Result<Value, String> {
    if !state.config().settings.manage_mcp {
        return Err("MCP 管理已关闭，请先在设置中开启".into());
    }
    let mut params = params;
    let dir = dir(&state);
    if method == "gatewayCheck" {
        let entry: management::Entry =
            serde_json::from_value(params["entry"].clone()).map_err(|e| e.to_string())?;
        return Ok(json!({"issue":management::gateway_server(&entry).err().map(|e|e.to_string())}));
    }
    if method == "parse" {
        let text = params["text"].as_str().ok_or("请粘贴配置")?;
        return native::parse_definition(text).map_err(|e| e.to_string());
    }
    if method == "searchRegistry" {
        let query = params["query"].as_str().ok_or("请输入搜索词")?;
        let cursor = params["cursor"].as_str();
        return Ok(json!(registry::search(query, cursor)
            .await
            .map_err(|e| e.to_string())?));
    }
    if method == "testDirect" {
        let id = params["id"].as_str().ok_or("缺少 MCP ID")?.to_owned();
        let catalog_dir = dir.clone();
        let entry = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let catalog = management::read(&catalog_dir)?;
            let entry = catalog
                .entries
                .into_iter()
                .find(|e| e.id == id)
                .ok_or_else(|| anyhow::anyhow!("MCP 不存在"))?;
            anyhow::ensure!(entry.mode == "direct", "此服务不是 Agent 直连");
            management::direct_probe_server(&entry)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        return gateway::probe_direct(entry.0, entry.1)
            .await
            .map_err(|e| e.to_string());
    }
    if method == "listBackups" {
        let configs = backup_configs(&state);
        let task_dir = dir.clone();
        return tokio::task::spawn_blocking(move || -> anyhow::Result<Value> {
            Ok(json!({
                "backups": management::backups::list(&task_dir)?,
                "legacy": management::backups::legacy(&task_dir, &configs)?,
            }))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string());
    }
    if method == "deleteBackups" || method == "deleteLegacyBackups" {
        state.ensure_writable().map_err(|e| e.to_string())?;
        let configs = backup_configs(&state);
        let task_dir = dir.clone();
        let removed = tokio::task::spawn_blocking(move || -> anyhow::Result<usize> {
            use management::backups::{self, Selection};
            if method == "deleteLegacyBackups" {
                return backups::delete_legacy(&task_dir, &configs);
            }
            let selection = match params["which"].as_str() {
                Some("unrestorable") => Selection::Unrestorable,
                Some("all") => Selection::All,
                Some(other) => anyhow::bail!("未知的删除范围：{other}"),
                None => Selection::Ids(
                    serde_json::from_value(params["ids"].clone())
                        .map_err(|_| anyhow::anyhow!("缺少备份 ID"))?,
                ),
            };
            backups::delete(&task_dir, &selection)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        return Ok(json!({"removed":removed}));
    }
    if method == "restoreBackup" {
        state.ensure_writable().map_err(|e| e.to_string())?;
        let id = params["id"].as_str().ok_or("缺少备份 ID")?.to_owned();
        let task_dir = dir.clone();
        tokio::task::spawn_blocking(move || management::restore_backup(&task_dir, &id))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let keep = state.config().settings.backup_keep;
        prune_backups(dir.clone(), keep).await;
        let _ = gateway::request(&dir, "list", Value::Null).await;
        return Ok(Value::Null);
    }
    if method == "removeProjectBindings" {
        state.ensure_writable().map_err(|e| e.to_string())?;
        let project_id = params["projectId"].as_str().ok_or("缺少项目 ID")?;
        let project_root = state
            .config()
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .ok_or("项目不存在")?
            .root
            .clone();
        let task_dir = dir.clone();
        let count = tokio::task::spawn_blocking(move || {
            management::remove_project_bindings(&task_dir, &project_root)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        let keep = state.config().settings.backup_keep;
        prune_backups(dir, keep).await;
        return Ok(json!({"removed":count}));
    }
    if matches!(
        method.as_str(),
        "saveGroup" | "removeGroup" | "activateGroup" | "reorderGroups"
    ) {
        state.ensure_writable().map_err(|e| e.to_string())?;
        let keep = state.config().settings.backup_keep;
        let state = state.inner().clone();
        let task_dir = dir.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            use anyhow::Context;
            use management::groups;
            match method.as_str() {
                "saveGroup" => {
                    let group: groups::Group = serde_json::from_value(params["group"].clone())?;
                    let mut imports = vec![];
                    if let Some(requested) = params["imports"].as_array().filter(|v| !v.is_empty())
                    {
                        let (files, _) = scan_files(&state);
                        let scanned = discovery::scan(&files, &[], &task_dir);
                        for requested in requested {
                            let found = scanned
                                .discovered
                                .iter()
                                .find(|d| {
                                    Some(d.id.as_str()) == requested["id"].as_str()
                                        || requested["sourceIds"].as_array().is_some_and(|ids| {
                                            d.sources.iter().any(|source| {
                                                ids.iter().any(|id| {
                                                    id.as_str() == Some(source.id.as_str())
                                                })
                                            })
                                        })
                                })
                                .context("MCP 来源已变化，请刷新后重试")?;
                            let source = found
                                .sources
                                .iter()
                                .find(|source| {
                                    native::canonical(&source.definition, &source.agent)
                                        == requested["definition"]
                                })
                                .context("MCP 来源已变化，请刷新后重试")?;
                            anyhow::ensure!(
                                !found.sources.iter().any(|s| s.gateway),
                                "请先在 Hub 管理此网关入口"
                            );
                            let definition = native::canonical(&source.definition, &source.agent);
                            anyhow::ensure!(
                                definition == requested["definition"],
                                "MCP 来源已变化，请刷新后重试"
                            );
                            let bindings = found
                                .sources
                                .iter()
                                .map(|s| management::Binding {
                                    id: s.id.clone(),
                                    agent: s.agent.clone(),
                                    path: s.path.clone(),
                                    project: s.project.clone(),
                                    key: s.key.clone(),
                                    original: Some(s.definition.clone()),
                                    installed: s.definition.clone(),
                                })
                                .collect();
                            imports.push(management::Entry {
                                id: requested["id"].as_str().context("缺少成员 ID")?.into(),
                                name: found.name.clone(),
                                mode: "direct".into(),
                                definition,
                                oauth: false,
                                client_id: None,
                                scopes: vec![],
                                bindings,
                            });
                        }
                    }
                    groups::save_group(&task_dir, group, imports)
                }
                "removeGroup" => {
                    groups::remove_group(&task_dir, params["id"].as_str().context("缺少分组 ID")?)
                }
                "reorderGroups" => groups::reorder(
                    &task_dir,
                    &serde_json::from_value::<Vec<String>>(params["ids"].clone())?,
                ),
                _ => {
                    let agent = params["agent"].as_str().context("缺少 Agent")?;
                    let ids: Vec<String> = match params.get("ids") {
                        Some(value) => serde_json::from_value(value.clone())?,
                        None => Vec::new(),
                    };
                    if ids.is_empty() {
                        return groups::activate(
                            &task_dir,
                            agent,
                            &[],
                            &PathBuf::new(),
                            &std::env::current_exe()?,
                        );
                    }
                    let app_agent = if agent == "claude" {
                        "claude-code"
                    } else {
                        agent
                    };
                    anyhow::ensure!(
                        !state
                            .config()
                            .settings
                            .disabled_agents
                            .iter()
                            .any(|a| a == app_agent),
                        "请先开启此 Agent 的管理"
                    );
                    let placeholder = management::Entry {
                        id: "group".into(),
                        name: "group".into(),
                        mode: "direct".into(),
                        definition: json!({}),
                        oauth: false,
                        client_id: None,
                        scopes: vec![],
                        bindings: vec![],
                    };
                    let targets = resolve_targets(
                        &state,
                        &task_dir,
                        &json!({"agents":[agent]}),
                        &placeholder,
                        &management::Catalog::default(),
                    )?;
                    let path = &targets.first().context("未找到 Agent 配置")?.path;
                    groups::activate(&task_dir, agent, &ids, path, &std::env::current_exe()?)
                }
            }
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        prune_backups(dir.clone(), keep).await;
        let _ = gateway::request(&dir, "list", Value::Null).await;
        return Ok(Value::Null);
    }
    if method == "saveEntry" || method == "removeEntry" || method == "removeSources" {
        state.ensure_writable().map_err(|e| e.to_string())?;
        let keep = state.config().settings.backup_keep;
        let state = state.inner().clone();
        let method = method.clone();
        let task_dir = dir.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            if method == "removeEntry" {
                return management::remove(
                    &task_dir,
                    params["id"].as_str().ok_or("缺少 ID")?,
                    params["restore"].as_bool().unwrap_or(true),
                )
                .map_err(|e| e.to_string());
            }
            let entry: management::Entry =
                serde_json::from_value(params["entry"].clone()).map_err(|e| e.to_string())?;
            let catalog = management::read(&task_dir).map_err(|e| e.to_string())?;
            let target_params = if method == "removeSources" {
                json!({"sources": params["sources"]})
            } else {
                params.clone()
            };
            let targets = resolve_targets(&state, &task_dir, &target_params, &entry, &catalog)
                .map_err(|e| e.to_string())?;
            if method == "removeSources" {
                return management::remove_sources(&task_dir, targets).map_err(|e| e.to_string());
            }
            let expected: Option<management::Entry> =
                serde_json::from_value(params["expectedEntry"].clone())
                    .map_err(|e| e.to_string())?;
            management::save(
                &task_dir,
                entry,
                targets,
                &std::env::current_exe().map_err(|e| e.to_string())?,
                expected.as_ref(),
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
        prune_backups(dir.clone(), keep).await;
        // Notify a running gateway without starting it. Every gateway request also
        // reads the latest committed catalog, so a missed notification is harmless.
        let _ = gateway::request(&dir, "list", Value::Null).await;
        return Ok(Value::Null);
    }
    if method == "list" {
        let mut value = match gateway::request(&dir, "list", Value::Null).await {
            Ok(value) => value,
            Err(_) => {
                let config = config::read(&dir).map_err(|e| e.to_string())?;
                json!({"running":false,"port":config.port,"servers":config.servers,"bindings":config.bindings})
            }
        };
        let catalog_dir = dir.clone();
        let catalog = tokio::task::spawn_blocking(move || management::read(&catalog_dir))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let (files, warnings) = scan_files(&state);
        let managed: Vec<config::Server> =
            serde_json::from_value(value["servers"].clone()).map_err(|e| e.to_string())?;
        let scanned = tokio::task::spawn_blocking(move || discovery::scan(&files, &managed, &dir))
            .await
            .map_err(|e| e.to_string())?;
        value["gatewayOutdated"] = json!(
            value["running"].as_bool() == Some(true)
                && value["configurationVersion"].as_u64() != Some(3)
        );
        value["groupIssues"] = json!(management::groups::issues(&catalog));
        value["entries"] = json!(catalog.entries);
        value["groups"] = json!(catalog.groups);
        value["backups"] = json!(catalog
            .backups
            .iter()
            .map(|backup| json!({
                "id": backup.id,
                "paths": backup.files.iter().map(|file| &file.path).collect::<Vec<_>>(),
                "createdAt": backup.created_at,
            }))
            .collect::<Vec<_>>());
        value["activeGroups"] = json!(catalog.active_groups);
        value["discovered"] = json!(scanned.discovered);
        value["builtins"] = json!(scanned.builtins);
        value["scanWarnings"] = json!(warnings
            .into_iter()
            .chain(scanned.warnings)
            .collect::<Vec<_>>());
        return Ok(value);
    }
    state.ensure_writable().map_err(|e| e.to_string())?;
    if method == "start" {
        start(dir).await?;
        return Ok(Value::Null);
    }
    if method == "apply" {
        let agent = params["agent"].as_str().ok_or("缺少 Agent")?;
        let config = state.config();
        let project = params["projectId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|id| {
                config
                    .projects
                    .iter()
                    .find(|p| p.id == id)
                    .ok_or("项目不存在")
            })
            .transpose()?;
        let path = mcp_agents::write_path(
            agent,
            &config.settings.agent_dir_overrides,
            project.map(|p| p.root.as_path()),
        )
        .map_err(|e| e.to_string())?;
        params["path"] = json!(path);
    }
    let result = gateway::request(&dir, &method, params)
        .await
        .map_err(|e| e.to_string())?;
    if method == "loginStatus" && result["status"].as_str() == Some("complete") {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }
    if method == "login" {
        use tauri_plugin_opener::OpenerExt;
        let url = result["url"].as_str().ok_or("未返回授权链接")?;
        // The UI always displays a copyable link when a browser cannot be opened.
        let _ = app.opener().open_url(url, None::<&str>);
    }
    Ok(result)
}

fn scan_files(state: &AppState) -> (Vec<ScanFile>, Vec<String>) {
    discovery::scan_files(&state.config())
}

fn resolve_targets(
    state: &AppState,
    dir: &std::path::Path,
    params: &Value,
    entry: &management::Entry,
    catalog: &management::Catalog,
) -> anyhow::Result<Vec<management::Target>> {
    use anyhow::{bail, Context};
    let mut targets = vec![];
    if let Some(ids) = params["bindingIds"].as_array() {
        for id in ids {
            let binding = catalog
                .entries
                .iter()
                .find(|e| e.id == entry.id)
                .and_then(|e| {
                    e.bindings
                        .iter()
                        .find(|b| Some(b.id.as_str()) == id.as_str())
                })
                .context("接入记录已变化，请刷新后重试")?;
            targets.push(management::Target {
                agent: binding.agent.clone(),
                path: binding.path.clone(),
                project: binding.project.clone(),
                key: binding.key.clone(),
                expected: Some(binding.installed.clone()),
            });
        }
    }
    if let Some(sources) = params["sources"].as_array().filter(|v| !v.is_empty()) {
        let (files, _) = scan_files(state);
        let scanned = discovery::scan(&files, &[], dir);
        for source in sources {
            let found = scanned
                .discovered
                .iter()
                .flat_map(|d| &d.sources)
                .find(|s| Some(s.id.as_str()) == source["id"].as_str())
                .context("来源不存在，请刷新后重试")?;
            if found.definition != source["definition"] {
                bail!("来源配置已变化，请刷新后重试");
            }
            if found.gateway {
                bail!("不能再次接管网关转发入口");
            }
            targets.push(management::Target {
                agent: found.agent.clone(),
                path: found.path.clone(),
                project: found.project.clone(),
                key: found.key.clone(),
                expected: Some(found.definition.clone()),
            });
        }
    }
    let config = state.config();
    let scope = params["scope"].as_str().unwrap_or("user");
    if !matches!(scope, "user" | "project" | "local") {
        bail!("无效的安装作用域");
    }
    if scope != "user" && params["projectId"].as_str().is_none_or(str::is_empty) {
        bail!("请选择安装项目");
    }
    if let Some(agents) = params["agents"].as_array() {
        for agent in agents {
            let agent = agent.as_str().context("无效的 Agent")?;
            let mut target_project = None;
            let path = if let Some(id) = params["projectId"].as_str().filter(|s| !s.is_empty()) {
                let project = config
                    .projects
                    .iter()
                    .find(|p| p.id == id)
                    .context("项目不存在")?;
                if scope == "local" {
                    if agent != "claude" {
                        bail!("项目本地作用域仅支持 Claude Code，请选择全局或项目配置");
                    }
                    target_project = Some(project.root.to_string_lossy().to_string());
                    mcp_agents::write_path(agent, &config.settings.agent_dir_overrides, None)?
                } else {
                    mcp_agents::write_path(
                        agent,
                        &config.settings.agent_dir_overrides,
                        Some(&project.root),
                    )?
                }
            } else {
                mcp_agents::write_path(agent, &config.settings.agent_dir_overrides, None)?
            };

            if !targets
                .iter()
                .any(|t| t.agent == agent && t.path == path && t.project == target_project)
            {
                targets.push(management::Target {
                    agent: agent.into(),
                    path,
                    project: target_project,
                    key: entry.name.trim().into(),
                    expected: None,
                });
            }
        }
    }
    if targets.iter().any(|target| target.agent == "grok") {
        let path = mcp_agents::write_path("grok", &config.settings.agent_dir_overrides, None)?;
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e).context("无法检查 Grok 全局停用状态"),
        };
        for target in targets.iter().filter(|target| target.agent == "grok") {
            native::check_activation(&text, "grok", &target.key)?;
        }
    }
    Ok(targets)
}
