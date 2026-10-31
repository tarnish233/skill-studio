//! Provenance is historical Hub metadata, never a native definition or edit revision.
use super::*;

fn entry() -> Entry {
    Entry {
        id: "sample".into(),
        name: "sample".into(),
        mode: "direct".into(),
        definition: json!({"type":"stdio","command":"demo"}),
        ..Default::default()
    }
}

fn claimed_origin() -> Provenance {
    Provenance {
        source_ids: vec!["untrusted".into()],
        original_sources: vec![],
    }
}

#[test]
fn provenance_studio_add_ignores_claimed_origin_and_new_deployment_targets() {
    for deploy in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("studio");
        let mut input = entry();
        input.provenance = Some(claimed_origin());
        let targets = if deploy {
            vec![Target {
                agent: "codex".into(),
                path: tmp.path().join("config.toml"),
                project: None,
                key: "sample".into(),
                expected: None,
            }]
        } else {
            vec![]
        };
        save(&dir, input, targets, Path::new(""), None).unwrap();
        let saved = read(&dir).unwrap().entries.remove(0);
        assert_eq!(
            saved.provenance.unwrap(),
            Provenance {
                source_ids: vec!["studio".into()],
                original_sources: vec![],
            }
        );
        assert_eq!(saved.bindings.len(), usize::from(deploy));
    }
}

#[test]
fn provenance_adoption_records_deduplicated_agent_identities_without_definitions() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("studio");
    let claude = tmp.path().join("claude.json");
    let codex = tmp.path().join("config.toml");
    let project = tmp.path().join("work").to_string_lossy().to_string();
    let original = json!({"command":"demo","env":{"TOKEN":"secret-origin-value"}});
    std::fs::write(
        &claude,
        json!({
            "mcpServers":{"global":original},
            "projects":{project.clone():{"mcpServers":{"local":original}}}
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(&codex, "[mcp_servers.sample]\ncommand='demo'\n[mcp_servers.sample.env]\nTOKEN='secret-origin-value'\n").unwrap();
    let targets = vec![
        Target {
            agent: "claude".into(),
            path: claude.clone(),
            project: None,
            key: "global".into(),
            expected: Some(original.clone()),
        },
        Target {
            agent: "claude".into(),
            path: claude.clone(),
            project: Some(project.clone()),
            key: "local".into(),
            expected: Some(original.clone()),
        },
        Target {
            agent: "codex".into(),
            path: codex.clone(),
            project: None,
            key: "sample".into(),
            expected: Some(original.clone()),
        },
    ];
    let mut with_duplicate = targets.clone();
    with_duplicate.push(targets[0].clone());
    let mut input = entry();
    input.provenance = Some(claimed_origin());
    save(&dir, input, with_duplicate, Path::new(""), None).unwrap();
    let saved = read(&dir).unwrap().entries.remove(0);
    let origin = saved.provenance.unwrap();
    assert_eq!(origin.source_ids, ["claude", "codex"]);
    assert_eq!(
        origin.original_sources,
        targets
            .iter()
            .map(|target| OriginalSource {
                agent: target.agent.clone(),
                path: target.path.clone(),
                project: target.project.clone(),
                key: target.key.clone(),
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(saved.bindings.len(), 3);
    let value = serde_json::to_value(origin).unwrap();
    assert!(value.get("sourceIds").is_some());
    assert!(value.get("originalSources").is_some());
    assert!(!value.to_string().contains("secret-origin-value"));
    for source in value["originalSources"].as_array().unwrap() {
        assert_eq!(source.as_object().unwrap().len(), 4);
        assert!(source.get("definition").is_none());
    }
}

#[test]
fn provenance_edits_preserve_stored_origin_and_ignore_metadata_for_concurrency() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("studio");
    save(&dir, entry(), vec![], Path::new(""), None).unwrap();
    let original = read(&dir).unwrap().entries.remove(0);
    // An editor predating metadata may omit it from its expected snapshot.
    let mut opened = original.clone();
    opened.provenance = None;
    let mut edited = opened.clone();
    edited.name = "renamed".into();
    edited.provenance = Some(claimed_origin());
    save(&dir, edited.clone(), vec![], Path::new(""), Some(&opened)).unwrap();
    let saved = read(&dir).unwrap().entries.remove(0);
    assert_eq!(saved.name, "renamed");
    assert_eq!(saved.provenance, original.provenance);
    // Definition/name/binding changes still make an optimistic snapshot stale.
    assert!(save(&dir, edited, vec![], Path::new(""), Some(&opened)).is_err());

    // A legacy record with no origin must stay unknown even when deployed later.
    let mut catalog = read(&dir).unwrap();
    catalog.entries[0].provenance = None;
    atomic::write_json_file(&dir.join("catalog.json"), &catalog).unwrap();
    let legacy = catalog.entries.remove(0);
    let mut edited = legacy.clone();
    edited.provenance = Some(claimed_origin());
    let target = Target {
        agent: "codex".into(),
        path: tmp.path().join("config.toml"),
        project: None,
        key: "sample".into(),
        expected: None,
    };
    save(&dir, edited, vec![target], Path::new(""), Some(&legacy)).unwrap();
    let saved = read(&dir).unwrap().entries.remove(0);
    assert!(saved.provenance.is_none());
    let mut edited = saved.clone();
    edited.definition["command"] = json!("new-command");
    save(&dir, edited, vec![], Path::new(""), Some(&saved)).unwrap();
    let saved = read(&dir).unwrap().entries.remove(0);
    assert!(saved.bindings.is_empty());
    assert!(saved.provenance.is_none());
}

#[test]
fn provenance_legacy_serde_and_runtime_projection_are_unchanged() {
    let legacy = json!({"id":"sample","name":"sample","mode":"gateway","definition":{"type":"stdio","command":"demo"},"oauth":false,"clientId":null,"scopes":[],"bindings":[]});
    let unknown: Entry = serde_json::from_value(legacy.clone()).unwrap();
    assert!(unknown.provenance.is_none());
    assert_eq!(serde_json::to_value(&unknown).unwrap(), legacy);
    let mut explicit_null = legacy;
    explicit_null["provenance"] = Value::Null;
    assert!(serde_json::from_value::<Entry>(explicit_null)
        .unwrap()
        .provenance
        .is_none());
    let mut known = unknown.clone();
    known.provenance = Some(claimed_origin());
    assert_eq!(
        serde_json::to_value(gateway_server(&known).unwrap()).unwrap(),
        serde_json::to_value(gateway_server(&unknown).unwrap()).unwrap()
    );
    assert_eq!(
        serde_json::to_value(direct_probe_server(&known).unwrap().0).unwrap(),
        serde_json::to_value(direct_probe_server(&unknown).unwrap().0).unwrap()
    );

    // The older gateway config conversion must not infer Studio from managed state.
    let tmp = tempfile::tempdir().unwrap();
    let mut legacy_config = config::Config::default();
    legacy_config
        .servers
        .push(gateway_server(&unknown).unwrap());
    config::save(tmp.path(), &legacy_config).unwrap();
    assert!(read_only(tmp.path()).unwrap().entries[0]
        .provenance
        .is_none());
    assert!(!tmp.path().join("catalog.json").exists());
}
