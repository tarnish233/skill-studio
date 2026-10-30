use crate::{
    config::{self, Config, Connection, Server},
    oauth::Credentials,
};
use anyhow::{bail, Context, Result};
use axum::{
    extract::{Path as RoutePath, Query, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rmcp::{
    model::*,
    service::{PeerRequestOptions, RequestContext, RunningService},
    transport::{
        auth::{
            AuthClient, AuthorizationManager, AuthorizationRequest, CredentialStore, OAuthState,
        },
        streamable_http_client::StreamableHttpClientTransportConfig,
        streamable_http_server::{
            session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
        },
        StreamableHttpClientTransport,
    },
    ErrorData, RoleClient, RoleServer, ServerHandler, ServiceExt,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, OnceCell};
use tower::ServiceExt as TowerServiceExt;

type Upstream = RunningService<RoleClient, ClientConfig>;
type HttpService = StreamableHttpService<Bridge, LocalSessionManager>;
#[derive(Clone)]
struct Runtime {
    server: Server,
    auth: Option<AuthClient<reqwest::Client>>,
    auth_required: Arc<Mutex<HashSet<String>>>,
}
struct Pending {
    flow_id: String,
    completed: bool,
    oauth: OAuthState,
    started: std::time::Instant,
}
struct App {
    shutdown: tokio::sync::Notify,
    dir: PathBuf,
    config: Mutex<Config>,
    services: Mutex<HashMap<String, HttpService>>,
    credentials: Mutex<HashMap<String, Credentials>>,
    pending: Mutex<HashMap<String, Pending>>,
    auth_required: Arc<Mutex<HashSet<String>>>,
}
#[derive(Clone)]
pub struct Bridge {
    runtime: Runtime,
    upstream: Arc<OnceCell<Upstream>>,
}
fn mcp_err(e: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}
fn http_client(
    url: &str,
    headers: &std::collections::BTreeMap<String, String>,
) -> Result<reqwest::Client> {
    let mut h = reqwest::header::HeaderMap::new();
    for (k, v) in headers {
        h.insert(
            reqwest::header::HeaderName::from_bytes(k.as_bytes())?,
            reqwest::header::HeaderValue::from_str(v)?,
        );
    }
    let mut builder = reqwest::Client::builder();
    if reqwest::Url::parse(url)?
        .host_str()
        .is_some_and(|h| matches!(h, "127.0.0.1" | "localhost" | "[::1]"))
    {
        builder = builder.no_proxy();
    }
    Ok(builder
        .default_headers(h)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .build()?)
}
fn requires_authorization(error: &anyhow::Error) -> bool {
    // rmcp's initialization error owns its transport error but does not expose
    // it via Error::source(), so unwrap that boundary explicitly.
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error.as_ref());
    while let Some(cause) = current {
        if cause.is::<rmcp::transport::streamable_http_client::AuthRequiredError>()
            || matches!(
                cause.downcast_ref::<rmcp::transport::auth::AuthError>(),
                Some(rmcp::transport::auth::AuthError::AuthorizationRequired)
            )
            || cause
                .downcast_ref::<reqwest::Error>()
                .is_some_and(|e| e.status() == Some(StatusCode::UNAUTHORIZED))
        {
            return true;
        }
        current =
            if let Some(rmcp::service::ClientInitializeError::TransportError { error, .. }) =
                cause.downcast_ref::<rmcp::service::ClientInitializeError>()
            {
                Some(error.error.as_ref())
            } else {
                cause.source()
            };
    }
    false
}
async fn connect(runtime: &Runtime) -> Result<Upstream> {
    connect_with_timeout(runtime, Duration::from_secs(30)).await
}
async fn connect_with_timeout(runtime: &Runtime, timeout: Duration) -> Result<Upstream> {
    let info = ClientConfig::default();
    let future = async {
        match &runtime.server.connection {
            Connection::Stdio {
                command,
                args,
                env,
                cwd,
            } => {
                let mut cmd = tokio::process::Command::new(command);
                cmd.args(args).envs(env);
                if let Some(cwd) = cwd {
                    cmd.current_dir(cwd);
                }
                Ok(info.serve(crate::subprocess::transport(cmd)?).await?)
            }
            Connection::Http { url, headers } => {
                let cfg = StreamableHttpClientTransportConfig::with_uri(url.clone());
                if let Some(auth) = &runtime.auth {
                    Ok(info
                        .serve(StreamableHttpClientTransport::with_client(
                            auth.clone(),
                            cfg,
                        ))
                        .await?)
                } else {
                    Ok(info
                        .serve(StreamableHttpClientTransport::with_client(
                            http_client(url, headers)?,
                            cfg,
                        ))
                        .await?)
                }
            }
        }
    };
    let result = tokio::time::timeout(timeout, future)
        .await
        .context("MCP 握手超时")?;
    let mut needed = runtime.auth_required.lock().await;
    match &result {
        Ok(_) => {
            needed.remove(&runtime.server.id);
        }
        Err(error) if requires_authorization(error) => {
            if matches!(&runtime.server.connection, Connection::Http { headers, .. }
                if !headers.keys().any(|k| k.eq_ignore_ascii_case("authorization")))
            {
                needed.insert(runtime.server.id.clone());
            }
        }
        _ => {}
    }
    result
}
/// A one-shot check for an Agent-direct definition. This does not imply that an
/// Agent has reloaded its own config or completed its own login.
pub async fn probe_direct(server: Server, startup_timeout: Duration) -> Result<Value> {
    config::validate(&server)?;
    let runtime = Runtime {
        server,
        auth: None,
        auth_required: Arc::new(Mutex::new(HashSet::new())),
    };
    let up = match connect_with_timeout(&runtime, startup_timeout).await {
        Ok(up) => up,
        Err(error) if requires_authorization(&error) => {
            if matches!(&runtime.server.connection, Connection::Http { headers, .. }
                if headers.keys().any(|k| k.eq_ignore_ascii_case("authorization")))
            {
                bail!("服务拒绝了 Authorization 请求头，请检查 Token 是否有效");
            }
            return Ok(json!({"authRequired": true}));
        }
        Err(error) => return Err(error),
    };
    let result = tokio::time::timeout(Duration::from_secs(30), up.list_all_tools()).await;
    let _ = up.cancel().await;
    Ok(serde_json::to_value(result.context("列出工具超时")??)?)
}
impl Bridge {
    async fn forward<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
        ctx: RequestContext<RoleServer>,
    ) -> Result<T, ErrorData> {
        let upstream = self.upstream.get().ok_or_else(|| mcp_err("尚未初始化"))?;
        let request =
            serde_json::from_value(json!({"method":method,"params":params})).map_err(mcp_err)?;
        let handle = upstream
            .send_request_with_option(
                request,
                PeerRequestOptions::with_timeout(Duration::from_secs(120)),
            )
            .await
            .map_err(mcp_err)?;
        let id = handle.id.clone();
        let peer = handle.peer.clone();
        let result = tokio::select! {
         r=handle.await_response()=>r.map_err(|e|match e {rmcp::ServiceError::McpError(e)=>e,_=>mcp_err(e)})?,
         _=ctx.ct.cancelled()=>{let _=peer.notify_cancelled(CancelledNotificationParam::new(Some(id),None)).await;return Err(mcp_err("请求已取消"));}
        };
        serde_json::from_value(serde_json::to_value(result).map_err(mcp_err)?).map_err(mcp_err)
    }
}
impl ServerHandler for Bridge {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::default()
    }
    async fn initialize(
        &self,
        request: InitializeRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        ctx.peer.set_peer_info(request.clone());
        let up =
            self.upstream
                .get_or_try_init(|| async {
                    connect(&self.runtime).await.map_err(|error| {
                if requires_authorization(&error) {
                    mcp_err("服务需要认证，请在 Studio MCP Hub 中检查连接并登录授权或更新 Token")
                } else { mcp_err(error) }
            })
                })
                .await?;
        let upstream_info = up.peer_info().ok_or_else(|| mcp_err("上游未返回信息"))?;
        let mut info = ServerConfig::new(upstream_info.capabilities.clone()).with_server_info(
            upstream_info
                .server_info
                .clone()
                .unwrap_or_else(Implementation::from_build_env),
        );
        info.instructions = upstream_info.instructions.clone();
        // Advertise only the operations forwarded by this first version. No sampling,
        // elicitation, subscriptions or task extensions are promised to either side.
        let caps = serde_json::to_value(&info.capabilities).map_err(mcp_err)?;
        let mut supported = json!({});
        for name in ["tools", "resources", "prompts"] {
            if caps.get(name).is_some() {
                supported[name] = json!({});
            }
        }
        info.capabilities = serde_json::from_value(supported).map_err(mcp_err)?;
        info.protocol_version = self.negotiate_initialize(&request)?.protocol_version;
        Ok(info)
    }
    async fn list_tools(
        &self,
        p: Option<PaginatedRequestParams>,
        c: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.forward("tools/list", p.unwrap_or_default(), c).await
    }
    async fn call_tool(
        &self,
        p: CallToolRequestParams,
        c: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.forward::<CallToolResult>("tools/call", p, c)
            .await
            .map(Into::into)
    }
    async fn list_prompts(
        &self,
        p: Option<PaginatedRequestParams>,
        c: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        self.forward("prompts/list", p.unwrap_or_default(), c).await
    }
    async fn get_prompt(
        &self,
        p: GetPromptRequestParams,
        c: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        self.forward::<GetPromptResult>("prompts/get", p, c)
            .await
            .map(Into::into)
    }
    async fn list_resources(
        &self,
        p: Option<PaginatedRequestParams>,
        c: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        self.forward("resources/list", p.unwrap_or_default(), c)
            .await
    }
    async fn list_resource_templates(
        &self,
        p: Option<PaginatedRequestParams>,
        c: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        self.forward("resources/templates/list", p.unwrap_or_default(), c)
            .await
    }
    async fn read_resource(
        &self,
        p: ReadResourceRequestParams,
        c: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        self.forward::<ReadResourceResult>("resources/read", p, c)
            .await
            .map(Into::into)
    }
}
impl App {
    async fn reload(&self) -> Result<()> {
        let Some(projection) = crate::management::projection(&self.dir)? else {
            return Ok(());
        };
        if projection.suspended {
            // Temporarily disconnect managed services without revoking OAuth or
            // replacing persistent bridge tokens. Re-enabling reuses both.
            let mut current = self.config.lock().await;
            for old in &current.servers {
                self.invalidate(&old.id).await;
            }
            current.servers.clear();
            return Ok(());
        }
        let servers = projection.servers;
        let mut current = self.config.lock().await;
        if serde_json::to_value(&current.servers)? == serde_json::to_value(&servers)? {
            return Ok(());
        }
        for old in &current.servers {
            let next = servers.iter().find(|s| s.id == old.id);
            let authentication_changed = next.is_none_or(|s| {
                serde_json::to_value(&s.connection).ok()
                    != serde_json::to_value(&old.connection).ok()
                    || s.oauth != old.oauth
                    || s.client_id != old.client_id
                    || s.scopes != old.scopes
            });
            if authentication_changed || next.is_some_and(|s| s.enabled != old.enabled) {
                self.invalidate(&old.id).await;
            }
            if authentication_changed {
                self.pending.lock().await.remove(&old.id);
                self.revoke(&old.id).await?;
            }
        }
        let mut next = current.clone();
        next.servers = servers;
        next.tokens
            .retain(|id, _| next.servers.iter().any(|s| &s.id == id));
        for server in &next.servers {
            next.tokens
                .entry(server.id.clone())
                .or_insert_with(|| uuid::Uuid::new_v4().to_string());
        }
        config::save(&self.dir, &next)?;
        *current = next;
        Ok(())
    }
    async fn credentials(&self, id: &str) -> Credentials {
        self.credentials
            .lock()
            .await
            .entry(id.into())
            .or_insert_with(|| Credentials {
                path: self.dir.join(format!("oauth-{id}.json")),
                refresh: Arc::new(Mutex::new(())),
                active: Arc::new(std::sync::Mutex::new(true)),
            })
            .clone()
    }
    async fn revoke(&self, id: &str) -> Result<()> {
        let credentials = self.credentials(id).await;
        credentials.revoke()?;
        self.credentials.lock().await.remove(id);
        Ok(())
    }
    async fn runtime(&self, server: Server) -> Result<Runtime> {
        // Authentication is driven by saved credentials and the upstream challenge,
        // not the legacy manual OAuth flag. Explicit headers take precedence.
        let credentials = self.credentials(&server.id).await;
        let use_credentials = matches!(&server.connection, Connection::Http { headers, .. }
            if !headers.keys().any(|k| k.eq_ignore_ascii_case("authorization")))
            && credentials
                .load()
                .await?
                .is_some_and(|c| c.token_response.is_some());
        let auth = if use_credentials {
            let Connection::Http { url, headers } = &server.connection else {
                bail!("OAuth 需要 HTTP")
            };
            let mut manager = AuthorizationManager::new(url).await?;
            manager.with_client(
                reqwest::Client::builder()
                    .timeout(Duration::from_secs(30))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()?,
            )?;
            manager.set_credential_store(self.credentials(&server.id).await);
            if !manager.initialize_from_store().await? {
                bail!("需要在 MCP Hub 中登录授权");
            }
            Some(AuthClient::new(http_client(url, headers)?, manager))
        } else {
            None
        };
        Ok(Runtime {
            server,
            auth,
            auth_required: self.auth_required.clone(),
        })
    }
    async fn invalidate(&self, id: &str) {
        self.auth_required.lock().await.remove(id);
        if let Some(s) = self.services.lock().await.remove(id) {
            s.config.cancellation_token.cancel();
        }
    }
}
fn authorized(headers: &HeaderMap, token: &str) -> bool {
    headers.get("origin").is_none()
        && headers.get("authorization").and_then(|h| h.to_str().ok())
            == Some(format!("Bearer {token}").as_str())
}
async fn mcp(
    State(app): State<Arc<App>>,
    RoutePath(id): RoutePath<String>,
    request: Request,
) -> Response {
    if app.reload().await.is_err() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let config = app.config.lock().await.clone();
    let Some(server) = config
        .servers
        .iter()
        .find(|s| s.id == id && s.enabled)
        .cloned()
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !config
        .tokens
        .get(&id)
        .is_some_and(|t| authorized(request.headers(), t))
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    // Serialize service creation so sessions share one OAuth manager/refresh owner.
    let mut services = app.services.lock().await;
    if !services.contains_key(&id) {
        let runtime = match app.runtime(server).await {
            Ok(r) => r,
            Err(_) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "MCP 未就绪，请在 Studio 中测试连接或登录",
                )
                    .into_response()
            }
        };
        let service = StreamableHttpService::new(
            move || {
                Ok(Bridge {
                    runtime: runtime.clone(),
                    upstream: Arc::new(OnceCell::new()),
                })
            },
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default(),
        );
        services.insert(id.clone(), service);
    }
    let service = services[&id].clone();
    drop(services);
    match service.oneshot(request).await {
        Ok(r) => r.into_response(),
        Err(e) => match e {},
    }
}
#[derive(Deserialize)]
struct Rpc {
    method: String,
    #[serde(default)]
    params: Value,
}
async fn admin(State(app): State<Arc<App>>, headers: HeaderMap, Json(rpc): Json<Rpc>) -> Response {
    let token = app.config.lock().await.admin_token.clone();
    if !authorized(&headers, &token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Err(e) = app.reload().await {
        return Json(json!({"error":e.to_string()})).into_response();
    }
    match action(&app, &rpc.method, rpc.params).await {
        Ok(v) => Json(json!({"result":v})).into_response(),
        Err(e) => Json(json!({"error":e.to_string()})).into_response(),
    }
}
async fn action(app: &Arc<App>, method: &str, mut p: Value) -> Result<Value> {
    if app.dir.join("catalog.json").exists()
        && matches!(method, "save" | "delete" | "apply" | "detach")
    {
        bail!("请通过 Studio 配置管理更新 MCP");
    }
    let method = if method == "detach" {
        let c = app.config.lock().await;
        let binding = c
            .bindings
            .iter()
            .find(|b| {
                Some(b.id.as_str()) == p["id"].as_str()
                    && Some(b.path.to_string_lossy().as_ref()) == p["path"].as_str()
            })
            .context("接入记录不存在")?;
        p = json!({"id":binding.id,"agent":binding.agent,"path":binding.path,"remove":true});
        "apply"
    } else {
        method
    };
    if let Some(id) = p.get("id").and_then(Value::as_str) {
        if id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            bail!("无效的 MCP ID");
        }
    }
    match method {
        "stop" => {
            app.shutdown.notify_one();
            Ok(Value::Null)
        }
        "list" => {
            let c = app.config.lock().await.clone();
            let mut servers = vec![];
            for s in &c.servers {
                let mut v = serde_json::to_value(s)?;
                v["authorized"] = json!(app
                    .credentials(&s.id)
                    .await
                    .load()
                    .await?
                    .is_some_and(|c| c.token_response.is_some()));
                v["authRequired"] = json!(app.auth_required.lock().await.contains(&s.id));
                servers.push(v);
            }
            Ok(
                json!({"running":true,"configurationVersion":3,"port":c.port,"servers":servers,"bindings":c.bindings}),
            )
        }
        "save" => {
            let s: Server = serde_json::from_value(p)?;
            config::validate(&s)?;
            let mut c = app.config.lock().await;
            let mut next = c.clone();
            if let Some(old) = next.servers.iter_mut().find(|old| old.id == s.id) {
                // Editing while credentials are live could send old tokens to a new endpoint.
                if serde_json::to_value(&old.connection)? != serde_json::to_value(&s.connection)?
                    || old.oauth != s.oauth
                    || old.client_id != s.client_id
                    || old.scopes != s.scopes
                {
                    app.revoke(&s.id).await?;
                }
                *old = s.clone();
            } else {
                next.servers.push(s.clone());
            }
            next.tokens
                .entry(s.id.clone())
                .or_insert_with(|| uuid::Uuid::new_v4().to_string());
            config::save(&app.dir, &next)?;
            *c = next;
            drop(c);
            app.pending.lock().await.remove(&s.id);
            app.invalidate(&s.id).await;
            Ok(Value::Null)
        }
        "delete" => {
            let id = p["id"].as_str().context("缺少 ID")?;
            let mut c = app.config.lock().await;
            if c.bindings.iter().any(|b| b.id == id) {
                bail!("请先移除该 MCP 的 Agent／项目接入");
            }
            let mut next = c.clone();
            next.servers.retain(|s| s.id != id);
            next.tokens.remove(id);
            config::save(&app.dir, &next)?;
            *c = next;
            drop(c);
            app.invalidate(id).await;
            app.pending.lock().await.remove(id);
            app.revoke(id).await?;
            Ok(Value::Null)
        }
        "test" => {
            let id = p["id"].as_str().context("缺少 ID")?;
            let s = app
                .config
                .lock()
                .await
                .servers
                .iter()
                .find(|s| s.id == id)
                .cloned()
                .context("MCP 不存在")?;
            let runtime = app.runtime(s).await?;
            let up = match connect(&runtime).await {
                Ok(up) => up,
                Err(error) if requires_authorization(&error) => {
                    if matches!(&runtime.server.connection, Connection::Http { headers, .. }
                        if headers.keys().any(|k| k.eq_ignore_ascii_case("authorization")))
                    {
                        bail!("服务拒绝了 Authorization 请求头，请检查 Token 是否有效");
                    }
                    return Ok(json!({"authRequired": true}));
                }
                Err(error) => return Err(error),
            };
            let result = tokio::time::timeout(Duration::from_secs(30), up.list_all_tools()).await;
            let _ = up.cancel().await;
            Ok(serde_json::to_value(result.context("列出工具超时")??)?)
        }
        "loginStatus" => {
            let id = p["id"].as_str().context("缺少 ID")?;
            let flow_id = p["flowId"].as_str().context("缺少授权流程 ID")?;
            let mut pending = app.pending.lock().await;
            let Some(flow) = pending.get(id).filter(|flow| flow.flow_id == flow_id) else {
                return Ok(json!({"status":"expired"}));
            };
            let status = if flow.started.elapsed() > Duration::from_secs(300) {
                "expired"
            } else if flow.completed {
                "complete"
            } else {
                "pending"
            };
            if status != "pending" {
                pending.remove(id);
            }
            Ok(json!({"status":status}))
        }
        "login" => {
            let id = p["id"].as_str().context("缺少 ID")?;
            let c = app.config.lock().await.clone();
            let s = c
                .servers
                .iter()
                .find(|s| s.id == id)
                .context("MCP 不存在")?;
            let Connection::Http { url, .. } = &s.connection else {
                bail!("仅 HTTP 服务支持 OAuth");
            };
            if let Connection::Http { headers, .. } = &s.connection {
                if headers
                    .keys()
                    .any(|k| k.eq_ignore_ascii_case("authorization"))
                {
                    bail!("已配置 Authorization 请求头，请先移除再使用网页登录");
                }
            }
            app.invalidate(id).await;
            let mut manager = AuthorizationManager::new(url).await?;
            manager.with_client(
                reqwest::Client::builder()
                    .timeout(Duration::from_secs(30))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()?,
            )?;
            manager.set_credential_store(app.credentials(id).await);
            let mut state = OAuthState::Unauthorized(manager);
            let mut request =
                AuthorizationRequest::new(format!("http://127.0.0.1:{}/callback/{id}", c.port))
                    .with_client_name("Skill Studio")
                    .with_scopes(s.scopes.clone());
            if let Some(client) = s.client_id.as_ref().filter(|s| !s.is_empty()) {
                request = request.with_preregistered_client(client);
            }
            state.start_authorization(request).await?;
            let url = state.get_authorization_url().await?;
            let flow_id = uuid::Uuid::new_v4().to_string();
            app.pending.lock().await.insert(
                id.into(),
                Pending {
                    flow_id: flow_id.clone(),
                    completed: false,
                    oauth: state,
                    started: std::time::Instant::now(),
                },
            );
            Ok(json!({"url":url,"flowId":flow_id}))
        }
        "logout" => {
            let id = p["id"].as_str().context("缺少 ID")?;
            app.invalidate(id).await;
            app.pending.lock().await.remove(id);
            app.revoke(id).await?;
            Ok(Value::Null)
        }
        "apply" => {
            let mut c = app.config.lock().await;
            let id = p["id"].as_str().context("缺少 ID")?;
            if !c.servers.iter().any(|s| s.id == id) {
                bail!("MCP 不存在");
            }
            let path = PathBuf::from(p["path"].as_str().context("缺少配置路径")?);
            crate::integration::apply(
                &path,
                p["agent"].as_str().context("缺少 Agent")?,
                id,
                std::env::current_exe()?
                    .to_str()
                    .context("应用路径不是 UTF-8")?,
                app.dir.to_str().context("配置路径不是 UTF-8")?,
                p["remove"].as_bool().unwrap_or(false),
            )?;
            let remove = p["remove"].as_bool().unwrap_or(false);
            c.bindings.retain(|b| !(b.id == id && b.path == path));
            if !remove {
                c.bindings.push(config::Binding {
                    id: id.into(),
                    agent: p["agent"].as_str().unwrap().into(),
                    path: path.clone(),
                });
            }
            config::save(&app.dir, &c)?;
            Ok(json!({"path":path}))
        }
        _ => bail!("未知 MCP 操作"),
    }
}
#[derive(Deserialize)]
struct Callback {
    code: Option<String>,
    state: Option<String>,
    iss: Option<String>,
    error: Option<String>,
}
async fn callback(
    State(app): State<Arc<App>>,
    RoutePath(id): RoutePath<String>,
    Query(p): Query<Callback>,
) -> Response {
    if app.reload().await.is_err() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let mut pending = app.pending.lock().await;
    let Some(flow) = pending.get_mut(&id) else {
        return (StatusCode::BAD_REQUEST, "登录已过期，请回到 Studio 重试").into_response();
    };
    if flow.completed {
        return (StatusCode::BAD_REQUEST, "此授权回调已处理").into_response();
    }
    if flow.started.elapsed() > Duration::from_secs(300) {
        pending.remove(&id);
        return (StatusCode::BAD_REQUEST, "登录已过期").into_response();
    }
    if p.error.is_some() {
        return (
            StatusCode::BAD_REQUEST,
            "授权未完成，请回到 Studio 重新登录",
        )
            .into_response();
    }
    let (Some(code), Some(state)) = (p.code, p.state) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match flow
        .oauth
        .handle_callback_with_issuer(&code, &state, p.iss.as_deref())
        .await
    {
        Ok(()) => {
            flow.completed = true;
            drop(pending);
            app.invalidate(&id).await;
            (
                StatusCode::OK,
                "授权成功，正在返回 Skill Studio。此页面可以关闭；若应用未显示，请手动切回。",
            )
                .into_response()
        }
        Err(_) => (
            StatusCode::BAD_REQUEST,
            "授权验证失败，请返回 Studio 重试。",
        )
            .into_response(),
    }
}
pub async fn serve(dir: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&dir)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("daemon.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("MCP 网关已在运行")?;
    let mut config = config::read(&dir)?;
    let listener =
        match tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, config.port)).await {
            Ok(listener) => listener,
            // A quick restart on macOS can leave the old listening port unavailable.
            // The bridge reads config.port for each connection; another free localhost port
            // keeps the gateway usable without touching the locked previous process.
            Err(error) if config.port != 0 && error.kind() == std::io::ErrorKind::AddrInUse => {
                tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?
            }
            Err(error) => return Err(error.into()),
        };
    config.port = listener.local_addr()?.port();
    config::save(&dir, &config)?;
    let app = Arc::new(App {
        shutdown: tokio::sync::Notify::new(),
        dir,
        config: Mutex::new(config),
        services: Mutex::new(HashMap::new()),
        credentials: Mutex::new(HashMap::new()),
        pending: Mutex::new(HashMap::new()),
        auth_required: Arc::new(Mutex::new(HashSet::new())),
    });
    app.reload().await?;
    let router = Router::new()
        .route("/admin", post(admin))
        .route("/mcp/{id}", axum::routing::any(mcp))
        .route("/callback/{id}", get(callback))
        .with_state(app.clone());
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            app.shutdown.notified().await;
            for service in app.services.lock().await.values() {
                service.config.cancellation_token.cancel();
            }
        })
        .await?;
    drop(lock);
    Ok(())
}
/// Desktop-only control channel; never send its bearer token to an upstream server.
/// 网关没在运行时，替它做下次启动时的清理：不再由网关连接的服务从网关配置里去掉，连同接入
/// 令牌和 OAuth 凭据（与 [`App::reload`] 对已删除服务的处理一致），返回清掉的服务数。删掉
/// 最后一个网关连接的 MCP 后网关可能不会再启动，不清的话凭据会一直留在磁盘上。
/// 暂停管理时不清：重新开启后要复用。
pub fn forget_removed(dir: &Path) -> Result<usize> {
    if !dir.join("config.json").is_file() {
        return Ok(0);
    }
    let Some(projection) = crate::management::projection(dir)? else {
        return Ok(0);
    };
    if projection.suspended {
        return Ok(0);
    }
    let live: HashSet<String> = projection.servers.into_iter().map(|s| s.id).collect();
    let mut config = config::read(dir)?;
    let stale: Vec<String> = config
        .servers
        .iter()
        .map(|s| s.id.clone())
        .filter(|id| !live.contains(id))
        .collect();
    if stale.is_empty() {
        return Ok(0);
    }
    for id in &stale {
        match std::fs::remove_file(dir.join(format!("oauth-{id}.json"))) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    config.servers.retain(|s| live.contains(&s.id));
    config.tokens.retain(|id, _| live.contains(id));
    config::save(dir, &config)?;
    Ok(stale.len())
}
pub async fn request(dir: &Path, method: &str, params: Value) -> Result<Value> {
    let c = config::read(dir)?;
    let value: Value = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(if method == "list" { 2 } else { 150 }))
        .build()?
        .post(format!("http://127.0.0.1:{}/admin", c.port))
        .bearer_auth(c.admin_token)
        .json(&json!({"method":method,"params":params}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if let Some(e) = value.get("error") {
        bail!("{}", e.as_str().unwrap_or("网关错误"));
    }
    Ok(value["result"].clone())
}

/// A thin stdio adapter keeps the gateway bearer credential out of project files.
pub async fn bridge_stdio(dir: PathBuf, id: String) -> Result<()> {
    if crate::management::read(&dir)?.suspended {
        bail!("MCP 管理已关闭");
    }
    let c = config::read(&dir)?;
    let token = c
        .tokens
        .get(&id)
        .context("MCP 不存在，请先在 Skill Studio 中配置")?;
    let mut headers = std::collections::BTreeMap::new();
    headers.insert("Authorization".into(), format!("Bearer {token}"));
    let server = Server {
        id: id.clone(),
        name: id.clone(),
        connection: Connection::Http {
            url: format!("http://127.0.0.1:{}/mcp/{id}", c.port),
            headers,
        },
        enabled: true,
        oauth: false,
        client_id: None,
        scopes: vec![],
    };
    Bridge {
        runtime: Runtime {
            server,
            auth: None,
            auth_required: Arc::new(Mutex::new(HashSet::new())),
        },
        upstream: Arc::new(OnceCell::new()),
    }
    .serve(rmcp::transport::stdio())
    .await?
    .waiting()
    .await?;
    Ok(())
}

#[cfg(test)]
#[path = "gateway_tests.rs"]
mod tests;
