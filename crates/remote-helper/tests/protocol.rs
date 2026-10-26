use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

use serde_json::{json, Value};

struct Session {
    child: Child,
    output: BufReader<std::process::ChildStdout>,
}

impl Session {
    fn start(home: &std::path::Path, writable: bool) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_skill-studio-remote"));
        cmd.arg("serve").arg("--sandbox-home").arg(home);
        if writable {
            cmd.arg("--allow-writes");
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self { child, output }
    }

    fn raw(&mut self, line: &str) -> Value {
        writeln!(self.child.stdin.as_mut().unwrap(), "{line}").unwrap();
        let mut response = String::new();
        assert!(self.output.read_line(&mut response).unwrap() > 0);
        serde_json::from_str(&response).unwrap()
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.raw(&json!({"id":"test", "version":1, "method":method,"params":params}).to_string())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.child.stdin.take();
        let _ = self.child.wait();
    }
}

fn fixture() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    for dir in [".claude/skills/probe", ".codex/skills/probe"] {
        let path = home.path().join(dir);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("SKILL.md"),
            "---\nname: probe\ndescription: fixture\n---\nTest\n",
        )
        .unwrap();
    }
    std::fs::write(
        home.path().join(".claude/settings.json"),
        "{\"unrelated\":true}",
    )
    .unwrap();
    std::fs::write(
        home.path().join(".codex/config.toml"),
        "# preserved\nmodel = \"fixture\"\n",
    )
    .unwrap();
    home
}

fn write_fixture(home: &std::path::Path, path: &str, contents: &str) {
    let path = home.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn snapshot(root: &std::path::Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(snapshot(&path));
        } else if path.extension().is_some_and(|ext| ext == "lock") {
            // Windows denies reads of files held by fs2 locks. These lock
            // sentinels are not configuration state; compare the journals.
            continue;
        } else {
            files.insert(path.clone(), std::fs::read(&path).unwrap());
        }
    }
    files
}

#[test]
fn mcp_inventory_is_metadata_only_and_never_recovers_or_executes_in_either_session_mode() {
    use skill_studio_core::models::{config::AppConfig, project::ProjectBinding};
    for writable in [false, true] {
        let home = fixture();
        let root = home.path();
        let project = root.join("project");
        let mut config = AppConfig::default();
        config.projects.push(ProjectBinding::new(
            "project".into(),
            "Project".into(),
            project.clone(),
        ));
        config
            .settings
            .agent_dir_overrides
            .insert("opencode".into(), root.join("custom-open"));
        write_fixture(
            root,
            ".skill-studio/config.json",
            &serde_json::to_string(&config).unwrap(),
        );
        // A bootstrapped service would attempt recovery and fail on these journals.
        write_fixture(
            root,
            ".skill-studio/registration.json",
            "unfinished-skill-transaction",
        );
        write_fixture(
            root,
            ".skill-studio/mcp/management-transaction.json",
            "unfinished-mcp-transaction",
        );
        write_fixture(root, ".claude.json", &json!({
            "mcpServers": {"shared": {"url":"https://service.invalid/mcp", "headers":{"Authorization":"Bearer private-header"}}},
            "projects": {project.to_string_lossy().as_ref(): {"mcpServers":{"claude-project":{"command":"claude-project-tool"}}}}
        }).to_string());
        write_fixture(root, ".codex/config.toml", "[mcp_servers.shared]\nurl='https://service.invalid/mcp'\nhttp_headers={Authorization='Bearer private-header'}\n[mcp_servers.disabled]\ncommand='disabled-tool'\nenabled=false\n[mcp_servers.node_repl]\ncommand='/Applications/Codex.app/Contents/Resources/cua_node/bin/node_repl'\n");
        write_fixture(root, "custom-open/opencode.jsonc", &json!({"mcp":{"servers":{"open":{"command":["sh", "-c", format!("touch {}/must-not-run", root.display())],"environment":{"TOKEN":"private-env"}}}}}).to_string());
        write_fixture(
            root,
            ".pi/agent/mcp.json",
            "{\"mcpServers\":{\"pi\":{\"command\":\"pi-tool\"}}}",
        );
        write_fixture(
            root,
            ".grok/config.toml",
            "[mcp_servers.grok]\ncommand='grok-tool'\n",
        );
        write_fixture(
            root,
            "project/.pi/mcp.json",
            "{\"mcpServers\":{\"pi-project\":{\"command\":\"project-pi-tool\"}}}",
        );
        // An invalid file must not hide the other valid sources or echo its contents.
        write_fixture(
            root,
            "project/.codex/config.toml",
            "private-broken-token = [",
        );
        let mut session = Session::start(root, writable);
        let hello = session.call("hello", Value::Null);
        assert!(hello["result"]["methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m == "scan_mcp"));
        let before = snapshot(root);
        let response = session.call("scan_mcp", Value::Null);
        assert!(response.get("error").is_none(), "{response}");
        let servers = response["result"]["servers"].as_array().unwrap();
        assert_eq!(servers.len(), 7, "{response}");
        let shared = servers.iter().find(|s| s["name"] == "shared").unwrap();
        assert_eq!(shared["agents"], json!(["claude", "codex"]));
        for name in [
            "open",
            "pi",
            "grok",
            "disabled",
            "claude-project",
            "pi-project",
        ] {
            assert!(servers.iter().any(|s| s["name"] == name), "missing {name}");
        }
        assert_eq!(response["result"]["warnings"].as_array().unwrap().len(), 1);
        for entry in servers {
            assert_eq!(
                entry
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<std::collections::BTreeSet<_>>(),
                ["agents", "id", "name"].into_iter().collect()
            );
        }
        for private in [
            "private-header",
            "private-env",
            "private-broken-token",
            "must-not-run",
            "https://service.invalid",
        ] {
            assert!(!response.to_string().contains(private), "leaked {private}");
        }
        // 只读会话不管理 MCP；可写会话的管理见 remote_mcp_is_managed_like_the_desktop_for_direct_connections
        if !writable {
            for method in [
                "list",
                "start",
                "saveEntry",
                "removeEntry",
                "setGroup",
                "saveProject",
                "activateProject",
                "removeProject",
                "testDirect",
                "login",
            ] {
                assert!(
                    session.call("mcp_request", json!({"method":method}))["error"]["message"]
                        .as_str()
                        .unwrap()
                        .contains("只读")
                );
            }
        }
        assert_eq!(snapshot(root), before);
        assert!(!root.join("must-not-run").exists());
        // Visibility takes effect on the next read, retaining shared entries.
        config.settings.disabled_agents = vec!["opencode".into(), "claude-code".into()];
        config.settings.show_codex_builtin_mcp = true;
        write_fixture(
            root,
            ".skill-studio/config.json",
            &serde_json::to_string(&config).unwrap(),
        );
        let response = session.call("scan_mcp", Value::Null);
        let servers = response["result"]["servers"].as_array().unwrap();
        assert!(!servers
            .iter()
            .any(|s| s["name"] == "open" || s["name"] == "claude-project"));
        assert_eq!(
            servers.iter().find(|s| s["name"] == "shared").unwrap()["agents"],
            json!(["codex"])
        );
        assert!(servers.iter().any(|s| s["name"] == "node_repl"));
    }
}

#[test]
fn mcp_inventory_empty_home_stays_empty_and_distinct_connections_do_not_merge_by_name() {
    let home = tempfile::tempdir().unwrap();
    let mut session = Session::start(home.path(), false);
    assert_eq!(
        session.call("scan_mcp", Value::Null)["result"],
        json!({"servers":[],"warnings":[]})
    );
    assert!(!home.path().join(".skill-studio").exists());
    write_fixture(
        home.path(),
        ".claude.json",
        "{\"mcpServers\":{\"shared\":{\"url\":\"https://one.invalid/mcp\"}}}",
    );
    write_fixture(
        home.path(),
        ".codex/config.toml",
        "[mcp_servers.shared]\nurl='https://two.invalid/mcp'\n",
    );
    let before = snapshot(home.path());
    let response = session.call("scan_mcp", Value::Null);
    let servers = response["result"]["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 2);
    assert_ne!(servers[0]["id"], servers[1]["id"]);
    assert_eq!(snapshot(home.path()), before);
}

#[test]
fn readonly_session_rejects_mutation_and_keeps_protocol_usable() {
    let home = fixture();
    let mut session = Session::start(home.path(), false);
    assert!(session.raw("not json")["error"].is_object());
    assert!(
        session.raw(r#"{"id":"bad-version","version":99,"method":"hello"}"#)["error"].is_object()
    );
    assert_eq!(
        session.call("hello", Value::Null)["result"]["writable"],
        false
    );
    let skills = session.call("scan_skills", Value::Null);
    assert_eq!(skills["result"].as_array().unwrap().len(), 2);
    assert!(session.call("set_skill_enabled", json!({}))["error"].is_object());
    assert!(!home.path().join(".skill-studio").exists());
    assert_eq!(
        std::fs::read_to_string(home.path().join(".claude/settings.json")).unwrap(),
        "{\"unrelated\":true}"
    );
}

#[test]
fn toggles_both_agents_and_persists_across_connections() {
    let home = fixture();
    let mut session = Session::start(home.path(), true);
    let scan = session.call("scan_skills", Value::Null);
    let skills = scan["result"].as_array().unwrap();
    for agent in ["claude-code", "codex"] {
        let skill = skills
            .iter()
            .find(|s| s["agents"][agent]["status"] == "source")
            .unwrap();
        let params = json!({"skillId":skill["id"],"agentId":agent,"enabled":false});
        assert!(session
            .call("set_skill_enabled", params.clone())
            .get("error")
            .is_none());
        assert!(session
            .call("set_skill_enabled", params)
            .get("error")
            .is_none());
    }
    drop(session);
    let mut session = Session::start(home.path(), true);
    let scan = session.call("scan_skills", Value::Null);
    for agent in ["claude-code", "codex"] {
        let skill = scan["result"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["agents"][agent]["status"] == "source")
            .unwrap();
        assert_eq!(skill["agents"][agent]["disabled"], true);
        assert!(session
            .call(
                "set_skill_enabled",
                json!({"skillId":skill["id"],"agentId":agent,"enabled":true})
            )
            .get("error")
            .is_none());
    }
    let scan = session.call("scan_skills", Value::Null);
    for skill in scan["result"].as_array().unwrap() {
        for state in skill["agents"].as_object().unwrap().values() {
            if state["status"] == "source" {
                assert_eq!(state["disabled"], false);
            }
        }
    }
    let claude: Value = serde_json::from_str(
        &std::fs::read_to_string(home.path().join(".claude/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(claude["unrelated"], true);
    assert!(
        std::fs::read_to_string(home.path().join(".codex/config.toml"))
            .unwrap()
            .starts_with("# preserved\nmodel = \"fixture\"")
    );
}

#[test]
fn desktop_project_group_and_hub_workflow_uses_the_remote_home() {
    let home = fixture();
    let project_root = home.path().join("project");
    std::fs::create_dir(&project_root).unwrap();
    let mut session = Session::start(home.path(), true);
    let scan = session.call("scan_skills", Value::Null);
    let skill = scan["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["agents"]["claude-code"]["status"] == "source")
        .unwrap();
    let adopted = session.call("adopt_to_hub", json!({"skillId":skill["id"]}));
    assert!(adopted.get("error").is_none(), "{adopted}");
    let id = &adopted["result"]["id"];
    let group = session.call(
        "save_group",
        json!({"groupId":null,"name":"Remote group","skillIds":[id]}),
    );
    assert!(group.get("error").is_none(), "{group}");
    let group_id = group["result"]["id"].clone();
    let activate = session.call(
        "set_agent_group",
        json!({"agentId":"claude-code","groupId":group_id,"enabled":true}),
    );
    assert!(activate.get("error").is_none(), "{activate}");
    let deactivate = session.call(
        "set_agent_group",
        json!({"agentId":"claude-code","groupId":group_id,"enabled":false}),
    );
    assert!(deactivate.get("error").is_none(), "{deactivate}");
    let project = session.call(
        "create_project",
        json!({"name":"Remote project","root":project_root}),
    );
    assert!(project.get("error").is_none(), "{project}");
    let project_id = &project["result"]["id"];
    let apply = session.call("apply_project", json!({"projectId":project_id,"selection":{"agentIds":["claude-code"],"skillIds":[id],"groupIds":[],"linkMode":"copy"}}));
    assert!(apply.get("error").is_none(), "{apply}");
    assert!(project_root.join(".claude/skills/probe/SKILL.md").is_file());
    assert_eq!(
        session.call("list_projects", Value::Null)["result"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(!session.call("list_backups", Value::Null)["result"]
        .as_array()
        .unwrap()
        .is_empty());
    let directory = session.call("list_directory", json!({"path":project_root}));
    assert!(directory.get("error").is_none());
    let document = session.call(
        "read_skill_document",
        json!({"path":adopted["result"]["sourcePath"]}),
    );
    // 预览返回结构化文档：frontmatter 单拎出来，正文不再带 YAML 头
    assert_eq!(document["result"]["frontmatter"]["name"], "probe");
    assert_eq!(document["result"]["body"], "Test\n");
    assert!(document["result"]["raw"]
        .as_str()
        .unwrap()
        .contains("name: probe"));
    drop(session);
    let mut session = Session::start(home.path(), true);
    assert_eq!(
        session.call("list_projects", Value::Null)["result"][0]["id"],
        *project_id
    );
}

#[test]
fn uploaded_variants_keep_their_identity_and_deploy_the_dedicated_payload() {
    let home = fixture();
    let mut session = Session::start(home.path(), true);
    assert_eq!(
        session.call("hello", Value::Null)["result"]["skillVariants"],
        1
    );
    let payload = home.path().join("variant-fixture");
    std::fs::create_dir_all(&payload).unwrap();
    let bytes =
        b"---\nname: variant-probe\ndescription: Codex variant\n---\nCodex instructions\n".to_vec();
    std::fs::write(payload.join("SKILL.md"), &bytes).unwrap();
    let hash = skill_studio_core::services::scanner::dir_content_hash(&payload).unwrap();
    session.call("upload_begin", Value::Null);
    for path in ["SKILL.md", ".skill-studio-variants/codex/SKILL.md"] {
        let reply = session.call("upload_file", json!({"path":path,"data":bytes}));
        assert!(reply.get("error").is_none(), "{reply}");
    }
    let reply = session.call("upload_finish", json!({
        "source":"owner/repo", "skillId":"variant-probe", "repositoryPath":".codex/skills/variant-probe",
        "variants":[{"key":"codex","repositoryPath":".codex/skills/variant-probe","contentHash":hash}]
    }));
    assert!(reply.get("error").is_none(), "{reply}");
    let id = reply["result"]["id"].clone();
    let report = session.call(
        "register_skills",
        json!({"skillIds":[id],"agentIds":["codex"],"mode":"copy","force":false}),
    );
    assert!(report.get("error").is_none(), "{report}");
    assert_eq!(report["result"]["failed"].as_array().unwrap().len(), 0);
    assert_eq!(
        std::fs::read(home.path().join(".codex/skills/variant-probe/SKILL.md")).unwrap(),
        bytes
    );
    assert!(!home
        .path()
        .join(".codex/skills/variant-probe/.skill-studio-variants")
        .exists());
    let scanned = session.call("scan_skills", Value::Null);
    let matching: Vec<_> = scanned["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["id"] == id)
        .collect();
    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0]["installation"]["variants"][0]["key"], "codex");
}

#[test]
fn uploaded_skill_is_installed_and_paths_cannot_escape_staging() {
    let home = fixture();
    let mut session = Session::start(home.path(), true);
    assert!(session
        .call("upload_begin", Value::Null)
        .get("error")
        .is_none());
    assert!(session
        .call("upload_file", json!({"path":"../escape","data":[1,2]}))
        .get("error")
        .is_some());
    let bytes = b"---\nname: upload-check\ndescription: test\n---\nTest\n".to_vec();
    assert!(session
        .call("upload_file", json!({"path":"SKILL.md","data":bytes}))
        .get("error")
        .is_none());
    let result = session.call(
        "upload_finish",
        json!({
            "source": format!("local:{}", home.path().join("fixture-source").display()),
            "skillId":"upload-check"
        }),
    );
    assert!(result.get("error").is_none(), "{result}");
    assert!(home
        .path()
        .join(".skill-studio/skills/upload-check/SKILL.md")
        .is_file());
}

/// 真正的两个进程：辅助程序和本进程里的「桌面应用」同时改同一份配置
#[test]
fn helper_and_desktop_writing_the_same_config_keep_each_others_changes() {
    use skill_studio_core::services::store::Store;
    use skill_studio_service::{groups, state::AppState};
    let home = fixture();
    // 桌面这边的 Agent 目录也要落在测试目录里
    std::env::set_var(skill_studio_core::fs::paths::TEST_HOME_ENV, home.path());
    let dir = home.path().join(".skill-studio");
    let desktop = AppState::bootstrap(Store::new(dir.clone())).unwrap();
    let mut helper = Session::start(home.path(), true);
    let saved = helper.call(
        "save_group",
        json!({"groupId": null, "name": "服务器上建的", "skillIds": []}),
    );
    assert!(saved.get("error").is_none(), "{saved}");
    groups::save_group(&desktop, None, "桌面建的".into(), vec![], None).unwrap();

    let mut mine: Vec<_> = groups::list_groups(&desktop)
        .unwrap()
        .into_iter()
        .map(|g| g.name)
        .collect();
    mine.sort();
    assert_eq!(mine, ["服务器上建的", "桌面建的"]);
    let listed = helper.call("list_groups", json!({}));
    let mut theirs: Vec<_> = listed["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["name"].as_str().unwrap().to_string())
        .collect();
    theirs.sort();
    assert_eq!(theirs, mine);

    // 桌面正拿着配置锁时，辅助程序的写入要等它写完
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join("config.json.lock"))
        .unwrap();
    fs2::FileExt::lock_exclusive(&lock).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let waiting = std::thread::spawn(move || {
        let result = helper.call(
            "save_group",
            json!({"groupId": null, "name": "等锁后建的", "skillIds": []}),
        );
        tx.send(()).unwrap();
        result
    });
    assert!(rx
        .recv_timeout(std::time::Duration::from_millis(500))
        .is_err());
    drop(lock);
    rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap();
    let result = waiting.join().unwrap();
    assert!(result.get("error").is_none(), "{result}");
    assert_eq!(groups::list_groups(&desktop).unwrap().len(), 3);
    std::env::remove_var(skill_studio_core::fs::paths::TEST_HOME_ENV);
}

/// 服务器上装的就是命令行：同一个程序不带 serve 时按命令行工作，与桌面应用连接时
/// 用的是同一个主目录和配置
#[test]
fn the_server_build_is_the_command_line_too() {
    let home = fixture();
    let version = Command::new(env!("CARGO_BIN_EXE_skill-studio-remote"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("skill-studio {}", env!("CARGO_PKG_VERSION"))
    );
    {
        let mut session = Session::start(home.path(), true);
        let result = session.call(
            "save_group",
            json!({"groupId": null, "name": "服务器分组", "skillIds": [], "expected": null}),
        );
        assert!(result.get("error").is_none(), "{result}");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_skill-studio-remote"))
        .args(["--json", "groups", "list"])
        .env("SKILL_STUDIO_TEST_HOME", home.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let groups: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(groups[0]["name"], "服务器分组");
    // 不认识的协议参数：报错退出，不会当成命令行去解析
    let bad = Command::new(env!("CARGO_BIN_EXE_skill-studio-remote"))
        .args(["serve", "--nosuch"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("未知参数"));
}

/// 界面发出的 MCP 请求：与桌面应用调用 `mcp_request` 时的形状一致
fn mcp(session: &mut Session, method: &str, params: Value) -> Value {
    session.call("mcp_request", json!({"method": method, "params": params}))
}

fn ok(response: Value) -> Value {
    assert!(response.get("error").is_none(), "{response}");
    response["result"].clone()
}

fn error(response: &Value) -> String {
    response["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("应当失败：{response}"))
        .to_string()
}

fn direct(id: &str, definition: Value) -> Value {
    json!({
        "id": id, "name": id, "mode": "direct", "definition": definition,
        "oauth": false, "clientId": null, "scopes": [], "bindings": []
    })
}

/// 只存进 Hub，不写任何 Agent：与「添加 MCP」一致
fn save_to_hub(session: &mut Session, entry: Value) -> Value {
    mcp(
        session,
        "saveEntry",
        json!({
            "entry": entry, "expectedEntry": null, "bindingIds": [], "sources": [],
            "agents": [], "scope": "user", "projectId": ""
        }),
    )
}

fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn remote_mcp_is_managed_like_the_desktop_for_direct_connections() {
    let home = fixture();
    let root = home.path();
    write_fixture(
        root,
        ".codex/config.toml",
        "# preserved\nmodel = \"fixture\"\n[mcp_servers.shared]\nurl = \"https://service.invalid/mcp\"\nhttp_headers = { Authorization = \"Bearer private-header\" }\n",
    );
    let project_root = root.join("project");
    std::fs::create_dir(&project_root).unwrap();
    let mut session = Session::start(root, true);

    // 一览与本机一致：原有配置连同请求头回到桌面，供查看和编辑；服务器上没有网关
    let status = ok(mcp(&mut session, "list", Value::Null));
    assert_eq!(status["running"], false);
    assert_eq!(status["entries"], json!([]));
    let shared = status["discovered"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "shared")
        .cloned()
        .unwrap();
    let source = shared["sources"][0].clone();
    assert_eq!(source["agent"], "codex");
    assert_eq!(
        source["definition"]["http_headers"]["Authorization"],
        "Bearer private-header"
    );

    // 托管原有配置：接管它的接入，配置文件里的其他内容不动
    ok(mcp(
        &mut session,
        "saveEntry",
        json!({
            "entry": direct("remote-shared", json!({
                "type": "http", "url": "https://service.invalid/mcp",
                "headers": {"Authorization": "Bearer private-header"}
            })),
            "expectedEntry": null, "bindingIds": [],
            "sources": [{"id": source["id"], "definition": source["definition"]}],
            "agents": [], "scope": "user", "projectId": ""
        }),
    ));
    let status = ok(mcp(&mut session, "list", Value::Null));
    let adopted = status["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "remote-shared")
        .cloned()
        .unwrap();
    assert_eq!(adopted["bindings"][0]["agent"], "codex");
    // Windows 的路径用反斜杠：只看文件名与目录，不假设分隔符
    assert!(adopted["bindings"][0]["path"]
        .as_str()
        .unwrap()
        .replace('\\', "/")
        .ends_with(".codex/config.toml"));

    // 新的直连 MCP 先只进 Hub，再通过分组启用到 Agent
    ok(save_to_hub(
        &mut session,
        direct(
            "remote-tool",
            json!({
                "type": "stdio", "command": "/nonexistent/remote-tool",
                "args": ["--serve"], "env": {"TOKEN": "private-env"}
            }),
        ),
    ));
    assert!(!root.join(".claude.json").exists());
    ok(mcp(
        &mut session,
        "saveGroup",
        json!({
            "group": {"id": "remote-group", "name": "服务器分组", "entryIds": ["remote-tool"], "sortOrder": 0},
            "imports": [], "expected": null
        }),
    ));
    let untouched = ok(mcp(
        &mut session,
        "setGroup",
        json!({"agent": "claude", "groupId": "remote-group", "enabled": true}),
    ));
    assert_eq!(untouched, json!([]));
    let claude = read_json(&root.join(".claude.json"));
    assert_eq!(
        claude["mcpServers"]["remote-tool"]["command"],
        "/nonexistent/remote-tool"
    );
    assert_eq!(
        claude["mcpServers"]["remote-tool"]["env"]["TOKEN"],
        "private-env"
    );
    let status = ok(mcp(&mut session, "list", Value::Null));
    assert_eq!(
        status["activeGroups"]["claude"][0]["groupId"],
        "remote-group"
    );
    ok(mcp(
        &mut session,
        "setGroup",
        json!({"agent": "claude", "groupId": "remote-group", "enabled": false}),
    ));
    assert!(read_json(&root.join(".claude.json"))["mcpServers"]
        .get("remote-tool")
        .is_none());

    // 项目：成员只保存一次，按 Agent 写进服务器上的项目目录
    let project = ok(session.call(
        "create_project",
        json!({"name": "Remote project", "root": project_root}),
    ));
    let project_id = project["id"].clone();
    ok(mcp(
        &mut session,
        "saveProject",
        json!({
            "projectId": project_id, "entryIds": ["remote-tool"], "groupIds": [],
            "claudeLocal": false, "imports": [], "expected": null
        }),
    ));
    ok(mcp(
        &mut session,
        "activateProject",
        json!({"projectId": project_id, "agent": "codex", "enabled": true}),
    ));
    let project_config = project_root.join(".codex/config.toml");
    assert!(std::fs::read_to_string(&project_config)
        .unwrap()
        .contains("/nonexistent/remote-tool"));
    ok(mcp(
        &mut session,
        "activateProject",
        json!({"projectId": project_id, "agent": "codex", "enabled": false}),
    ));
    assert!(!std::fs::read_to_string(&project_config)
        .unwrap_or_default()
        .contains("/nonexistent/remote-tool"));
    let backups = ok(mcp(&mut session, "listBackups", Value::Null));
    assert!(!backups["backups"].as_array().unwrap().is_empty());

    // 网关和浏览器授权只在本机：写入前就拒绝，说明原因，Hub 里不多出条目
    let mut gateway = direct(
        "remote-gateway",
        json!({"type": "http", "url": "https://gateway.invalid/mcp"}),
    );
    gateway["mode"] = json!("gateway");
    let refused = save_to_hub(&mut session, gateway);
    assert!(
        error(&refused).contains("服务器上暂不支持 Studio 网关"),
        "{refused}"
    );
    for method in ["start", "stop", "test", "login", "loginStatus", "apply"] {
        let refused = mcp(&mut session, method, json!({"id": "remote-tool"}));
        assert!(
            error(&refused).contains("服务器上暂不支持 Studio 网关"),
            "{method}: {refused}"
        );
    }
    assert!(error(&mcp(
        &mut session,
        "searchRegistry",
        json!({"query": "github"})
    ))
    .contains("桌面应用"));

    // 连接测试在服务器上执行：启动失败时如实报错，协议继续可用
    let started = std::time::Instant::now();
    let failed = mcp(&mut session, "testDirect", json!({"id": "remote-tool"}));
    assert!(failed.get("error").is_some(), "{failed}");
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
    assert_eq!(ok(session.call("hello", Value::Null))["writable"], true);

    // 重新连接后仍是同一份托管状态
    drop(session);
    let mut session = Session::start(root, true);
    let status = ok(mcp(&mut session, "list", Value::Null));
    let ids: Vec<_> = status["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids.len(), 2, "{status}");
    assert!(ids.contains(&"remote-shared".to_string()) && ids.contains(&"remote-tool".to_string()));

    // 移出 Hub 并还原：原有配置原样留在服务器上
    let removal = ok(mcp(
        &mut session,
        "removeEntry",
        json!({"id": "remote-shared", "restore": true}),
    ));
    assert_eq!(removal["foreign"], json!([]));
    let codex = std::fs::read_to_string(root.join(".codex/config.toml")).unwrap();
    assert!(
        codex.starts_with("# preserved\nmodel = \"fixture\"\n"),
        "{codex}"
    );
    assert!(codex.contains("[mcp_servers.shared]") && codex.contains("Bearer private-header"));

    // 服务器的设置里关掉 MCP 管理后，所有 MCP 请求都停下
    ok(session.call("update_settings", json!({"patch": {"manageMcp": false}})));
    assert!(error(&mcp(&mut session, "list", Value::Null)).contains("MCP 管理已关闭"));
}

/// 连接测试与服务器上的 Agent 一样，在服务器上启动 stdio 服务并列出工具
#[cfg(unix)]
#[test]
fn remote_connection_test_starts_the_stdio_service_on_the_server() {
    let home = tempfile::tempdir().unwrap();
    let marker = home.path().join("started-on-server");
    let script = format!(
        r#"import json,pathlib,sys
pathlib.Path({marker:?}).write_text('ok')
for line in sys.stdin:
    r=json.loads(line)
    if 'id' not in r: continue
    method=r['method']
    if method=='initialize': result={{'protocolVersion':r['params']['protocolVersion'],'capabilities':{{'tools':{{}}}},'serverInfo':{{'name':'fixture','version':'1'}}}}
    elif method=='tools/list': result={{'tools':[{{'name':'stdio-echo','inputSchema':{{'type':'object'}}}}]}}
    else: result={{}}
    print(json.dumps({{'jsonrpc':'2.0','id':r['id'],'result':result}}),flush=True)
"#,
        marker = marker.to_string_lossy()
    );
    let mut session = Session::start(home.path(), true);
    ok(save_to_hub(
        &mut session,
        direct(
            "stdio-echo",
            json!({"type": "stdio", "command": "python3", "args": ["-u", "-c", script]}),
        ),
    ));
    let tools = ok(mcp(&mut session, "testDirect", json!({"id": "stdio-echo"})));
    assert_eq!(tools[0]["name"], "stdio-echo", "{tools}");
    assert!(marker.is_file());
    // 服务的输出没有混进协议：下一次请求照常应答
    assert_eq!(ok(session.call("hello", Value::Null))["writable"], true);
}
