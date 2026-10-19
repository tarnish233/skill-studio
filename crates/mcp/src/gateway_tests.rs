use super::*;
use axum::middleware::{self, Next};
use std::sync::atomic::{AtomicUsize, Ordering};
#[derive(Clone)]
struct Fixture;
impl ServerHandler for Fixture {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(serde_json::from_value(json!({"tools":[{"name":"echo","description":"Fixture","inputSchema":{"type":"object"}}]})).unwrap())
    }
    async fn call_tool(
        &self,
        p: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        Ok(CallToolResult::success(vec![ContentBlock::text(p.name.to_string())]).into())
    }
}
async fn wait(dir: &Path) {
    for _ in 0..100 {
        if request(dir, "list", Value::Null).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("gateway did not start");
}
fn definition(url: String, oauth: bool) -> Server {
    Server {
        id: "demo".into(),
        name: "Demo".into(),
        connection: Connection::Http {
            url,
            headers: Default::default(),
        },
        enabled: true,
        oauth,
        client_id: None,
        scopes: vec![],
    }
}
async fn client(dir: &Path, token: Option<&str>) -> Result<Upstream> {
    let c = config::read(dir)?;
    let mut headers = std::collections::BTreeMap::new();
    if let Some(token) = token {
        headers.insert("Authorization".into(), format!("Bearer {token}"));
    }
    connect(&Runtime {
        server: Server {
            connection: Connection::Http {
                url: format!("http://127.0.0.1:{}/mcp/demo", c.port),
                headers,
            },
            ..definition(String::new(), false)
        },
        auth: None,
        auth_required: Arc::new(Mutex::new(HashSet::new())),
    })
    .await
}
#[tokio::test]
async fn direct_probe_distinguishes_login_from_rejected_authorization_header() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let mock = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/mcp",
                axum::routing::any(|| async {
                    (StatusCode::UNAUTHORIZED, [("www-authenticate", "Bearer")])
                }),
            ),
        )
        .await
        .unwrap();
    });
    let timeout = Duration::from_secs(2);
    assert_eq!(
        probe_direct(definition(url.clone(), false), timeout)
            .await
            .unwrap(),
        json!({"authRequired":true})
    );
    let mut with_token = definition(url, false);
    if let Connection::Http { headers, .. } = &mut with_token.connection {
        headers.insert("authorization".into(), "Bearer invalid".into());
    }
    assert!(probe_direct(with_token, timeout)
        .await
        .unwrap_err()
        .to_string()
        .contains("Token"));
    mock.abort();
}
#[tokio::test]
async fn http_gateway_isolates_sessions_and_requires_service_credentials() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let service = StreamableHttpService::new(
        || Ok(Fixture),
        Arc::new(LocalSessionManager::default()),
        Default::default(),
    );
    let mock = tokio::spawn(async move {
        axum::serve(listener, Router::new().nest_service("/mcp", service))
            .await
            .unwrap();
    });
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_owned();
    let daemon = tokio::spawn(serve(dir.clone()));
    wait(&dir).await;
    request(
        &dir,
        "save",
        serde_json::to_value(definition(format!("http://127.0.0.1:{port}/mcp"), false)).unwrap(),
    )
    .await
    .unwrap();
    assert!(client(&dir, None).await.is_err());
    let token = config::read(&dir).unwrap().tokens["demo"].clone();
    let (a, b) = tokio::join!(client(&dir, Some(&token)), client(&dir, Some(&token)));
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.list_all_tools().await.unwrap()[0].name, "echo");
    let response = a
        .call_tool(CallToolRequestParams::new("echo"))
        .await
        .unwrap();
    assert!(!response.is_error.unwrap_or(false));
    a.cancel().await.unwrap();
    assert_eq!(b.list_all_tools().await.unwrap().len(), 1);
    b.cancel().await.unwrap();
    let c = config::read(&dir).unwrap();
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{}/admin", c.port))
        .bearer_auth(&token)
        .json(&json!({"method":"list"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    request(&dir, "stop", Value::Null).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    mock.abort();
}
#[tokio::test]
async fn oauth_login_is_persistent_and_shared_by_two_clients() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let refreshes = Arc::new(AtomicUsize::new(0));
    let count = refreshes.clone();
    let metadata = json!({"issuer":base,"authorization_endpoint":format!("{base}/authorize"),"token_endpoint":format!("{base}/token"),"registration_endpoint":format!("{base}/register"),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none"]});
    let protected = json!({"resource":format!("{base}/mcp"),"authorization_servers":[base]});
    let service = StreamableHttpService::new(
        || Ok(Fixture),
        Arc::new(LocalSessionManager::default()),
        Default::default(),
    );
    let app=Router::new().nest_service("/mcp",service).layer(middleware::from_fn(|req:Request,next:Next|async move{
   if req.headers().get("authorization").and_then(|v|v.to_str().ok())!=Some("Bearer refreshed"){return (StatusCode::UNAUTHORIZED, [("www-authenticate", "Bearer")]).into_response();}next.run(req).await
  })).route("/.well-known/oauth-authorization-server",get(move||async move{Json(metadata)})).route("/.well-known/oauth-protected-resource",get(move||async move{Json(protected)}))
  .route("/register",post(|Json(p):Json<Value>|async move{Json(json!({"client_id":"studio-fixture","redirect_uris":p["redirect_uris"],"token_endpoint_auth_method":"none"}))}))
  .route("/token",post(move|body:String|{let count=count.clone();async move{
   if body.contains("grant_type=refresh_token"){count.fetch_add(1,Ordering::SeqCst);Json(json!({"access_token":"refreshed","token_type":"Bearer","expires_in":3600,"refresh_token":"refresh-next"}))}
   else {Json(json!({"access_token":"initial","token_type":"Bearer","expires_in":0,"refresh_token":"refresh-first"}))}
  }}));
    let mock = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_owned();
    let daemon = tokio::spawn(serve(dir.clone()));
    wait(&dir).await;
    request(
        &dir,
        "save",
        serde_json::to_value(definition(format!("{base}/mcp"), false)).unwrap(),
    )
    .await
    .unwrap();
    let probe = request(&dir, "test", json!({"id":"demo"})).await.unwrap();
    assert_eq!(probe, json!({"authRequired":true}));
    let status = request(&dir, "list", Value::Null).await.unwrap();
    assert_eq!(status["servers"][0]["authRequired"], true);
    let mut with_token = definition(format!("{base}/mcp"), false);
    if let Connection::Http { headers, .. } = &mut with_token.connection {
        headers.insert("Authorization".into(), "Bearer invalid".into());
    }
    request(&dir, "save", serde_json::to_value(with_token).unwrap())
        .await
        .unwrap();
    let error = request(&dir, "test", json!({"id":"demo"}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Token"));
    request(
        &dir,
        "save",
        serde_json::to_value(definition(format!("{base}/mcp"), false)).unwrap(),
    )
    .await
    .unwrap();
    let login = request(&dir, "login", json!({"id":"demo"})).await.unwrap();
    let flow_params = json!({"id":"demo","flowId":login["flowId"]});
    assert_eq!(
        request(&dir, "loginStatus", flow_params.clone())
            .await
            .unwrap()["status"],
        "pending"
    );
    assert_eq!(
        request(&dir, "loginStatus", json!({"id":"demo","flowId":"wrong"}))
            .await
            .unwrap()["status"],
        "expired"
    );
    let url = reqwest::Url::parse(login["url"].as_str().unwrap()).unwrap();
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["code_challenge_method"], "S256");
    let callback = &query["redirect_uri"];
    let bad = reqwest::Client::new()
        .get(callback)
        .query(&[("code", "code"), ("state", "wrong")])
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    let ok = reqwest::Client::new()
        .get(callback)
        .query(&[
            ("code", "code"),
            ("state", query["state"].as_str()),
            ("iss", base.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    assert_eq!(
        request(&dir, "loginStatus", flow_params.clone())
            .await
            .unwrap()["status"],
        "complete"
    );
    assert_eq!(
        request(&dir, "loginStatus", flow_params).await.unwrap()["status"],
        "expired"
    );
    assert!(dir.join("oauth-demo.json").exists());
    let token = config::read(&dir).unwrap().tokens["demo"].clone();
    let (a, b) = tokio::join!(client(&dir, Some(&token)), client(&dir, Some(&token)));
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.list_all_tools().await.unwrap().len(), 1);
    assert_eq!(b.list_all_tools().await.unwrap().len(), 1);
    assert_eq!(refreshes.load(Ordering::SeqCst), 1);
    a.cancel().await.unwrap();
    b.cancel().await.unwrap();
    request(&dir, "stop", Value::Null).await.unwrap();
    daemon.await.unwrap().unwrap();
    let daemon = tokio::spawn(serve(dir.clone()));
    wait(&dir).await;
    let a = client(&dir, Some(&token)).await.unwrap();
    assert_eq!(a.list_all_tools().await.unwrap().len(), 1);
    a.cancel().await.unwrap();
    request(&dir, "logout", json!({"id":"demo"})).await.unwrap();
    assert!(!dir.join("oauth-demo.json").exists());
    assert!(client(&dir, Some(&token)).await.is_err());
    request(&dir, "stop", Value::Null).await.unwrap();
    daemon.await.unwrap().unwrap();
    mock.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn stdio_upstream_is_started_only_when_requested_and_disable_closes_access() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_owned();
    let daemon = tokio::spawn(serve(dir.clone()));
    wait(&dir).await;
    let script = r#"import json,sys
for line in sys.stdin:
    r=json.loads(line)
    if 'id' not in r: continue
    method=r['method']
    if method=='initialize': result={'protocolVersion':r['params']['protocolVersion'],'capabilities':{'tools':{}},'serverInfo':{'name':'fixture','version':'1'}}
    elif method=='tools/list': result={'tools':[{'name':'stdio-echo','inputSchema':{'type':'object'}}]}
    elif method=='tools/call': result={'content':[{'type':'text','text':'stdio-ok'}]}
    else: result={}
    print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':result}),flush=True)
"#;
    let mut definition = definition(String::new(), false);
    definition.connection = Connection::Stdio {
        command: "python3".into(),
        args: vec!["-u".into(), "-c".into(), script.into()],
        env: Default::default(),
        cwd: None,
    };
    request(&dir, "save", serde_json::to_value(&definition).unwrap())
        .await
        .unwrap();
    let token = config::read(&dir).unwrap().tokens["demo"].clone();
    let a = client(&dir, Some(&token)).await.unwrap();
    assert_eq!(a.list_all_tools().await.unwrap()[0].name, "stdio-echo");
    let result = a
        .call_tool(CallToolRequestParams::new("stdio-echo"))
        .await
        .unwrap();
    assert!(serde_json::to_string(&result).unwrap().contains("stdio-ok"));
    a.cancel().await.unwrap();
    definition.enabled = false;
    request(&dir, "save", serde_json::to_value(&definition).unwrap())
        .await
        .unwrap();
    assert!(client(&dir, Some(&token)).await.is_err());
    request(&dir, "stop", Value::Null).await.unwrap();
    daemon.await.unwrap().unwrap();
}

#[tokio::test]
async fn gateway_reads_committed_catalog_and_stops_serving_direct_entries() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let service = StreamableHttpService::new(
        || Ok(Fixture),
        Arc::new(LocalSessionManager::default()),
        Default::default(),
    );
    let mock = tokio::spawn(async move {
        axum::serve(listener, Router::new().nest_service("/mcp", service))
            .await
            .unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_owned();
    let entry = crate::management::Entry {
        id: "demo".into(),
        name: "Demo".into(),
        mode: "gateway".into(),
        definition: json!({"type":"http","url":format!("http://127.0.0.1:{port}/mcp")}),
        oauth: false,
        client_id: None,
        scopes: vec![],
        bindings: vec![],
    };
    crate::management::save(&dir, entry.clone(), vec![], Path::new("/app"), None).unwrap();
    assert!(!dir.join("config.json").exists());
    let daemon = tokio::spawn(serve(dir.clone()));
    wait(&dir).await;
    let token = config::read(&dir).unwrap().tokens["demo"].clone();
    let connected = client(&dir, Some(&token)).await.unwrap();
    assert_eq!(connected.list_all_tools().await.unwrap().len(), 1);
    connected.cancel().await.unwrap();
    let mut renamed = entry.clone();
    renamed.name = "Renamed".into();
    crate::management::save(
        &dir,
        renamed.clone(),
        vec![],
        Path::new("/app"),
        Some(&entry),
    )
    .unwrap();
    let credential_path = dir.join("oauth-demo.json");
    let credential = r#"{"client_id":"sentinel","token_response":null}"#;
    std::fs::write(&credential_path, credential).unwrap();
    let connected = client(&dir, Some(&token)).await.unwrap();
    connected.cancel().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(&credential_path).unwrap(),
        credential
    );
    crate::management::set_enabled(&dir, false).unwrap();
    assert!(request(&dir, "list", Value::Null).await.unwrap()["servers"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(config::read(&dir).unwrap().tokens["demo"], token);
    assert_eq!(
        std::fs::read_to_string(&credential_path).unwrap(),
        credential
    );
    crate::management::set_enabled(&dir, true).unwrap();
    assert_eq!(
        request(&dir, "list", Value::Null).await.unwrap()["servers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(config::read(&dir).unwrap().tokens["demo"], token);
    std::fs::remove_file(credential_path).unwrap();
    let mut direct = renamed.clone();
    direct.mode = "direct".into();
    crate::management::save(&dir, direct, vec![], Path::new("/app"), Some(&renamed)).unwrap();
    assert!(request(&dir, "list", Value::Null).await.unwrap()["servers"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(client(&dir, Some(&token)).await.is_err());
    request(&dir, "stop", Value::Null).await.unwrap();
    daemon.await.unwrap().unwrap();
    mock.abort();
}

#[test]
fn a_stopped_gateway_still_forgets_removed_servers_and_their_credentials() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().to_path_buf();
    for id in ["gone", "kept"] {
        let entry = crate::management::Entry {
            id: id.into(),
            name: id.into(),
            mode: "gateway".into(),
            definition: json!({"type":"stdio","command":id}),
            oauth: false,
            client_id: None,
            scopes: vec![],
            bindings: vec![],
        };
        crate::management::save(&dir, entry, vec![], Path::new("/app"), None).unwrap();
        std::fs::write(dir.join(format!("oauth-{id}.json")), "{}").unwrap();
    }
    // 网关从没启动过：没有网关配置就什么都不写
    assert_eq!(forget_removed(&dir).unwrap(), 0);
    assert!(!dir.join("config.json").exists());
    let servers = crate::management::projection(&dir)
        .unwrap()
        .unwrap()
        .servers;
    let tokens = [
        ("gone".to_string(), "t1".to_string()),
        ("kept".to_string(), "t2".to_string()),
    ];
    config::save(
        &dir,
        &config::Config {
            servers,
            tokens: tokens.into(),
            ..Default::default()
        },
    )
    .unwrap();
    crate::management::remove(&dir, "gone", false).unwrap();
    // 和网关下次启动时一样：不再托管的服务连同接入令牌、OAuth 凭据一并清掉
    assert_eq!(forget_removed(&dir).unwrap(), 1);
    let config = config::read(&dir).unwrap();
    assert_eq!(
        config
            .servers
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["kept"]
    );
    assert_eq!(config.tokens.keys().collect::<Vec<_>>(), ["kept"]);
    assert!(!dir.join("oauth-gone.json").exists());
    assert!(dir.join("oauth-kept.json").exists());
    // 暂停管理时保留凭据，重新开启后复用
    crate::management::set_enabled(&dir, false).unwrap();
    assert_eq!(forget_removed(&dir).unwrap(), 0);
    assert!(dir.join("oauth-kept.json").exists());
}
