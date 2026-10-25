//! MCP 的界面入口。Hub、分组、项目、备份的管理在服务层（与命令行共用）；这里只做要异步
//! 运行或依赖窗口的部分：网关的启停与转发、连接测试、官方目录搜索、浏览器授权。
use crate::AppState;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use skill_studio_mcp::{agents as mcp_agents, config, gateway, management, registry};
use skill_studio_service::mcp as service;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tauri::{Manager, State};
fn dir(state: &AppState) -> PathBuf {
    service::dir(state)
}

/// 写入后通知运行中的网关重读托管状态（不启动它；每次网关请求都会读最新的托管状态，漏一次
/// 通知无妨）。网关没在运行时，替它清掉已删除服务的接入令牌和 OAuth 凭据：删掉最后一个网关
/// 连接的 MCP 后，网关可能不会再启动。
async fn notify_gateway(dir: &std::path::Path) {
    if gateway::request(dir, "list", Value::Null).await.is_ok() {
        return;
    }
    let dir = dir.to_path_buf();
    match tokio::task::spawn_blocking(move || gateway::forget_removed(&dir)).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => log::warn!("清理已删除 MCP 的网关凭据失败: {error:#}"),
        Err(error) => log::warn!("清理已删除 MCP 的网关凭据失败: {error}"),
    }
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

/// 取一个参数；缺少时按 `null` 解析（可选参数得到 `None`）
fn param<T: DeserializeOwned>(params: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(params.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("参数 {key}: {e}"))
}

/// 列表参数：缺少时为空
fn list<T: DeserializeOwned>(params: &Value, key: &str) -> Result<Vec<T>, String> {
    Ok(param::<Option<Vec<T>>>(params, key)?.unwrap_or_default())
}

/// 整个参数对象解析成一个结构（保存位置、项目成员这类平铺在参数里的字段）
fn whole<T: DeserializeOwned>(params: &Value) -> Result<T, String> {
    serde_json::from_value(params.clone()).map_err(|e| format!("参数: {e}"))
}

/// 服务层的操作是同步的文件读写：放到阻塞线程里跑
async fn blocking<T: Send + 'static>(
    state: &AppState,
    task: impl FnOnce(&AppState) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || task(&state))
        .await
        .map_err(|e| e.to_string())?
}

fn to_value(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mcp_request(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    method: String,
    params: Value,
) -> Result<Value, String> {
    service::ensure_enabled(&state)?;
    let dir = dir(&state);
    // 网关接入写进 Agent 配置的命令：桌面应用自己
    let exe = std::env::current_exe().ok();
    let state = state.inner();
    // 改动了托管状态：通知网关
    let notify = matches!(
        method.as_str(),
        "restoreBackup"
            | "saveGroup"
            | "removeGroup"
            | "reorderGroups"
            | "setGroup"
            | "reapplyGroups"
            | "saveProject"
            | "activateProject"
            | "removeProject"
            | "saveEntry"
            | "removeEntry"
            | "removeSources"
    );
    let result = match method.as_str() {
        "gatewayCheck" => {
            let entry: management::Entry = param(&params, "entry")?;
            json!({"issue": service::gateway_issue(&entry)})
        }
        "parse" => service::parse(params["text"].as_str().ok_or("请粘贴配置")?)?,
        "searchRegistry" => {
            let query = params["query"].as_str().ok_or("请输入搜索词")?;
            let cursor = params["cursor"].as_str();
            json!(registry::search(query, cursor)
                .await
                .map_err(|e| e.to_string())?)
        }
        "testDirect" => {
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
            gateway::probe_direct(entry.0, entry.1)
                .await
                .map_err(|e| e.to_string())?
        }
        "listBackups" => to_value(blocking(state, service::backups::list).await?)?,
        "deleteBackups" => {
            let which: Option<String> = param(&params, "which")?;
            let ids: Option<Vec<String>> = param(&params, "ids").map_err(|_| "缺少备份 ID")?;
            let removed = blocking(state, move |s| {
                service::backups::delete(s, which.as_deref(), ids)
            })
            .await?;
            json!({"removed": removed})
        }
        "deleteLegacyBackups" => {
            json!({"removed": blocking(state, service::backups::delete_legacy).await?})
        }
        "restoreBackup" => {
            let id: String = param(&params, "id").map_err(|_| "缺少备份 ID")?;
            blocking(state, move |s| service::backups::restore(s, &id)).await?;
            Value::Null
        }
        "removeProjectBindings" => {
            let id: String = param(&params, "projectId").map_err(|_| "缺少项目 ID")?;
            to_value(blocking(state, move |s| service::projects::remove_bindings(s, &id)).await?)?
        }
        "saveGroup" => {
            let group = param(&params, "group")?;
            let imports: Vec<service::ImportRef> = list(&params, "imports")?;
            let expected: Option<management::groups::GroupSnapshot> = param(&params, "expected")?;
            blocking(state, move |s| {
                service::groups::save(s, group, &imports, expected.as_ref())
            })
            .await?;
            Value::Null
        }
        "removeGroup" => {
            let id: String = param(&params, "id").map_err(|_| "缺少分组 ID")?;
            blocking(state, move |s| service::groups::remove(s, &id)).await?;
            Value::Null
        }
        "reorderGroups" => {
            let ids: Vec<String> = param(&params, "ids")?;
            blocking(state, move |s| service::groups::reorder(s, &ids)).await?;
            Value::Null
        }
        // 只启停这一个分组：这个 Agent 叠加的其他分组由后端按最新状态保留
        "setGroup" => {
            let agent: String = param(&params, "agent").map_err(|_| "缺少 Agent")?;
            let group: String = param(&params, "groupId").map_err(|_| "缺少分组 ID")?;
            let enabled: bool = param(&params, "enabled").map_err(|_| "缺少启停")?;
            to_value(
                blocking(state, move |s| {
                    service::groups::set(s, exe.as_deref(), &agent, &group, enabled)
                })
                .await?,
            )?
        }
        "reapplyGroups" => {
            let agent: String = param(&params, "agent").map_err(|_| "缺少 Agent")?;
            to_value(
                blocking(state, move |s| {
                    service::groups::reapply(s, exe.as_deref(), &agent)
                })
                .await?,
            )?
        }
        "saveProject" => {
            let id: String = param(&params, "projectId").map_err(|_| "缺少项目")?;
            let members: service::projects::Members = whole(&params)?;
            let imports: Vec<service::ImportRef> = list(&params, "imports")?;
            let expected: Option<management::projects::Snapshot> = param(&params, "expected")?;
            to_value(
                blocking(state, move |s| {
                    service::projects::save(
                        s,
                        exe.as_deref(),
                        &id,
                        members,
                        &imports,
                        expected.as_ref(),
                    )
                })
                .await?,
            )?
        }
        "activateProject" => {
            let id: String = param(&params, "projectId").map_err(|_| "缺少项目")?;
            let agent: String = param(&params, "agent").map_err(|_| "缺少 Agent")?;
            let enabled: bool = param(&params, "enabled").map_err(|_| "缺少启停")?;
            to_value(
                blocking(state, move |s| {
                    service::projects::activate(s, exe.as_deref(), &id, &agent, enabled)
                })
                .await?,
            )?
        }
        "removeProject" => {
            let id: String = param(&params, "projectId").map_err(|_| "缺少项目")?;
            blocking(state, move |s| service::projects::remove(s, &id)).await?;
            Value::Null
        }
        "saveEntry" => {
            let entry: management::Entry = param(&params, "entry")?;
            let expected: Option<management::Entry> = param(&params, "expectedEntry")?;
            let placement: service::entries::Placement = whole(&params)?;
            blocking(state, move |s| {
                service::entries::save(s, exe.as_deref(), entry, expected, &placement)
            })
            .await?;
            Value::Null
        }
        "removeEntry" => {
            let id: String = param(&params, "id").map_err(|_| "缺少 ID")?;
            let restore = params["restore"].as_bool().unwrap_or(true);
            to_value(blocking(state, move |s| service::entries::remove(s, &id, restore)).await?)?
        }
        "removeSources" => {
            let sources: Vec<service::entries::SourceRef> = list(&params, "sources")?;
            blocking(state, move |s| {
                service::entries::remove_sources(s, &sources)
            })
            .await?;
            Value::Null
        }
        "list" => {
            let gateway = gateway::request(&dir, "list", Value::Null).await.ok();
            blocking(state, move |s| service::status(s, gateway)).await?
        }
        _ => return gateway_request(state, &app, dir, &method, params).await,
    };
    if notify {
        notify_gateway(&dir).await;
    }
    Ok(result)
}

/// 网关自己的操作：启停、写入网关接入、授权……
async fn gateway_request(
    state: &AppState,
    app: &tauri::AppHandle,
    dir: PathBuf,
    method: &str,
    mut params: Value,
) -> Result<Value, String> {
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
    let result = gateway::request(&dir, method, params)
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
