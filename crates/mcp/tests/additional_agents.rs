use serde_json::{json, Value};
use skill_studio_mcp::{
    agents,
    discovery::{self, ScanFile},
    management::{self, groups, Entry, Target},
    native,
};
use std::{collections::HashMap, fs, path::Path};

const IDS: [&str; 3] = ["opencode", "pi", "grok"];
fn entry(mode: &str) -> Entry {
    Entry {
        id: "sample".into(),
        name: "sample".into(),
        mode: mode.into(),
        definition: json!({"type":"stdio","command":"managed","args":["argument with spaces"],"env":{"SAFE":"value"}}),
        oauth: false,
        client_id: None,
        scopes: vec![],
        bindings: vec![],
    }
}
fn original(agent: &str) -> &'static str {
    match agent {
        "opencode" => "{\n// model comment\n\"model\":\"keep\",\"mcp\":{\"timeout\":{\"startup\":45000},\"servers\":{\"sample\":{\"type\":\"local\",\"command\":[\"original\",\"arg\"],\"timeout\":{\"execution\":60000}},\"manual\":{\"type\":\"local\",\"command\":[\"keep\"]},}},\n}",
        "pi" => "{\n// adapter comment\n\"settings\":{\"toolPrefix\":\"server\"},\"mcpServers\":{\"sample\":{\"command\":\"original\",\"args\":[\"arg\"],\"directTools\":true},\"manual\":{\"command\":\"keep\"}},\n}",
        _ => "# model comment\nmodel='keep'\n[mcp_servers.sample]\ncommand='original'\nargs=['arg']\ntool_timeout_sec=120\n[mcp_servers.manual]\ncommand='keep'\n",
    }
}
fn read(path: &Path, agent: &str, key: &str) -> Option<Value> {
    native::entry(&fs::read_to_string(path).unwrap(), agent, None, key).unwrap()
}
fn target(path: &Path, agent: &str, expected: Option<Value>) -> Target {
    Target {
        agent: agent.into(),
        path: path.into(),
        project: None,
        key: "sample".into(),
        expected,
    }
}

#[test]
fn native_codecs_preserve_arguments_headers_and_disabled_state() {
    for agent in IDS {
        for definition in [
            json!({"type":"stdio","command":"/path with spaces/server","args":["a b","--flag"],"env":{"K":"V"},"cwd":"/work","enabled":false}),
            json!({"type":"http","url":"https://example.com/mcp","headers":{"X-Token":"dummy"},"enabled":false}),
        ] {
            let converted = native::for_agent(&definition, agent).unwrap();
            assert!(!native::enabled(&converted, agent));
            assert_eq!(native::canonical(&converted, agent), definition);
            let text =
                native::patch("", agent, None, "sample", &None, &Some(converted.clone())).unwrap();
            assert_eq!(
                native::entry(&text, agent, None, "sample").unwrap(),
                Some(converted.clone())
            );
            let doc = native::document(&text, agent).unwrap();
            if agent == "opencode" {
                assert_eq!(doc["mcp"]["servers"]["sample"], converted);
            }
        }
    }
    assert!(native::patch(
        r#"{"mcp":{"old":{"type":"local","command":["old"]}}}"#,
        "opencode",
        None,
        "new",
        &None,
        &Some(json!({"type":"local","command":["new"]}))
    )
    .unwrap_err()
    .to_string()
    .contains("v2"));
    assert!(native::entry(r#"{"mcp":{"servers":42}}"#, "opencode", None, "x").is_err());
    for agent in IDS {
        assert!(native::entry("{}", agent, Some("/project"), "x").is_err());
    }
}

#[test]
fn import_accepts_opencode_v2_jsonc_and_grok_toml() {
    let parsed = native::parse_definition(r#"{// comment
        "mcp":{"servers":{"demo":{"type":"local","command":["node","test.js"],"environment":{"K":"V"},},},},}"#).unwrap();
    assert_eq!(
        parsed[0]["definition"],
        json!({"type":"stdio","command":"node","args":["test.js"],"env":{"K":"V"}})
    );
    let parsed = native::parse_definition(
        "[mcp_servers.remote]\nurl='https://example.com/mcp'\nheaders={X='value'}",
    )
    .unwrap();
    assert_eq!(parsed[0]["definition"]["headers"], json!({"X":"value"}));
    assert!(
        native::parse_definition(r#"{"mcp":{"old":{"type":"local","command":["old"]}}}"#).is_err()
    );
}

#[test]
fn adoption_suspend_resume_restore_and_backup_keep_unrelated_config() {
    for agent in IDS {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("studio");
        let path = temp.path().join("config");
        fs::write(&path, original(agent)).unwrap();
        let before = read(&path, agent, "sample");
        let manual = read(&path, agent, "manual");
        management::save(
            &dir,
            entry("gateway"),
            vec![target(&path, agent, before.clone())],
            Path::new("/studio app"),
            None,
        )
        .unwrap();
        let installed = read(&path, agent, "sample").unwrap();
        assert_eq!(
            native::canonical(&installed, agent)["args"],
            json!(["--mcp-client", "sample", dir])
        );
        let scan = discovery::scan(
            &[ScanFile {
                agent: agent.into(),
                path: path.clone(),
                scope: "用户全局".into(),
            }],
            &management::projection(&dir).unwrap().unwrap().servers,
            &dir,
        );
        assert!(scan
            .discovered
            .iter()
            .any(|d| d.managed_id.as_deref() == Some("sample") && d.sources[0].gateway));
        management::set_enabled(&dir, false).unwrap();
        assert_eq!(read(&path, agent, "sample"), before);
        assert_eq!(read(&path, agent, "manual"), manual);
        assert!(fs::read_to_string(&path).unwrap().contains("comment"));
        management::set_enabled(&dir, true).unwrap();
        assert_eq!(read(&path, agent, "sample"), Some(installed));
        management::remove(&dir, "sample", true).unwrap();
        assert_eq!(read(&path, agent, "sample"), before);
        let backup = management::read(&dir)
            .unwrap()
            .backups
            .last()
            .unwrap()
            .id
            .clone();
        management::restore_backup(&dir, &backup).unwrap();
        assert_eq!(management::read(&dir).unwrap().entries.len(), 1);
    }
}

#[test]
fn groups_and_project_removal_restore_each_agents_original_definition() {
    for agent in IDS {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("studio");
        let root = temp.path().join("project");
        let path = agents::write_path(agent, &HashMap::new(), Some(&root)).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, original(agent)).unwrap();
        let before = read(&path, agent, "sample");
        management::save(
            &dir,
            entry("direct"),
            vec![target(&path, agent, before.clone())],
            Path::new("/studio"),
            None,
        )
        .unwrap();
        let global = temp.path().join("global");
        groups::save_group(
            &dir,
            groups::Group {
                id: "group".into(),
                name: "work".into(),
                entry_ids: vec!["sample".into()],
                references: vec![],
                sort_order: 0,
            },
            vec![],
        )
        .unwrap();
        groups::activate(&dir, agent, Some("group"), &global, Path::new("/studio")).unwrap();
        assert_eq!(
            native::canonical(&read(&global, agent, "sample").unwrap(), agent)["command"],
            "managed"
        );
        management::set_enabled(&dir, false).unwrap();
        assert_eq!(read(&path, agent, "sample"), before);
        assert!(read(&global, agent, "sample").is_none());
        management::set_enabled(&dir, true).unwrap();
        groups::activate(&dir, agent, None, &global, Path::new("/studio")).unwrap();
        assert!(read(&global, agent, "sample").is_none());
        assert_eq!(management::remove_project_bindings(&dir, &root).unwrap(), 1);
        assert_eq!(read(&path, agent, "sample"), before);
        assert!(management::read(&dir).unwrap().entries[0]
            .bindings
            .is_empty());
    }
}

#[test]
fn external_conflict_aborts_all_agents_and_preserves_jsonc_comments() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("studio");
    let mut targets = vec![];
    for agent in IDS {
        let path = temp.path().join(agent);
        fs::write(&path, original(agent)).unwrap();
        targets.push(target(&path, agent, read(&path, agent, "sample")));
    }
    management::save(&dir, entry("gateway"), targets, Path::new("/studio"), None).unwrap();
    let pi = temp.path().join("pi");
    let text = fs::read_to_string(&pi).unwrap();
    let old = read(&pi, "pi", "sample");
    fs::write(
        &pi,
        native::patch(
            &text,
            "pi",
            None,
            "sample",
            &old,
            &Some(json!({"command":"external"})),
        )
        .unwrap(),
    )
    .unwrap();
    let snapshots: Vec<_> = IDS
        .iter()
        .map(|id| fs::read(temp.path().join(id)).unwrap())
        .collect();
    assert!(management::set_enabled(&dir, false).is_err());
    for (id, bytes) in IDS.iter().zip(snapshots) {
        assert_eq!(fs::read(temp.path().join(id)).unwrap(), bytes);
    }
}

#[test]
fn activating_groups_enables_new_direct_bindings_without_changing_hub_definitions() {
    for agent in ["codex", "opencode", "pi", "grok"] {
        for definition in [
            json!({"type":"stdio","command":"managed","args":["argument with spaces"],"env":{"SAFE":"value"},"enabled":false}),
            json!({"type":"http","url":"https://example.com/mcp","headers":{"X-Test":"value"},"enabled":false}),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let dir = temp.path().join("studio");
            let path = temp.path().join("config");
            let mut saved = entry("direct");
            saved.definition = definition.clone();
            management::save(&dir, saved, vec![], Path::new("/studio"), None).unwrap();
            groups::save_group(
                &dir,
                groups::Group {
                    id: "group".into(),
                    name: "work".into(),
                    entry_ids: vec!["sample".into()],
                    references: vec![],
                    sort_order: 0,
                },
                vec![],
            )
            .unwrap();
            // Re-applying must retain enabled native state and the original rollback target.
            for _ in 0..2 {
                groups::activate(&dir, agent, Some("group"), &path, Path::new("/studio")).unwrap();
                let installed = read(&path, agent, "sample").unwrap();
                assert!(native::enabled(&installed, agent), "{agent}: {installed}");
                if agent == "pi" {
                    assert_eq!(installed["enabled"], true);
                    assert!(installed.get("disabled").is_none());
                }
                let mut expected = definition.clone();
                expected["enabled"] = json!(true);
                assert_eq!(native::canonical(&installed, agent), expected);
                let catalog = management::read(&dir).unwrap();
                assert_eq!(catalog.entries[0].definition, definition);
                assert_eq!(
                    catalog.active_groups[agent].entries[0].definition,
                    definition
                );
                assert!(groups::issues(&catalog).is_empty());
                let scan = discovery::scan(
                    &[ScanFile {
                        agent: agent.into(),
                        path: path.clone(),
                        scope: "用户全局".into(),
                    }],
                    &[],
                    &dir,
                );
                assert!(scan.discovered[0].sources[0].enabled);
            }
            management::set_enabled(&dir, false).unwrap();
            assert!(read(&path, agent, "sample").is_none());
            management::set_enabled(&dir, true).unwrap();
            assert!(native::enabled(
                &read(&path, agent, "sample").unwrap(),
                agent
            ));
            groups::activate(&dir, agent, None, &path, Path::new("/studio")).unwrap();
            assert!(read(&path, agent, "sample").is_none());
            assert!(management::read(&dir).unwrap().active_groups.is_empty());
            assert_eq!(
                management::read(&dir).unwrap().entries[0].definition,
                definition
            );
        }
    }
}

#[test]
fn disabled_entries_and_variable_references_remain_agent_owned() {
    for agent in IDS {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config");
        let def = native::for_agent(
            &json!({"type":"stdio","command":"original","enabled":false}),
            agent,
        )
        .unwrap();
        fs::write(
            &path,
            native::patch("", agent, None, "sample", &None, &Some(def)).unwrap(),
        )
        .unwrap();
        let scan = discovery::scan(
            &[ScanFile {
                agent: agent.into(),
                path: path.clone(),
                scope: "用户全局".into(),
            }],
            &[],
            temp.path(),
        );
        assert!(!scan.discovered[0].sources[0].enabled);
        let dir = temp.path().join("studio");
        management::save(&dir, entry("direct"), vec![], Path::new("/studio"), None).unwrap();
        groups::save_group(
            &dir,
            groups::Group {
                id: "group".into(),
                name: "work".into(),
                entry_ids: vec!["sample".into()],
                references: vec![],
                sort_order: 0,
            },
            vec![],
        )
        .unwrap();
        let before = fs::read(&path).unwrap();
        assert!(
            groups::activate(&dir, agent, Some("group"), &path, Path::new("/studio"))
                .unwrap_err()
                .to_string()
                .contains("手动停用")
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(management::read(&dir).unwrap().active_groups.is_empty());
    }
    let def = json!({"type":"remote","url":"https://example.com/mcp","headers":{"Authorization":"{env:TOKEN}"}});
    assert!(discovery::normalize("x", "x", &def, "opencode")
        .err()
        .unwrap()
        .to_string()
        .contains("变量引用"));
}

#[test]
fn paths_honor_agent_overrides_and_prefer_existing_opencode_jsonc() {
    let temp = tempfile::tempdir().unwrap();
    for agent in IDS {
        let root = temp.path().join(agent);
        fs::create_dir_all(&root).unwrap();
        let overrides = HashMap::from([(agent.to_owned(), root.clone())]);
        assert!(agents::write_path(agent, &overrides, None)
            .unwrap()
            .starts_with(&root));
    }
    let nested = temp.path().join(".opencode");
    fs::create_dir_all(&nested).unwrap();
    fs::write(temp.path().join("opencode.json"), "{}").unwrap();
    fs::write(nested.join("opencode.jsonc"), "{}").unwrap();
    assert_eq!(
        agents::write_path("opencode", &HashMap::new(), Some(temp.path())).unwrap(),
        nested.join("opencode.jsonc")
    );
    assert!(agents::write_path("unknown", &HashMap::new(), Some(temp.path())).is_err());
}

#[test]
fn grok_global_disable_and_invalid_tool_namespace_are_not_silently_ignored() {
    let text = "disabled_mcp_servers=['sample']\n[mcp_servers.sample]\ncommand='demo'\n";
    assert!(native::check_activation(text, "grok", "sample").is_err());
    for key in ["1bad", "bad name", "bad__name", "bad_"] {
        assert!(native::check_activation("", "grok", key).is_err());
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    fs::write(&path, text).unwrap();
    let scan = discovery::scan(
        &[ScanFile {
            agent: "grok".into(),
            path,
            scope: "用户全局".into(),
        }],
        &[],
        temp.path(),
    );
    assert!(!scan.discovered[0].sources[0].enabled);
    assert_eq!(
        scan.discovered[0].sources[0].definition,
        json!({"command":"demo"})
    );
}

/// Explicit opt-in: export real converter output for CLI/GUI checks in a new temporary directory.
#[test]
#[ignore]
fn export_cli_smoke_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var("SKILL_STUDIO_MCP_SMOKE_DIR").expect("set a new temporary fixture directory"),
    );
    fs::create_dir(&root).unwrap();
    let server = root.join("server.py");
    fs::write(&server,r#"import json,sys
for line in sys.stdin:
    request=json.loads(line)
    method=request.get('method')
    if 'id' not in request: continue
    if method=='initialize': result={'protocolVersion':request['params']['protocolVersion'],'capabilities':{'tools':{}},'serverInfo':{'name':'studio-smoke','version':'1.0'}}
    elif method=='tools/list': result={'tools':[{'name':'echo','description':'Local smoke test','inputSchema':{'type':'object','properties':{}}}]}
    elif method=='tools/call': result={'content':[{'type':'text','text':'studio-ok'}]}
    elif method=='resources/list': result={'resources':[]}
    elif method=='resources/templates/list': result={'resourceTemplates':[]}
    elif method=='prompts/list': result={'prompts':[]}
    else: result={}
    print(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':result}),flush=True)
"#).unwrap();
    let executable =
        std::env::var("SKILL_STUDIO_TEST_APP").expect("set compiled Studio executable");
    for mode in ["direct", "gateway"] {
        let dir = root.join(mode).join("studio");
        let mut e = entry(mode);
        e.definition = json!({"type":"stdio","command":"/usr/bin/python3","args":[server]});
        let targets = IDS
            .iter()
            .map(|id| {
                let file = match *id {
                    "opencode" => "opencode/opencode.jsonc",
                    "pi" => "pi/mcp.json",
                    _ => "grok/config.toml",
                };
                target(&root.join(mode).join(file), id, None)
            })
            .collect();
        management::save(&dir, e, targets, Path::new(&executable), None).unwrap();
    }
    println!("fixture: {}", root.display());
}

#[test]
fn ambiguous_jsonc_and_invalid_flags_are_not_silently_rewritten() {
    let text = r#"{"mcp":{"servers":{"sample":{"type":"local","command":["first"]},"sample":{"type":"local","command":["last"]}}}}"#;
    let before = native::entry(text, "opencode", None, "sample").unwrap();
    assert!(
        native::patch(text, "opencode", None, "sample", &before, &None)
            .unwrap_err()
            .to_string()
            .contains("重复属性")
    );
    let invalid = json!({"type":"local","command":["demo"],"disabled":"false"});
    assert_eq!(native::canonical(&invalid, "opencode")["disabled"], "false");
    assert!(native::for_agent(
        &json!({"type":"stdio","command":"demo","enabled":"false"}),
        "pi"
    )
    .is_err());
}

#[test]
fn grok_user_disable_list_also_applies_to_project_sources() {
    let temp = tempfile::tempdir().unwrap();
    let global = temp.path().join("global.toml");
    let project = temp.path().join("project.toml");
    fs::write(&global, "disabled_mcp_servers=['sample']\n").unwrap();
    fs::write(&project, "[mcp_servers.sample]\ncommand='demo'\n").unwrap();
    let scan = discovery::scan(
        &[
            ScanFile {
                agent: "grok".into(),
                path: project,
                scope: "项目 · test".into(),
            },
            ScanFile {
                agent: "grok".into(),
                path: global,
                scope: "用户全局".into(),
            },
        ],
        &[],
        temp.path(),
    );
    assert!(!scan.discovered[0].sources[0].enabled);
    assert!(!scan.discovered[0].server.as_ref().unwrap().enabled);
}

#[test]
fn pi_native_state_and_options_survive_round_trip() {
    for enabled in [true, false] {
        let definition = json!({
            "type": "http", "url": "https://example.com/mcp",
            "enabled": enabled, "disabled": enabled,
            "oauth": {"clientId": "pi-client"},
            "exposure": "deferred", "toolExposure": {"search": "direct"},
            "timeout": 90
        });
        let native = native::for_agent(&definition, "pi").unwrap();
        assert_eq!(native["enabled"], enabled);
        assert_eq!(native::enabled(&native, "pi"), enabled);
        assert_eq!(native::canonical(&native, "pi"), definition);
    }
    // Adapter-only flags must not override the native client's actual state.
    let legacy = json!({"command": "demo", "disabled": true});
    assert!(native::enabled(&legacy, "pi"));
    assert!(native::canonical(&legacy, "pi").get("enabled").is_none());
    let native = native::for_agent(
        &json!({"type": "stdio", "command": "demo", "enabled": false}),
        "pi",
    )
    .unwrap();
    assert_eq!(native["enabled"], false);
    assert!(native.get("disabled").is_none());
}

#[test]
fn pi_rejects_invalid_and_colliding_names_without_partial_writes() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("studio");
    let path = temp.path().join("mcp.json");
    let original = r#"{"mcpServers":{"a-b":{"command":"keep"}}}"#;
    fs::write(&path, original).unwrap();
    for name in ["", "bad.name", "with space", "中文", "a_b"] {
        let mut binding = target(&path, "pi", None);
        binding.key = name.into();
        assert!(management::save(
            &dir,
            entry("direct"),
            vec![binding],
            Path::new("/studio"),
            None
        )
        .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert!(management::read(&dir).unwrap().entries.is_empty());
    }
    assert!(native::check_activation(original, "pi", "a-b").is_ok());
    assert!(native::check_activation(original, "pi", "valid_2").is_ok());

    // Check staged edits too: two targets in one save must not bypass validation.
    let fresh = temp.path().join("new.json");
    let mut first = target(&fresh, "pi", None);
    first.key = "x-y".into();
    let mut second = first.clone();
    second.key = "x_y".into();
    assert!(management::save(
        &dir,
        entry("direct"),
        vec![first, second],
        Path::new("/studio"),
        None
    )
    .is_err());
    assert!(!fresh.exists());
    assert!(management::read(&dir).unwrap().entries.is_empty());
}
