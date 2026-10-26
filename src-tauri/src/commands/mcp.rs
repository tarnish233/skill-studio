//! MCP 的界面入口。Hub、分组、项目、备份的请求由服务层分派（与命令行、服务器上的辅助程序
//! 共用）；这里只做要异步运行或依赖窗口的部分：网关的启停与转发、连接测试、官方目录搜索、
//! 浏览器授权。
use crate::AppState;
use serde_json::{json, Value};
use skill_studio_mcp::{agents as mcp_agents, config, gateway, registry};
use skill_studio_service::mcp::{self as service, requests};
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

#[tauri::command(rename_all = "camelCase")]
pub async fn mcp_request(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    method: String,
    params: Value,
) -> Result<Value, String> {
    // 官方目录是公开的网络查询：管理服务器时也在本机搜索（服务器可能连不上外网），所以不看
    // 本机的 MCP 管理开关
    if method == "searchRegistry" {
        let query = params["query"].as_str().ok_or("请输入搜索词")?;
        let cursor = params["cursor"].as_str();
        return Ok(json!(registry::search(query, cursor)
            .await
            .map_err(|e| e.to_string())?));
    }
    service::ensure_enabled(&state)?;
    let dir = dir(&state);
    let state = state.inner();
    if method == "testDirect" {
        let id = params["id"].as_str().ok_or("缺少 MCP ID")?.to_owned();
        let (server, startup) = blocking(state, move |s| requests::direct_probe(s, &id)).await?;
        return gateway::probe_direct(server, startup)
            .await
            .map_err(|e| e.to_string());
    }
    if !requests::handles(&method) {
        return gateway_request(state, &app, dir, &method, params).await;
    }
    let gateway = if method == "list" {
        gateway::request(&dir, "list", Value::Null).await.ok()
    } else {
        None
    };
    // 网关接入写进 Agent 配置的命令：桌面应用自己
    let exe = std::env::current_exe().ok();
    let task = method.clone();
    let result = blocking(state, move |s| {
        requests::request(s, exe.as_deref(), gateway, &task, &params)
    })
    .await?;
    // 改动了托管状态：通知网关
    if requests::CATALOG_WRITES.contains(&method.as_str()) {
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
