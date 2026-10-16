//! Opt-in integration test using the published official Memory and Filesystem servers.
//! MCP_REAL_SERVER_DIR must contain node_modules for both packages; MCP_TEST_NODE is optional.
//! Build skill-studio first so the test can exercise its real --mcp-client executable.
use anyhow::{Context, Result};
use rmcp::{
    model::{CallToolRequestParams, ClientConfig},
    service::RunningService,
    transport::TokioChildProcess,
    RoleClient, ServiceExt,
};
use serde_json::{json, Value};
use skill_studio_mcp::{
    discovery::{self, ScanFile},
    gateway,
    management::{self, groups, Entry, Target},
    native,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
type Client = RunningService<RoleClient, ClientConfig>;
async fn connect(value: &Value) -> Result<Client> {
    let mut command = tokio::process::Command::new(value["command"].as_str().context("command")?);
    command.kill_on_drop(true);
    if let Some(args) = value["args"].as_array() {
        for arg in args {
            command.arg(arg.as_str().context("arg")?);
        }
    }
    if let Some(env) = value["env"].as_object() {
        for (k, v) in env {
            command.env(k, v.as_str().context("env")?);
        }
    }
    Ok(tokio::time::timeout(
        Duration::from_secs(30),
        ClientConfig::default().serve(TokioChildProcess::new(command)?),
    )
    .await??)
}
async fn call(client: &Client, name: &str, arguments: Value) -> Result<String> {
    let request: CallToolRequestParams =
        serde_json::from_value(json!({"name": name, "arguments": arguments}))?;
    let response =
        tokio::time::timeout(Duration::from_secs(30), client.call_tool(request)).await??;
    anyhow::ensure!(!response.is_error.unwrap_or(false), "{name}: {response:?}");
    Ok(serde_json::to_string(&response)?)
}
fn read(path: &Path, agent: &str, name: &str) -> Result<Option<Value>> {
    native::entry(&std::fs::read_to_string(path)?, agent, None, name)
}
fn entry(id: &str, definition: Value) -> Entry {
    Entry {
        id: id.into(),
        name: id.into(),
        mode: "direct".into(),
        definition,
        oauth: false,
        client_id: None,
        scopes: vec![],
        bindings: vec![],
    }
}
fn targets(claude: &Path, codex: &Path, name: &str) -> Vec<Target> {
    [(claude, "claude"), (codex, "codex")]
        .into_iter()
        .map(|(path, agent)| Target {
            agent: agent.into(),
            path: path.into(),
            project: None,
            key: name.into(),
            expected: None,
        })
        .collect()
}
fn group(id: &str, ids: &[&str]) -> groups::Group {
    groups::Group {
        id: id.into(),
        name: id.into(),
        entry_ids: ids.iter().map(|s| s.to_string()).collect(),
        references: vec![],
        sort_order: 0,
    }
}
fn bindings(entry: &Entry) -> Vec<Target> {
    entry
        .bindings
        .iter()
        .map(|b| Target {
            agent: b.agent.clone(),
            path: b.path.clone(),
            project: b.project.clone(),
            key: b.key.clone(),
            expected: Some(b.installed.clone()),
        })
        .collect()
}
#[tokio::test]
#[ignore = "requires npm-installed real MCP servers and the compiled desktop executable"]
async fn official_servers_full_lifecycle() -> Result<()> {
    let packages = PathBuf::from(std::env::var("MCP_REAL_SERVER_DIR")?).canonicalize()?;
    let executable = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/skill-studio")
        .canonicalize()?;
    let root = tempfile::tempdir()?;
    let dir = root.path().join("studio");
    let claude = root.path().join("claude.json");
    let codex = root.path().join("config.toml");
    let files = root.path().join("files");
    std::fs::create_dir(&files)?;
    std::fs::write(
        &claude,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://example.invalid/provider"},"projects":{"/work":{"mcpServers":{"project":{"command":"project"}}}},"mcpServers":{}}"#,
    )?;
    std::fs::write(
        &codex,
        "# preserve this comment\nmodel_provider='test-provider'\n",
    )?;
    let node = std::env::var("MCP_TEST_NODE").unwrap_or_else(|_| "node".into());
    let memory_definition = json!({"type":"stdio","command":node,"args":[packages.join("node_modules/@modelcontextprotocol/server-memory/dist/index.js")],"env":{"MEMORY_FILE_PATH":root.path().join("memory.jsonl")}});
    let filesystem_definition = json!({"type":"stdio","command":node,"args":[packages.join("node_modules/@modelcontextprotocol/server-filesystem/dist/index.js"),files]});
    assert_eq!(
        native::parse_definition(
            &json!({"mcpServers":{"real-memory":memory_definition}}).to_string()
        )?
        .as_array()
        .unwrap()
        .len(),
        1
    );
    let memory = entry("real-memory", memory_definition);
    management::save(
        &dir,
        memory.clone(),
        targets(&claude, &codex, &memory.name),
        &executable,
        None,
    )?;
    let scan = discovery::scan(
        &[
            ScanFile {
                agent: "claude".into(),
                path: claude.clone(),
                scope: "global".into(),
            },
            ScanFile {
                agent: "codex".into(),
                path: codex.clone(),
                scope: "global".into(),
            },
        ],
        &[],
        &dir,
    );
    assert_eq!(
        scan.discovered
            .iter()
            .find(|d| d.name == memory.name)
            .unwrap()
            .sources
            .len(),
        2
    );
    let native_client = connect(&read(&claude, "claude", &memory.name)?.unwrap()).await?;
    assert!(native_client
        .list_all_tools()
        .await?
        .iter()
        .any(|t| t.name == "create_entities"));
    call(&native_client,"create_entities",json!({"entities":[{"name":"studio-real-test","entityType":"test","observations":["written through native config"]}]})).await?;
    assert!(call(
        &native_client,
        "search_nodes",
        json!({"query":"studio-real-test"})
    )
    .await?
    .contains("written through native config"));
    native_client.cancel().await?;
    let codex_client = connect(&read(&codex, "codex", &memory.name)?.unwrap()).await?;
    assert!(call(&codex_client, "read_graph", json!({}))
        .await?
        .contains("studio-real-test"));
    call(
        &codex_client,
        "delete_entities",
        json!({"entityNames":["studio-real-test"]}),
    )
    .await?;
    codex_client.cancel().await?;
    println!("PASS real Memory: install to both agents, scan, list tools, create/search/read/delete persistent entity");
    management::remove(&dir, &memory.id, false)?;
    assert!(read(&claude, "claude", &memory.name)?.is_none());
    assert!(read(&codex, "codex", &memory.name)?.is_none());
    management::save(&dir, memory.clone(), vec![], &executable, None)?;
    let filesystem = entry("real-filesystem", filesystem_definition);
    management::save(&dir, filesystem.clone(), vec![], &executable, None)?;
    groups::save_group(&dir, group("memory", &["real-memory"]), vec![])?;
    groups::save_group(&dir, group("files", &["real-filesystem"]), vec![])?;
    groups::save_group(&dir, group("codex", &["real-memory"]), vec![])?;
    groups::activate(
        &dir,
        "claude",
        &["memory".to_string()],
        &claude,
        &executable,
    )?;
    groups::activate(&dir, "codex", &["codex".to_string()], &codex, &executable)?;
    let codex_before = std::fs::read(&codex)?;
    groups::activate(&dir, "claude", &["files".to_string()], &claude, &executable)?;
    assert!(read(&claude, "claude", &memory.name)?.is_none());
    assert_eq!(std::fs::read(&codex)?, codex_before);
    let fs_client = connect(&read(&claude, "claude", &filesystem.name)?.unwrap()).await?;
    assert!(fs_client
        .list_all_tools()
        .await?
        .iter()
        .any(|t| t.name == "write_file"));
    let test_file = files.join("real-mcp.txt");
    call(
        &fs_client,
        "write_file",
        json!({"path":test_file,"content":"Skill Studio real MCP round trip"}),
    )
    .await?;
    assert!(
        call(&fs_client, "read_text_file", json!({"path":test_file}))
            .await?
            .contains("Skill Studio real MCP round trip")
    );
    fs_client.cancel().await?;
    assert_eq!(
        std::fs::read_to_string(&test_file)?,
        "Skill Studio real MCP round trip"
    );
    let before_edit = std::fs::read(&claude)?;
    groups::save_group(
        &dir,
        group("files", &["real-filesystem", "real-memory"]),
        vec![],
    )?;
    assert_eq!(std::fs::read(&claude)?, before_edit);
    groups::activate(&dir, "claude", &["files".to_string()], &claude, &executable)?;
    assert!(read(&claude, "claude", &memory.name)?.is_some());
    groups::reorder(&dir, &["files".into(), "memory".into(), "codex".into()])?;
    assert!(groups::remove_group(&dir, "files").is_err());
    assert!(management::remove(&dir, &memory.id, true).is_err());
    println!("PASS groups: save without applying, exclusive switch, other-agent isolation, real Filesystem write/read, edit/apply, reorder, deletion guards");
    let old = management::read(&dir)?
        .entries
        .into_iter()
        .find(|e| e.id == memory.id)
        .unwrap();
    let mut proxied = old.clone();
    proxied.mode = "gateway".into();
    management::save(&dir, proxied, bindings(&old), &executable, Some(&old))?;
    groups::activate(&dir, "claude", &["files".to_string()], &claude, &executable)?;
    groups::activate(&dir, "codex", &["codex".to_string()], &codex, &executable)?;
    let mut daemon = tokio::process::Command::new(&executable)
        .arg("--mcp-daemon")
        .arg(&dir)
        .kill_on_drop(true)
        .spawn()?;
    for i in 0..100 {
        if gateway::request(&dir, "list", Value::Null).await.is_ok() {
            break;
        }
        anyhow::ensure!(i < 99, "gateway startup timeout");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let proxy_a = connect(&read(&claude, "claude", &memory.name)?.unwrap()).await?;
    let proxy_b = connect(&read(&codex, "codex", &memory.name)?.unwrap()).await?;
    call(&proxy_a,"create_entities",json!({"entities":[{"name":"gateway-shared","entityType":"test","observations":["two real clients"]}]})).await?;
    assert!(call(&proxy_b, "read_graph", json!({}))
        .await?
        .contains("gateway-shared"));
    call(
        &proxy_b,
        "delete_entities",
        json!({"entityNames":["gateway-shared"]}),
    )
    .await?;
    proxy_a.cancel().await?;
    proxy_b.cancel().await?;
    gateway::request(&dir, "stop", Value::Null).await?;
    tokio::time::timeout(Duration::from_secs(10), daemon.wait()).await??;
    let disconnected = connect(&read(&claude, "claude", &memory.name)?.unwrap()).await;
    assert!(disconnected.is_err());
    println!("PASS real gateway: compiled daemon and two compiled --mcp-client processes share Memory, stop closes access");
    // A manually edited native entry must not be overwritten by group switch or stop.
    let original = std::fs::read_to_string(&codex)?;
    let expected = read(&codex, "codex", &memory.name)?;
    let altered = native::patch(
        &original,
        "codex",
        None,
        &memory.name,
        &expected,
        &Some(json!({"command":"external-change"})),
    )?;
    std::fs::write(&codex, &altered)?;
    let catalog_before = std::fs::read(dir.join("catalog.json"))?;
    assert!(groups::activate(&dir, "codex", &[], &codex, &executable).is_err());
    assert_eq!(std::fs::read_to_string(&codex)?, altered);
    assert_eq!(std::fs::read(dir.join("catalog.json"))?, catalog_before);
    std::fs::write(&codex, original)?;
    groups::activate(&dir, "claude", &[], &claude, &executable)?;
    groups::activate(&dir, "codex", &[], &codex, &executable)?;
    for id in ["files", "memory", "codex"] {
        groups::remove_group(&dir, id)?;
    }
    for id in [&memory.id, &filesystem.id] {
        management::remove(&dir, id, true)?;
    }
    let final_catalog = management::read(&dir)?;
    assert!(
        final_catalog.entries.is_empty()
            && final_catalog.groups.is_empty()
            && final_catalog.active_groups.is_empty()
    );
    let final_claude: Value = serde_json::from_str(&std::fs::read_to_string(&claude)?)?;
    assert_eq!(final_claude["mcpServers"], json!({}));
    assert_eq!(
        final_claude["env"]["ANTHROPIC_BASE_URL"],
        "https://example.invalid/provider"
    );
    assert!(final_claude["projects"]["/work"]["mcpServers"]["project"].is_object());
    assert!(std::fs::read_to_string(&codex)?
        .starts_with("# preserve this comment\nmodel_provider='test-provider'\n"));
    assert!(!dir.join("management-transaction.json").exists());
    println!("PASS cleanup: external edit protected, both agents stopped, all groups/services removed, providers and projects preserved");
    Ok(())
}
