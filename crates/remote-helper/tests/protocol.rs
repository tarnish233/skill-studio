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
        for method in [
            "start",
            "saveEntry",
            "removeEntry",
            "activateGroup",
            "saveProject",
            "activateProject",
            "removeProject",
            "login",
        ] {
            assert!(
                session.call("mcp_request", json!({"method":method}))["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("只读")
            );
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
