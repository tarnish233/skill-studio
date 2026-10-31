use super::*;
use crate::native;

#[test]
fn disabling_management_restores_both_agents_and_resumes_managed_group() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let claude = root.path().join("claude.json");
    let codex = root.path().join("config.toml");
    std::fs::write(
        &claude,
        r#"{"theme":"dark","mcpServers":{"sample":{"command":"claude-original"},"manual":{"command":"keep"}}}"#,
    )
    .unwrap();
    std::fs::write(
        &codex,
        "model='keep'\n[mcp_servers.sample]\ncommand='codex-original'\n[mcp_servers.manual]\ncommand='keep'\n",
    )
    .unwrap();
    let original_claude = native::entry(
        &read_text(&claude).unwrap().unwrap(),
        "claude",
        None,
        "sample",
    )
    .unwrap();
    let original_codex = native::entry(
        &read_text(&codex).unwrap().unwrap(),
        "codex",
        None,
        "sample",
    )
    .unwrap();
    save(
        &dir,
        entry(json!({"type":"stdio","command":"managed"}), "gateway"),
        vec![
            target(claude.clone(), "claude", "sample", original_claude.clone()),
            target(codex.clone(), "codex", "sample", original_codex.clone()),
        ],
        Path::new("/app"),
        None,
    )
    .unwrap();
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
        None,
    )
    .unwrap();
    groups::activate(
        &dir,
        "codex",
        &["group".to_string()],
        &codex,
        Path::new("/app"),
    )
    .unwrap();
    let managed_claude = read_text(&claude).unwrap().unwrap();
    let managed_codex = read_text(&codex).unwrap().unwrap();

    set_enabled(&dir, false).unwrap();
    let catalog = read(&dir).unwrap();
    assert!(catalog.suspended);
    assert_eq!(catalog.entries.len(), 1);
    assert_eq!(catalog.active_groups.len(), 1);
    assert!(projection(&dir).unwrap().unwrap().servers.is_empty());
    let restored_claude = read_text(&claude).unwrap().unwrap();
    let restored_codex = read_text(&codex).unwrap().unwrap();
    assert_eq!(
        native::entry(&restored_claude, "claude", None, "sample").unwrap(),
        original_claude
    );
    assert_eq!(
        native::entry(&restored_codex, "codex", None, "sample").unwrap(),
        original_codex
    );
    assert_eq!(
        native::entry(&restored_claude, "claude", None, "manual")
            .unwrap()
            .unwrap()["command"],
        "keep"
    );
    assert_eq!(
        native::entry(&restored_codex, "codex", None, "manual")
            .unwrap()
            .unwrap()["command"],
        "keep"
    );
    assert!(save(
        &dir,
        entry(json!({"type":"stdio","command":"new"}), "direct"),
        vec![],
        Path::new("/app"),
        None
    )
    .is_err());

    set_enabled(&dir, true).unwrap();
    assert!(!read(&dir).unwrap().suspended);
    assert_eq!(projection(&dir).unwrap().unwrap().servers.len(), 1);
    assert_eq!(read_text(&claude).unwrap().unwrap(), managed_claude);
    assert_eq!(read_text(&codex).unwrap().unwrap(), managed_codex);
}

#[test]
fn disabling_or_resuming_never_overwrites_external_mcp_edits() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let codex = root.path().join("config.toml");
    std::fs::write(&codex, "[mcp_servers.sample]\ncommand='original'\n").unwrap();
    let original = native::entry(
        &read_text(&codex).unwrap().unwrap(),
        "codex",
        None,
        "sample",
    )
    .unwrap();
    save(
        &dir,
        entry(json!({"type":"stdio","command":"managed"}), "direct"),
        vec![target(codex.clone(), "codex", "sample", original)],
        Path::new("/app"),
        None,
    )
    .unwrap();
    let managed = read_text(&codex).unwrap().unwrap();
    std::fs::write(&codex, "[mcp_servers.sample]\ncommand='outside'\n").unwrap();
    assert!(set_enabled(&dir, false).is_err());
    assert!(!read(&dir).unwrap().suspended);
    assert!(read_text(&codex).unwrap().unwrap().contains("outside"));
    std::fs::write(&codex, managed).unwrap();
    set_enabled(&dir, false).unwrap();
    std::fs::write(
        &codex,
        "[mcp_servers.sample]\ncommand='changed-while-off'\n",
    )
    .unwrap();
    let error = set_enabled(&dir, true).unwrap_err();
    assert!(error.to_string().contains(codex.to_str().unwrap()));
    assert!(read(&dir).unwrap().suspended);
    assert!(read_text(&codex)
        .unwrap()
        .unwrap()
        .contains("changed-while-off"));
}
#[test]
fn backup_restore_requires_unchanged_target_and_preserves_other_entries() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    let original =
        "# retained\n[mcp_servers.one]\ncommand='one'\n[mcp_servers.two]\ncommand='two'\n";
    std::fs::write(&path, original).unwrap();
    let one = native::entry(original, "codex", None, "one").unwrap();
    remove_sources(&dir, vec![target(path.clone(), "codex", "one", one)]).unwrap();
    let backup = read(&dir).unwrap().backups[0].clone();
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        std::fs::read_to_string(backup.files[0].backup_path.as_ref().unwrap()).unwrap(),
        original
    );
    std::fs::write(
        &path,
        "# edited by user\n[mcp_servers.two]\ncommand='two'\n",
    )
    .unwrap();
    assert!(restore_backup(&dir, &backup.id)
        .unwrap_err()
        .to_string()
        .contains("备份后修改"));
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("edited by user"));
    std::fs::write(&path, &after).unwrap();
    // Restore requires the exact post-write file, including TOML formatting.
    assert_eq!(Some(digest(&after)), backup.files[0].after_hash);
    restore_backup(&dir, &backup.id).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert!(!read(&dir)
        .unwrap()
        .backups
        .iter()
        .any(|b| b.id == backup.id));
}
#[test]
fn backup_restore_updates_catalog_and_rejects_later_catalog_edits() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    std::fs::write(&path, "# original\n").unwrap();
    save(
        &dir,
        entry(json!({"type":"stdio","command":"demo"}), "direct"),
        vec![target(path.clone(), "codex", "sample", None)],
        Path::new("/app"),
        None,
    )
    .unwrap();
    let backup = read(&dir).unwrap().backups[0].clone();
    let after = std::fs::read_to_string(&path).unwrap();
    let mut edited = read(&dir).unwrap().entries[0].clone();
    edited.name = "Changed".into();
    save(
        &dir,
        edited,
        current_targets(&read(&dir).unwrap().entries[0]),
        Path::new("/app"),
        Some(&read(&dir).unwrap().entries[0]),
    )
    .unwrap();
    assert!(restore_backup(&dir, &backup.id)
        .unwrap_err()
        .to_string()
        .contains("托管状态"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), after);
    // Recreate the transaction and restore without intervening edits.
    let fresh = root.path().join("fresh");
    let fresh_file = root.path().join("fresh.toml");
    std::fs::write(&fresh_file, "# original\n").unwrap();
    save(
        &fresh,
        entry(json!({"type":"stdio","command":"demo"}), "direct"),
        vec![target(fresh_file.clone(), "codex", "sample", None)],
        Path::new("/app"),
        None,
    )
    .unwrap();
    let id = read(&fresh).unwrap().backups[0].id.clone();
    restore_backup(&fresh, &id).unwrap();
    assert!(read(&fresh).unwrap().entries.is_empty());
    assert_eq!(std::fs::read_to_string(fresh_file).unwrap(), "# original\n");
}
#[test]
fn project_bulk_remove_preserves_global_and_manual_entries() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let project_file = project.join(".mcp.json");
    let global_file = root.path().join("global.json");
    std::fs::write(
        &project_file,
        r#"{"mcpServers":{"manual":{"command":"keep"}}}"#,
    )
    .unwrap();
    std::fs::write(&global_file, "{}").unwrap();
    save(
        &dir,
        entry(json!({"type":"stdio","command":"demo"}), "direct"),
        vec![
            target(project_file.clone(), "claude", "sample", None),
            target(global_file.clone(), "claude", "sample", None),
        ],
        Path::new("/app"),
        None,
    )
    .unwrap();
    assert_eq!(remove_project_bindings(&dir, &project).unwrap().removed, 1);
    let project_doc: Value =
        serde_json::from_str(&std::fs::read_to_string(project_file).unwrap()).unwrap();
    assert_eq!(project_doc["mcpServers"]["manual"]["command"], "keep");
    assert!(project_doc["mcpServers"].get("sample").is_none());
    assert!(native::entry(
        &std::fs::read_to_string(global_file).unwrap(),
        "claude",
        None,
        "sample"
    )
    .unwrap()
    .is_some());
    assert_eq!(read(&dir).unwrap().entries[0].bindings.len(), 1);
}
#[test]
fn backup_restores_multi_file_transaction_including_new_project_file() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let codex = root.path().join("config.toml");
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let claude = project.join(".mcp.json");
    std::fs::write(&codex, "# before\n").unwrap();
    save(
        &dir,
        entry(json!({"type":"stdio","command":"demo"}), "direct"),
        vec![
            target(codex.clone(), "codex", "sample", None),
            target(claude.clone(), "claude", "sample", None),
        ],
        Path::new("/app"),
        None,
    )
    .unwrap();
    let backup = read(&dir).unwrap().backups[0].clone();
    assert_eq!(backup.files.len(), 2);
    let written = std::fs::read_to_string(&codex).unwrap();
    std::fs::write(&codex, "# user edit\n").unwrap();
    assert!(restore_backup(&dir, &backup.id).is_err());
    assert!(claude.exists());
    std::fs::write(&codex, written).unwrap();
    restore_backup(&dir, &backup.id).unwrap();
    assert_eq!(std::fs::read_to_string(codex).unwrap(), "# before\n");
    assert!(!claude.exists());
    assert!(read(&dir).unwrap().entries.is_empty());
}
fn entry(definition: Value, mode: &str) -> Entry {
    Entry {
        id: "sample".into(),
        name: "Sample".into(),
        mode: mode.into(),
        definition,
        oauth: false,
        client_id: None,
        scopes: vec![],
        provenance: None,
        bindings: vec![],
    }
}
fn target(path: PathBuf, agent: &str, key: &str, expected: Option<Value>) -> Target {
    Target {
        agent: agent.into(),
        path,
        project: None,
        key: key.into(),
        expected,
    }
}
fn current_targets(entry: &Entry) -> Vec<Target> {
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
#[test]
fn direct_save_preserves_extensions_for_both_agents_without_a_gateway() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let claude = root.path().join("claude.json");
    let codex = root.path().join("config.toml");
    std::fs::write(&claude,r#"{"env":{"ANTHROPIC_BASE_URL":"http://provider"},"mcpServers":{"other":{"command":"other"}}}"#).unwrap();
    std::fs::write(
        &codex,
        "# provider\nmodel_provider='custom'\n[mcp_servers.other]\ncommand='other'\n",
    )
    .unwrap();
    let e = entry(
        json!({"type":"stdio","command":"demo","args":[" argument with spaces "],"startup_timeout_sec":120,"cwd":"relative"}),
        "direct",
    );
    save(
        &dir,
        e,
        vec![
            target(claude.clone(), "claude", "sample", None),
            target(codex.clone(), "codex", "sample", None),
        ],
        Path::new("/app"),
        None,
    )
    .unwrap();
    assert!(!dir.join("config.json").exists());
    let text = std::fs::read_to_string(&codex).unwrap();
    assert!(text.contains("# provider"));
    assert!(text.contains("model_provider='custom'"));
    for (path, agent) in [(&claude, "claude"), (&codex, "codex")] {
        let value = native::entry(
            &std::fs::read_to_string(path).unwrap(),
            agent,
            None,
            "sample",
        )
        .unwrap()
        .unwrap();
        assert_eq!(value["startup_timeout_sec"], 120);
        assert_eq!(value["cwd"], "relative");
        assert_eq!(value["args"][0], " argument with spaces ");
        assert!(native::entry(
            &std::fs::read_to_string(path).unwrap(),
            agent,
            None,
            "other"
        )
        .unwrap()
        .is_some());
    }
    assert_eq!(read(&dir).unwrap().entries[0].bindings.len(), 2);
}
#[test]
fn direct_probe_accepts_agent_extensions_and_startup_timeout() {
    let e = entry(
        json!({"type":"stdio","command":"demo","startup_timeout_sec":120,"tool_timeout_sec":45}),
        "direct",
    );
    let (server, timeout) = direct_probe_server(&e).unwrap();
    assert_eq!(timeout.as_secs(), 120);
    assert!(matches!(
        server.connection,
        config::Connection::Stdio { .. }
    ));
    assert!(gateway_server(&e).is_err());
}
#[test]
fn direct_probe_does_not_convert_sse_to_streamable_http() {
    let e = entry(
        json!({"type":"sse","url":"https://example.com/sse"}),
        "direct",
    );
    // SSE is valid for Claude, but Studio's probe has no SSE transport.
    assert!(native::for_agent(&e.definition, "claude").is_ok());
    let result = direct_probe_server(&e);
    assert!(
        result.is_err(),
        "SSE must not become a Streamable HTTP probe"
    );
    assert!(result.err().unwrap().to_string().contains("请在 Agent 中"));
}
/// 把 Claude 和 Codex 里同一个已有的 MCP 交给网关托管，返回（Studio 目录, claude 配置, codex 配置, 原配置）
fn adopt_into_gateway(root: &Path) -> (PathBuf, PathBuf, PathBuf, Value) {
    let dir = root.join("studio");
    let claude = root.join("claude.json");
    let codex = root.join("config.toml");
    let original = json!({"command":"demo","args":["--serve"]});
    std::fs::write(&claude, json!({"mcpServers":{"demo":original}}).to_string()).unwrap();
    std::fs::write(
        &codex,
        "[mcp_servers.demo]\ncommand='demo'\nargs=['--serve']\n",
    )
    .unwrap();
    save(
        &dir,
        entry(native::canonical(&original, "claude"), "gateway"),
        vec![
            target(claude.clone(), "claude", "demo", Some(original.clone())),
            target(codex.clone(), "codex", "demo", Some(original.clone())),
        ],
        Path::new("/app"),
        None,
    )
    .unwrap();
    (dir, claude, codex, original)
}
fn demo_entry(path: &Path, agent: &str) -> Option<Value> {
    native::entry(&std::fs::read_to_string(path).unwrap(), agent, None, "demo").unwrap()
}
#[test]
fn deleting_leaves_connections_another_tool_rewrote_and_reports_them() {
    let root = tempfile::tempdir().unwrap();
    let (dir, claude, codex, original) = adopt_into_gateway(root.path());
    // 另一个工具（如 cc-switch）把原来的配置写回了 Claude，覆盖了 Studio 的网关接入
    std::fs::write(&claude, json!({"mcpServers":{"demo":original}}).to_string()).unwrap();
    let removal = remove(&dir, "sample", false).unwrap();
    assert!(read(&dir).unwrap().entries.is_empty());
    // 别人写的不动、报告出来；Studio 自己的接入照常删掉
    assert_eq!(demo_entry(&claude, "claude"), Some(original.clone()));
    assert_eq!(demo_entry(&codex, "codex"), None);
    assert_eq!(removal.removed, 1);
    assert_eq!(removal.foreign.len(), 1);
    assert_eq!(removal.foreign[0].agent, "claude");
    assert_eq!(removal.foreign[0].path, claude);
    // 留下的那条成了普通的原有配置，可以按来源删除
    remove_sources(
        &dir,
        vec![target(claude.clone(), "claude", "demo", Some(original))],
    )
    .unwrap();
    assert_eq!(demo_entry(&claude, "claude"), None);
}
#[test]
fn deleting_counts_only_connections_it_actually_rewrote() {
    let root = tempfile::tempdir().unwrap();
    let (dir, claude, codex, _) = adopt_into_gateway(root.path());
    // Claude 里的接入已被删掉：没有要写的，不算进「已移除」（界面据此判断有没有写配置、留备份）
    std::fs::write(&claude, json!({"mcpServers":{}}).to_string()).unwrap();
    let removal = remove(&dir, "sample", false).unwrap();
    assert_eq!((removal.removed, removal.foreign.len()), (1, 0));
    assert_eq!(demo_entry(&codex, "codex"), None);
}
#[test]
fn restoring_puts_the_original_back_where_studios_connection_was_already_removed() {
    let root = tempfile::tempdir().unwrap();
    let (dir, claude, codex, original) = adopt_into_gateway(root.path());
    // 有人删掉了 Studio 写的接入：那里没有别人的内容，照常按「还原」放回原配置
    std::fs::write(&claude, json!({"mcpServers":{}}).to_string()).unwrap();
    let removal = remove(&dir, "sample", true).unwrap();
    assert!(removal.foreign.is_empty());
    assert_eq!(removal.removed, 2);
    assert_eq!(demo_entry(&claude, "claude"), Some(original.clone()));
    assert_eq!(demo_entry(&codex, "codex"), Some(original));
}
#[test]
fn project_removal_leaves_connections_another_tool_rewrote() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let project_file = project.join(".mcp.json");
    std::fs::write(&project_file, "{}").unwrap();
    save(
        &dir,
        entry(json!({"type":"stdio","command":"demo"}), "direct"),
        vec![target(project_file.clone(), "claude", "sample", None)],
        Path::new("/app"),
        None,
    )
    .unwrap();
    std::fs::write(
        &project_file,
        r#"{"mcpServers":{"sample":{"command":"theirs"}}}"#,
    )
    .unwrap();
    let removal = remove_project_bindings(&dir, &project).unwrap();
    assert_eq!((removal.removed, removal.foreign.len()), (0, 1));
    assert!(read(&dir).unwrap().entries[0].bindings.is_empty());
    let doc: Value =
        serde_json::from_str(&std::fs::read_to_string(&project_file).unwrap()).unwrap();
    assert_eq!(doc["mcpServers"]["sample"]["command"], "theirs");
}
#[test]
fn migration_reuses_original_keys_and_restores_original_entries() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let claude = root.path().join("claude.json");
    let codex = root.path().join("config.toml");
    let original = json!({"command":"demo","args":["--serve"]});
    std::fs::write(
        &claude,
        json!({"mcpServers":{"original-name":original}}).to_string(),
    )
    .unwrap();
    std::fs::write(
        &codex,
        "[mcp_servers.original_name]\ncommand='demo'\nargs=['--serve']\n",
    )
    .unwrap();
    let e = entry(native::canonical(&original, "claude"), "gateway");
    save(
        &dir,
        e,
        vec![
            target(
                claude.clone(),
                "claude",
                "original-name",
                Some(original.clone()),
            ),
            target(
                codex.clone(),
                "codex",
                "original_name",
                Some(original.clone()),
            ),
        ],
        Path::new("/app"),
        None,
    )
    .unwrap();
    let catalog = read(&dir).unwrap();
    let managed = &catalog.entries[0];
    for b in &managed.bindings {
        let text = std::fs::read_to_string(&b.path).unwrap();
        assert_eq!(
            native::entry(&text, &b.agent, None, &b.key)
                .unwrap()
                .unwrap()["args"][0],
            "--mcp-client"
        );
        assert!(!text.contains("studio-sample"));
    }
    assert!(projection(&dir)
        .unwrap()
        .is_some_and(|state| state.servers.len() == 1));
    let mut direct = managed.clone();
    direct.mode = "direct".into();
    save(
        &dir,
        direct,
        current_targets(managed),
        Path::new("/app"),
        Some(managed),
    )
    .unwrap();
    assert!(projection(&dir).unwrap().unwrap().servers.is_empty());
    remove(&dir, "sample", true).unwrap();
    assert_eq!(
        native::entry(
            &std::fs::read_to_string(&claude).unwrap(),
            "claude",
            None,
            "original-name"
        )
        .unwrap(),
        Some(original.clone())
    );
    assert_eq!(
        native::entry(
            &std::fs::read_to_string(&codex).unwrap(),
            "codex",
            None,
            "original_name"
        )
        .unwrap(),
        Some(original)
    );
    // 每次写入都留了备份，集中在 Studio 目录；配置文件旁边不再出现备份文件
    let backups = read(&dir).unwrap().backups;
    assert!(!backups.is_empty());
    for backup in &backups {
        assert!(backup.catalog_backup_path.starts_with(dir.join("backups")));
        for file in backup.files.iter().filter_map(|f| f.backup_path.as_ref()) {
            assert!(file.starts_with(dir.join("backups")) && file.is_file());
        }
    }
    assert!(!std::fs::read_dir(root.path())
        .unwrap()
        .flatten()
        .any(|f| f.file_name().to_string_lossy().contains("studio-backup")));
}
#[test]
fn conflicts_do_not_partially_change_other_agents_and_stale_edits_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let a = root.path().join("a.json");
    let b = root.path().join("b.toml");
    std::fs::write(&a, "{}").unwrap();
    std::fs::write(&b, "[mcp_servers.sample]\ncommand='foreign'\n").unwrap();
    let e = entry(json!({"type":"stdio","command":"demo"}), "direct");
    assert!(save(
        &dir,
        e.clone(),
        vec![
            target(a.clone(), "claude", "sample", None),
            target(b, "codex", "sample", None)
        ],
        Path::new("/app"),
        None
    )
    .is_err());
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "{}");
    assert!(!dir.join("catalog.json").exists());
    save(
        &dir,
        e.clone(),
        vec![target(a, "claude", "sample", None)],
        Path::new("/app"),
        None,
    )
    .unwrap();
    assert!(save(&dir, e, vec![], Path::new("/app"), None).is_err());
}
#[test]
fn interrupted_transaction_rolls_back_without_overwriting_external_changes() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    let a = dir.join("agent.json");
    std::fs::write(&a, "new").unwrap();
    let journal = Journal {
        changes: vec![Change {
            path: a.clone(),
            before: Some("old".into()),
            after: Some("new".into()),
        }],
        catalog: Change {
            path: dir.join("catalog.json"),
            before: None,
            after: Some("committed".into()),
        },
    };
    atomic::write_json_file(&dir.join("management-transaction.json"), &journal).unwrap();
    recover(dir).unwrap();
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "old");
    std::fs::write(&a, "external").unwrap();
    atomic::write_json_file(&dir.join("management-transaction.json"), &journal).unwrap();
    assert!(recover(dir).is_err());
    assert_eq!(std::fs::read_to_string(a).unwrap(), "external");
    let deleted = dir.join("project-mcp.json");
    let deletion = Journal {
        changes: vec![Change {
            path: deleted.clone(),
            before: Some("project config".into()),
            after: None,
        }],
        catalog: Change {
            path: dir.join("catalog.json"),
            before: None,
            after: Some("committed".into()),
        },
    };
    atomic::write_json_file(&dir.join("management-transaction.json"), &deletion).unwrap();
    recover(dir).unwrap();
    assert_eq!(std::fs::read_to_string(deleted).unwrap(), "project config");
}
#[test]
fn claude_local_scope_is_preserved_during_migration() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("claude.json");
    let dir = root.path().join("studio");
    let old = json!({"command":"demo"});
    std::fs::write(&path,json!({"projects":{"/work":{"mcpServers":{"tool":old},"other":"keep"}},"mcpServers":{"global":{"command":"global"}}}).to_string()).unwrap();
    let t = Target {
        project: Some("/work".into()),
        ..target(path.clone(), "claude", "tool", Some(old.clone()))
    };
    save(
        &dir,
        entry(native::canonical(&old, "claude"), "gateway"),
        vec![t],
        Path::new("/app"),
        None,
    )
    .unwrap();
    remove(&dir, "sample", true).unwrap();
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(doc["projects"]["/work"]["mcpServers"]["tool"], old);
    assert_eq!(doc["projects"]["/work"]["other"], "keep");
    assert_eq!(doc["mcpServers"]["global"]["command"], "global");
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
#[test]
fn groups_switch_restore_and_isolate_agents_and_provider_settings() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let claude = root.path().join("claude.json");
    let codex = root.path().join("config.toml");
    std::fs::write(&claude, r#"{"env":{"ANTHROPIC_BASE_URL":"provider"},"projects":{"/work":{"mcpServers":{"project":{"command":"project"}}}},"mcpServers":{"manual":{"command":"manual"}}}"#).unwrap();
    std::fs::write(
        &codex,
        "# provider\nmodel_provider='ccswitch'\n[mcp_servers.manual]\ncommand='manual'\n",
    )
    .unwrap();
    let a = entry(json!({"type":"stdio","command":"a"}), "direct");
    let mut b = entry(
        json!({"type":"http","url":"https://example.com/mcp"}),
        "gateway",
    );
    b.id = "b".into();
    b.name = "B".into();
    save(&dir, a, vec![], Path::new("/app"), None).unwrap();
    save(&dir, b, vec![], Path::new("/app"), None).unwrap();
    groups::save_group(&dir, group("one", &["sample"]), vec![], None).unwrap();
    groups::save_group(&dir, group("two", &["b"]), vec![], None).unwrap();
    groups::save_group(&dir, group("code", &["sample"]), vec![], None).unwrap();
    assert!(native::entry(
        &std::fs::read_to_string(&claude).unwrap(),
        "claude",
        None,
        "Sample"
    )
    .unwrap()
    .is_none());
    groups::activate(
        &dir,
        "claude",
        &["one".to_string()],
        &claude,
        Path::new("/app"),
    )
    .unwrap();
    groups::activate(
        &dir,
        "codex",
        &["code".to_string()],
        &codex,
        Path::new("/app"),
    )
    .unwrap();
    let codex_active = std::fs::read_to_string(&codex).unwrap();
    groups::activate(
        &dir,
        "claude",
        &["two".to_string()],
        &claude,
        Path::new("/app"),
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&codex).unwrap(), codex_active);
    let current = std::fs::read_to_string(&claude).unwrap();
    assert!(native::entry(&current, "claude", None, "Sample")
        .unwrap()
        .is_none());
    assert_eq!(
        native::entry(&current, "claude", None, "B")
            .unwrap()
            .unwrap()["args"][0],
        "--mcp-client"
    );
    assert!(!dir.join("config.json").exists()); // No gateway start required.
    groups::activate(&dir, "claude", &[], &claude, Path::new("/app")).unwrap();
    let final_doc: Value =
        serde_json::from_str(&std::fs::read_to_string(&claude).unwrap()).unwrap();
    assert_eq!(final_doc["env"]["ANTHROPIC_BASE_URL"], "provider");
    assert_eq!(
        final_doc["mcpServers"],
        json!({"manual":{"command":"manual"}})
    );
    assert_eq!(
        final_doc["projects"]["/work"]["mcpServers"]["project"]["command"],
        "project"
    );
    assert!(read(&dir).unwrap().active_groups.contains_key("codex"));
    groups::activate(&dir, "codex", &[], &codex, Path::new("/app")).unwrap();
    let final_codex = std::fs::read_to_string(&codex).unwrap();
    assert!(final_codex.starts_with("# provider\nmodel_provider='ccswitch'\n"));
    let doc: Value = toml_edit::de::from_str(&final_codex).unwrap();
    assert_eq!(doc["mcp_servers"], json!({"manual":{"command":"manual"}}));
}
#[test]
fn group_edits_are_pending_and_external_drift_is_left_alone_on_switch() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    let a = entry(json!({"type":"stdio","command":"a"}), "direct");
    save(&dir, a, vec![], Path::new("/app"), None).unwrap();
    groups::save_group(&dir, group("one", &["sample"]), vec![], None).unwrap();
    groups::activate(
        &dir,
        "codex",
        &["one".to_string()],
        &path,
        Path::new("/app"),
    )
    .unwrap();
    let old = read(&dir).unwrap().entries[0].clone();
    let mut updated = old.clone();
    updated.definition["command"] = json!("new");
    let before = std::fs::read_to_string(&path).unwrap();
    save(&dir, updated, vec![], Path::new("/app"), Some(&old)).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    assert!(groups::remove_group(&dir, "one").is_err());
    assert!(remove(&dir, "sample", true).is_err());
    groups::activate(
        &dir,
        "codex",
        &["one".to_string()],
        &path,
        Path::new("/app"),
    )
    .unwrap();
    assert_eq!(
        native::entry(
            &std::fs::read_to_string(&path).unwrap(),
            "codex",
            None,
            "Sample"
        )
        .unwrap()
        .unwrap()["command"],
        "new"
    );
    groups::save_group(&dir, group("two", &["sample"]), vec![], None).unwrap();
    std::fs::write(
        &path,
        "model_provider='custom'\n[mcp_servers.Sample]\ncommand='externally-edited'\n",
    )
    .unwrap();
    let changed = std::fs::read(&path).unwrap();
    // 被外部改过的接入不再拦住切换：两个分组都要 Sample，原样留着、仍归分组管，并报告出来
    let untouched = groups::activate(
        &dir,
        "codex",
        &["two".to_string()],
        &path,
        Path::new("/app"),
    )
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), changed);
    assert_eq!(
        untouched
            .iter()
            .map(|u| (u.key.as_str(), u.tracked))
            .collect::<Vec<_>>(),
        [("Sample", true)]
    );
    let catalog = read(&dir).unwrap();
    assert_eq!(catalog.active_groups["codex"][0].group_id, "two");
    assert!(groups::issues(&catalog)["codex"].contains_key("two"));
}
#[test]
fn stopping_a_group_leaves_a_connection_another_tool_rewrote() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    save(
        &dir,
        entry(json!({"type":"stdio","command":"a"}), "direct"),
        vec![],
        Path::new("/app"),
        None,
    )
    .unwrap();
    groups::save_group(&dir, group("one", &["sample"]), vec![], None).unwrap();
    groups::activate(
        &dir,
        "codex",
        &["one".to_string()],
        &path,
        Path::new("/app"),
    )
    .unwrap();
    std::fs::write(
        &path,
        "[mcp_servers.Sample]\ncommand='rewritten-by-another-tool'\n",
    )
    .unwrap();
    let rewritten = std::fs::read(&path).unwrap();
    let untouched =
        groups::activate(&dir, "codex", &[], &PathBuf::new(), Path::new("/app")).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), rewritten);
    assert_eq!(
        untouched
            .iter()
            .map(|u| (u.agent.as_str(), u.key.as_str(), u.tracked))
            .collect::<Vec<_>>(),
        [("codex", "Sample", false)]
    );
    let catalog = read(&dir).unwrap();
    assert!(!catalog.active_groups.contains_key("codex"));
    assert!(groups::issues(&catalog).is_empty());
}
#[test]
fn stopping_one_group_is_not_blocked_by_a_rewritten_connection_in_another() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    for (id, command) in [("alpha", "a"), ("beta", "b"), ("gamma", "g")] {
        let mut e = entry(json!({"type":"stdio","command":command}), "direct");
        e.id = id.into();
        e.name = id.into();
        save(&dir, e, vec![], Path::new("/app"), None).unwrap();
    }
    groups::save_group(&dir, group("a", &["alpha"]), vec![], None).unwrap();
    groups::save_group(&dir, group("b", &["beta"]), vec![], None).unwrap();
    groups::save_group(&dir, group("c", &["gamma"]), vec![], None).unwrap();
    groups::activate(
        &dir,
        "codex",
        &["a".to_string(), "b".to_string()],
        &path,
        Path::new("/app"),
    )
    .unwrap();
    let mut doc: toml_edit::DocumentMut = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    doc["mcp_servers"]["beta"]["command"] = toml_edit::value("rewritten");
    // 新成员的同名位置上已有别人的配置
    doc["mcp_servers"]["gamma"]["command"] = toml_edit::value("someone-else");
    std::fs::write(&path, doc.to_string()).unwrap();
    let untouched =
        groups::activate(&dir, "codex", &["b".to_string()], &path, Path::new("/app")).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(native::entry(&text, "codex", None, "alpha").unwrap(), None);
    assert_eq!(
        native::entry(&text, "codex", None, "beta")
            .unwrap()
            .unwrap()["command"],
        "rewritten"
    );
    assert_eq!(
        untouched
            .iter()
            .map(|u| (u.key.as_str(), u.tracked))
            .collect::<Vec<_>>(),
        [("beta", true)]
    );
    assert_eq!(
        groups::issues(&read(&dir).unwrap())["codex"]
            .keys()
            .collect::<Vec<_>>(),
        ["b"]
    );
    // 启用新成员时仍然不覆盖别人的同名配置
    let before = std::fs::read(&path).unwrap();
    assert!(groups::activate(
        &dir,
        "codex",
        &["b".to_string(), "c".to_string()],
        &path,
        Path::new("/app"),
    )
    .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
#[test]
fn deployment_issues_name_the_groups_whose_connections_were_changed() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    for (id, command) in [("alpha", "a"), ("beta", "b"), ("shared", "s")] {
        let mut e = entry(json!({"type":"stdio","command":command}), "direct");
        e.id = id.into();
        e.name = id.into();
        save(&dir, e, vec![], Path::new("/app"), None).unwrap();
    }
    groups::save_group(&dir, group("a", &["alpha", "shared"]), vec![], None).unwrap();
    groups::save_group(&dir, group("b", &["beta", "shared"]), vec![], None).unwrap();
    groups::activate(
        &dir,
        "codex",
        &["a".to_string(), "b".to_string()],
        &path,
        Path::new("/app"),
    )
    .unwrap();
    assert!(groups::issues(&read(&dir).unwrap()).is_empty());
    let rewrite = |key: &str, command: &str| {
        let mut doc: toml_edit::DocumentMut =
            std::fs::read_to_string(&path).unwrap().parse().unwrap();
        doc["mcp_servers"][key]["command"] = toml_edit::value(command);
        std::fs::write(&path, doc.to_string()).unwrap();
    };
    // 另一个程序只改了 b 自己的成员：只有 b 需要检查，同一 Agent 上的 a 不受牵连
    rewrite("beta", "rewritten");
    let issues = groups::issues(&read(&dir).unwrap());
    assert_eq!(issues["codex"].keys().collect::<Vec<_>>(), ["b"]);
    assert!(issues["codex"]["b"].contains("beta"));
    rewrite("beta", "b");
    assert!(groups::issues(&read(&dir).unwrap()).is_empty());
    // 共用的成员只记在第一个分组（a）名下，但 b 也包含它：两个都要标出
    rewrite("shared", "rewritten");
    let issues = groups::issues(&read(&dir).unwrap());
    assert_eq!(issues["codex"].keys().collect::<Vec<_>>(), ["a", "b"]);
    assert!(issues["codex"]["a"].contains("shared"));
    assert!(issues["codex"]["b"].contains("shared"));
}
#[test]
fn group_references_do_not_adopt_or_overwrite_manual_entries() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    let original = "# keep formatting\n[mcp_servers.Sample]\ncommand='a'\n";
    std::fs::write(&path, original).unwrap();
    let definition = json!({"command":"a"});
    let mut imported = entry(native::canonical(&definition, "codex"), "direct");
    imported.name = "Display name".into();
    imported.bindings.push(Binding {
        id: "source".into(),
        agent: "codex".into(),
        path: path.clone(),
        project: None,
        key: "Sample".into(),
        original: Some(definition.clone()),
        installed: definition,
    });
    groups::save_group(&dir, group("one", &["sample"]), vec![imported], None).unwrap();
    assert!(read(&dir).unwrap().entries.is_empty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    groups::activate(
        &dir,
        "codex",
        &["one".to_string()],
        &path,
        Path::new("/app"),
    )
    .unwrap();
    assert!(read(&dir).unwrap().active_groups["codex"][0]
        .bindings
        .is_empty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    groups::activate(&dir, "codex", &[], &path, Path::new("/app")).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    std::fs::write(&path, "[mcp_servers.Sample]\ncommand='changed'\n").unwrap();
    assert!(groups::activate(
        &dir,
        "codex",
        &["one".to_string()],
        &path,
        Path::new("/app")
    )
    .is_err());
    assert!(read(&dir).unwrap().active_groups.is_empty());
}
#[test]
fn groups_validate_missing_members_names_and_order_without_writing_targets() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("claude.json");
    groups::save_group(&dir, group("one", &["missing"]), vec![], None).unwrap();
    groups::save_group(&dir, group("two", &[]), vec![], None).unwrap();
    let mut duplicate = group("three", &[]);
    duplicate.name = "one".into();
    assert!(groups::save_group(&dir, duplicate, vec![], None).is_err());
    assert!(groups::activate(
        &dir,
        "codex",
        &["one".to_string()],
        &path,
        Path::new("/app")
    )
    .is_err());
    assert!(groups::activate(
        &dir,
        "claude",
        &["one".to_string()],
        &path,
        Path::new("/app")
    )
    .is_err());
    assert!(groups::activate(
        &dir,
        "claude",
        &["two".to_string()],
        &path,
        Path::new("/app")
    )
    .is_err());
    assert!(!path.exists());
    groups::reorder(&dir, &["two".into(), "one".into()]).unwrap();
    assert!(groups::reorder(&dir, &["two".into(), "two".into()]).is_err());
    assert_eq!(
        read(&dir)
            .unwrap()
            .groups
            .iter()
            .find(|g| g.id == "two")
            .unwrap()
            .sort_order,
        0
    );
    groups::remove_group(&dir, "one").unwrap();
}

/// A new group lands after the current last one, even when earlier groups were
/// deleted and `len()` would repeat an existing slot.
#[test]
fn new_group_is_appended_after_deletions() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    for id in ["a", "b", "c"] {
        groups::save_group(&dir, group(id, &[]), vec![], None).unwrap();
    }
    groups::remove_group(&dir, "a").unwrap();
    groups::remove_group(&dir, "b").unwrap();
    groups::save_group(&dir, group("d", &[]), vec![], None).unwrap();
    let catalog = read(&dir).unwrap();
    let order = |id: &str| {
        catalog
            .groups
            .iter()
            .find(|g| g.id == id)
            .unwrap()
            .sort_order
    };
    assert!(order("d") > order("c"), "c={} d={}", order("c"), order("d"));
}

/// A group is not scoped to one agent any more: it can be the active
/// combination for several agents at the same time, each with its own
/// native file and `active_groups` entry, and stopping one must not touch
/// the other.
#[test]
fn group_can_be_active_for_two_agents_at_once() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let claude = root.path().join("claude.json");
    let codex = root.path().join("config.toml");
    save(
        &dir,
        entry(json!({"type":"stdio","command":"a"}), "direct"),
        vec![],
        Path::new("/app"),
        None,
    )
    .unwrap();
    groups::save_group(&dir, group("shared", &["sample"]), vec![], None).unwrap();
    groups::activate(
        &dir,
        "claude",
        &["shared".to_string()],
        &claude,
        Path::new("/app"),
    )
    .unwrap();
    groups::activate(
        &dir,
        "codex",
        &["shared".to_string()],
        &codex,
        Path::new("/app"),
    )
    .unwrap();
    let catalog = read(&dir).unwrap();
    assert_eq!(catalog.active_groups["claude"][0].group_id, "shared");
    assert_eq!(catalog.active_groups["codex"][0].group_id, "shared");
    assert!(native::entry(
        &std::fs::read_to_string(&claude).unwrap(),
        "claude",
        None,
        "Sample"
    )
    .unwrap()
    .is_some());
    assert!(native::entry(
        &std::fs::read_to_string(&codex).unwrap(),
        "codex",
        None,
        "Sample"
    )
    .unwrap()
    .is_some());
    // Stopping claude's copy must not disturb codex's independent one.
    groups::activate(&dir, "claude", &[], &claude, Path::new("/app")).unwrap();
    assert!(native::entry(
        &std::fs::read_to_string(&claude).unwrap(),
        "claude",
        None,
        "Sample"
    )
    .unwrap()
    .is_none());
    let catalog = read(&dir).unwrap();
    assert!(!catalog.active_groups.contains_key("claude"));
    assert_eq!(catalog.active_groups["codex"][0].group_id, "shared");
    assert!(native::entry(
        &std::fs::read_to_string(&codex).unwrap(),
        "codex",
        None,
        "Sample"
    )
    .unwrap()
    .is_some());
}

#[test]
fn deleting_unmanaged_sources_preserves_other_keys_and_rejects_stale_snapshots() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let claude = root.path().join("claude.json");
    let codex = root.path().join("config.toml");
    let original = r#"{"env":{"KEEP":"yes"},"mcpServers":{"sample":{"command":"sample"},"other":{"command":"other"}}}"#;
    std::fs::write(&claude, original).unwrap();
    std::fs::write(
        &codex,
        "model='keep'\n[mcp_servers.sample]\ncommand='changed'\n",
    )
    .unwrap();
    let targets = || {
        vec![
            target(
                claude.clone(),
                "claude",
                "sample",
                Some(json!({"command":"sample"})),
            ),
            target(
                codex.clone(),
                "codex",
                "sample",
                Some(json!({"command":"sample"})),
            ),
        ]
    };
    assert!(remove_sources(&dir, targets()).is_err());
    assert_eq!(std::fs::read_to_string(&claude).unwrap(), original);
    std::fs::write(
        &codex,
        "model='keep'\n[mcp_servers.sample]\ncommand='sample'\n",
    )
    .unwrap();
    remove_sources(&dir, targets()).unwrap();
    let text = std::fs::read_to_string(&claude).unwrap();
    assert!(native::entry(&text, "claude", None, "sample")
        .unwrap()
        .is_none());
    assert!(native::entry(&text, "claude", None, "other")
        .unwrap()
        .is_some());
    assert!(text.contains("KEEP"));
    let text = std::fs::read_to_string(&codex).unwrap();
    assert!(native::entry(&text, "codex", None, "sample")
        .unwrap()
        .is_none());
    assert!(text.contains("keep"));
    assert!(read(&dir).unwrap().entries.is_empty());
}

#[test]
fn deleting_scanned_sources_rejects_managed_bindings() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("claude.json");
    save(
        &dir,
        entry(json!({"type":"stdio","command":"sample"}), "direct"),
        vec![target(path.clone(), "claude", "sample", None)],
        Path::new("/app"),
        None,
    )
    .unwrap();
    let catalog = read(&dir).unwrap();
    assert!(remove_sources(&dir, current_targets(&catalog.entries[0])).is_err());
    assert!(native::entry(
        &std::fs::read_to_string(path).unwrap(),
        "claude",
        None,
        "sample"
    )
    .unwrap()
    .is_some());
}

#[test]
fn deleting_scanned_sources_rejects_active_group_bindings() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    save(
        &dir,
        entry(json!({"type":"stdio","command":"sample"}), "direct"),
        vec![],
        Path::new("/app"),
        None,
    )
    .unwrap();
    groups::save_group(&dir, group("one", &["sample"]), vec![], None).unwrap();
    groups::activate(
        &dir,
        "codex",
        &["one".to_string()],
        &path,
        Path::new("/app"),
    )
    .unwrap();
    let before = std::fs::read_to_string(&path).unwrap();
    let current = native::entry(&before, "codex", None, "Sample")
        .unwrap()
        .unwrap();
    assert!(remove_sources(
        &dir,
        vec![target(path.clone(), "codex", "Sample", Some(current))]
    )
    .is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn referenced_group_deploys_only_missing_target_and_removes_only_its_copy() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let source = root.path().join("claude.json");
    let target = root.path().join("config.toml");
    let definition = json!({"command":"demo"});
    std::fs::write(
        &source,
        json!({"mcpServers":{"Sample":definition}}).to_string(),
    )
    .unwrap();
    let original = std::fs::read(&source).unwrap();
    let mut reference = entry(native::canonical(&definition, "claude"), "direct");
    reference.bindings.push(Binding {
        id: "source".into(),
        agent: "claude".into(),
        path: source.clone(),
        project: None,
        key: "Sample".into(),
        original: Some(definition.clone()),
        installed: definition,
    });
    groups::save_group(&dir, group("one", &["sample"]), vec![reference], None).unwrap();
    assert!(read(&dir).unwrap().entries.is_empty());
    assert!(!target.exists());
    for _ in 0..2 {
        groups::activate(
            &dir,
            "codex",
            &["one".to_string()],
            &target,
            Path::new("/app"),
        )
        .unwrap();
        assert_eq!(
            read(&dir).unwrap().active_groups["codex"][0].bindings.len(),
            1
        );
    }
    groups::activate(&dir, "codex", &[], &target, Path::new("/app")).unwrap();
    assert!(native::entry(
        &std::fs::read_to_string(&target).unwrap(),
        "codex",
        None,
        "Sample"
    )
    .unwrap()
    .is_none());
    groups::remove_group(&dir, "one").unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert!(read(&dir).unwrap().entries.is_empty());
}

#[test]
fn group_does_not_reenable_manually_disabled_mcp() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    let definition = json!({"command":"demo","enabled":false});
    let mut reference = entry(native::canonical(&definition, "codex"), "direct");
    std::fs::write(
        &path,
        "[mcp_servers.Sample]\ncommand='demo'\nenabled=false\n",
    )
    .unwrap();
    let original = std::fs::read(&path).unwrap();
    reference.bindings.push(Binding {
        id: "source".into(),
        agent: "codex".into(),
        path: path.clone(),
        project: None,
        key: "Sample".into(),
        original: Some(definition.clone()),
        installed: definition,
    });
    groups::save_group(&dir, group("one", &["sample"]), vec![reference], None).unwrap();
    assert!(groups::activate(
        &dir,
        "codex",
        &["one".to_string()],
        &path,
        Path::new("/app")
    )
    .unwrap_err()
    .to_string()
    .contains("手动停用"));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(read(&dir).unwrap().active_groups.is_empty());
}

/// 目录文件（catalog.json）没有版本号：旧版每个 agent 只存一个分组对象，
/// 现在可以叠加多个，两种形状都要能读。
#[test]
fn old_single_active_group_object_in_the_catalog_loads_as_a_stack() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("catalog.json"),
        r#"{
            "entries": [],
            "groups": [{"id":"g","name":"G","entryIds":["e"],"sortOrder":0}],
            "activeGroups": {
                "claude": {"groupId":"g","entries":[],"bindings":[]}
            }
        }"#,
    )
    .unwrap();
    let catalog = read(&dir).unwrap();
    assert_eq!(catalog.active_groups["claude"].len(), 1);
    assert_eq!(catalog.active_groups["claude"][0].group_id, "g");
}
