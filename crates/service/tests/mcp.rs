//! MCP 的服务层：桌面应用和命令行共用。启停与保存在托管状态的锁内按最新内容计算，删除
//! 项目时一并清掉它的 MCP 记录，只读打开时不写任何文件。
//!
//! 每个 `AppState` 代表一个进程（桌面应用、一次命令行调用），见 concurrent_writers.rs。

use serial_test::serial;
use skill_studio_core::fs::paths;
use skill_studio_core::services::store::Store;
use skill_studio_mcp::management::{self, groups::Group, Entry, OriginalSource, Provenance};
use skill_studio_service::mcp::{
    self,
    entries::{Placement, SourceRef},
    projects::Members,
};
use skill_studio_service::settings::{self, SettingsPatch};
use skill_studio_service::{projects, state::AppState};
use std::path::PathBuf;

struct Env {
    home: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, home.path());
        for var in [
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "XDG_CONFIG_HOME",
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_CONFIG",
            "PI_CODING_AGENT_DIR",
            "GROK_HOME",
        ] {
            std::env::remove_var(var);
        }
        std::fs::create_dir_all(home.path().join(".codex")).unwrap();
        Self { home }
    }

    fn dir(&self) -> PathBuf {
        self.home.path().join(".skill-studio")
    }

    fn process(&self) -> AppState {
        AppState::bootstrap(Store::new(self.dir())).unwrap()
    }

    fn codex(&self) -> String {
        std::fs::read_to_string(self.home.path().join(".codex/config.toml")).unwrap_or_default()
    }

    fn catalog(&self) -> management::Catalog {
        management::read(&self.dir().join("mcp")).unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        std::env::remove_var(paths::TEST_HOME_ENV);
    }
}

fn hub(state: &AppState, id: &str) {
    let entry = Entry {
        id: id.into(),
        name: id.into(),
        mode: "direct".into(),
        definition: serde_json::json!({"type":"stdio","command":id}),
        ..Default::default()
    };
    mcp::entries::save(state, None, entry, None, &Placement::default()).unwrap();
}

#[test]
#[serial]
fn provenance_new_studio_entry_survives_group_project_deploy_and_stop() {
    let env = Env::new();
    let state = env.process();
    hub(&state, "alpha");
    let origin = env.catalog().entries[0].provenance.clone();
    assert_eq!(
        origin,
        Some(Provenance {
            source_ids: vec!["studio".into()],
            original_sources: vec![],
        })
    );
    group(&state, "a", &["alpha"]);
    mcp::groups::set(&state, None, "codex", "a", true).unwrap();
    let catalog = env.catalog();
    assert_eq!(catalog.entries[0].provenance, origin);
    assert_eq!(
        catalog.active_groups["codex"][0].entries[0].provenance,
        origin
    );
    let id = project(&env, &state);
    mcp::projects::save(
        &state,
        None,
        &id,
        Members {
            entry_ids: vec!["alpha".into()],
            ..Default::default()
        },
        &[],
        None,
    )
    .unwrap();
    mcp::projects::activate(&state, None, &id, "codex", true).unwrap();
    assert_eq!(
        env.catalog().projects[&id].active["codex"].entries[0].provenance,
        origin
    );
    for enabled in [false, true] {
        settings::update_settings(
            &state,
            SettingsPatch {
                manage_mcp: Some(enabled),
                ..Default::default()
            },
        )
        .unwrap();
        let catalog = env.catalog();
        assert_eq!(catalog.entries[0].provenance, origin);
        assert_eq!(
            catalog.active_groups["codex"][0].entries[0].provenance,
            origin
        );
        assert_eq!(
            catalog.projects[&id].active["codex"].entries[0].provenance,
            origin
        );
    }
    mcp::groups::set(&state, None, "codex", "a", false).unwrap();
    mcp::projects::activate(&state, None, &id, "codex", false).unwrap();
    assert_eq!(env.catalog().entries[0].provenance, origin);
    assert!(!env.codex().contains("alpha"));
}

#[test]
#[serial]
fn provenance_adoption_uses_rescanned_sources_and_survives_edits_and_target_changes() {
    let env = Env::new();
    let state = env.process();
    let path = env.home.path().join(".codex/config.toml");
    std::fs::write(&path, "[mcp_servers.original]\ncommand='original-server'\n[mcp_servers.original.env]\nTOKEN='source-secret'\n").unwrap();
    let scanned = mcp::overview(&state, None).unwrap().scanned;
    let source = scanned
        .discovered
        .iter()
        .flat_map(|entry| &entry.sources)
        .find(|source| source.agent == "codex" && source.key == "original")
        .unwrap();
    let input = Entry {
        id: "adopted".into(),
        name: "adopted".into(),
        mode: "direct".into(),
        definition: skill_studio_mcp::native::canonical(&source.definition, "codex"),
        provenance: Some(Provenance {
            source_ids: vec!["forged".into()],
            original_sources: vec![],
        }),
        ..Default::default()
    };
    let mut placement = Placement {
        sources: vec![SourceRef {
            id: source.id.clone(),
            definition: source.definition.clone(),
        }],
        // A fresh deployment is not an original source.
        agents: vec!["claude".into()],
        ..Default::default()
    };
    // The source definition must still match the backend scan; failed adoption records nothing.
    placement.sources[0].definition["command"] = serde_json::json!("stale");
    assert!(mcp::entries::save(&state, None, input.clone(), None, &placement).is_err());
    assert!(env.catalog().entries.is_empty());
    placement.sources[0].definition = source.definition.clone();
    mcp::entries::save(&state, None, input, None, &placement).unwrap();
    let saved = env.catalog().entries.remove(0);
    let origin = Some(Provenance {
        source_ids: vec!["codex".into()],
        original_sources: vec![OriginalSource {
            agent: "codex".into(),
            path,
            project: None,
            key: "original".into(),
        }],
    });
    assert_eq!(saved.provenance, origin);
    assert_eq!(saved.bindings.len(), 2);
    assert!(!serde_json::to_string(&origin)
        .unwrap()
        .contains("source-secret"));
    let mut edited = saved.clone();
    edited.name = "renamed".into();
    edited.definition["command"] = serde_json::json!("edited-server");
    edited.provenance = None; // Older editors omit the new field.
    let placement = Placement {
        binding_ids: saved
            .bindings
            .iter()
            .map(|binding| binding.id.clone())
            .collect(),
        agents: vec!["pi".into()],
        ..Default::default()
    };
    mcp::entries::save(&state, None, edited, Some(saved), &placement).unwrap();
    let saved = env.catalog().entries.remove(0);
    assert_eq!(saved.bindings.len(), 3);
    assert_eq!(saved.provenance, origin);
    // Removing every deployment does not turn an adopted source into a Studio source.
    mcp::entries::save(
        &state,
        None,
        saved.clone(),
        Some(saved),
        &Placement::default(),
    )
    .unwrap();
    let saved = env.catalog().entries.remove(0);
    assert!(saved.bindings.is_empty());
    assert_eq!(saved.provenance, origin);
    group(&state, "adopted-group", &["adopted"]);
    mcp::groups::set(&state, None, "codex", "adopted-group", true).unwrap();
    assert_eq!(
        env.catalog().active_groups["codex"][0].entries[0].provenance,
        origin
    );
    mcp::groups::set(&state, None, "codex", "adopted-group", false).unwrap();
    assert_eq!(env.catalog().entries[0].provenance, origin);
}

#[test]
#[serial]
fn provenance_legacy_entry_stays_unknown_after_edit_deploy_and_stop() {
    let env = Env::new();
    let state = env.process();
    hub(&state, "legacy");
    let mut catalog = env.catalog();
    catalog.entries[0].provenance = None;
    std::fs::write(
        env.dir().join("mcp/catalog.json"),
        serde_json::to_vec(&catalog).unwrap(),
    )
    .unwrap();
    let opened = catalog.entries.remove(0);
    let mut edited = opened.clone();
    edited.name = "legacy-renamed".into();
    edited.provenance = Some(Provenance {
        source_ids: vec!["studio".into()],
        original_sources: vec![],
    });
    mcp::entries::save(&state, None, edited, Some(opened), &Placement::default()).unwrap();
    group(&state, "legacy-group", &["legacy"]);
    mcp::groups::set(&state, None, "codex", "legacy-group", true).unwrap();
    let catalog = env.catalog();
    assert!(catalog.entries[0].provenance.is_none());
    assert!(catalog.active_groups["codex"][0].entries[0]
        .provenance
        .is_none());
    for enabled in [false, true] {
        settings::update_settings(
            &state,
            SettingsPatch {
                manage_mcp: Some(enabled),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(env.catalog().entries[0].provenance.is_none());
    }
    mcp::groups::set(&state, None, "codex", "legacy-group", false).unwrap();
    assert!(env.catalog().entries[0].provenance.is_none());
    assert!(mcp::status(&state, None).unwrap()["entries"][0]
        .get("provenance")
        .is_none());
}

fn group(state: &AppState, id: &str, members: &[&str]) {
    let group = Group {
        id: id.into(),
        name: id.into(),
        entry_ids: members.iter().map(|s| s.to_string()).collect(),
        references: vec![],
        sort_order: 0,
    };
    mcp::groups::save(state, group, &[], None).unwrap();
}

#[test]
#[serial]
fn two_processes_enabling_different_groups_keep_each_others() {
    let env = Env::new();
    let desktop = env.process();
    hub(&desktop, "alpha");
    hub(&desktop, "beta");
    group(&desktop, "a", &["alpha"]);
    group(&desktop, "b", &["beta"]);
    let cli = env.process();
    mcp::groups::set(&cli, None, "codex", "b", true).unwrap();
    mcp::groups::set(&desktop, None, "codex", "a", true).unwrap();
    let active = management::groups::active_ids(&env.catalog(), "codex");
    assert_eq!(active, ["b", "a"]);
    for key in ["alpha", "beta"] {
        let deployed = skill_studio_mcp::native::entry(&env.codex(), "codex", None, key).unwrap();
        assert!(deployed.is_some(), "{key}：{}", env.codex());
    }
}

#[test]
#[serial]
fn an_agent_out_of_management_can_stop_but_not_start_a_group() {
    let env = Env::new();
    let state = env.process();
    hub(&state, "alpha");
    group(&state, "a", &["alpha"]);
    group(&state, "b", &["alpha"]);
    mcp::groups::set(&state, None, "codex", "a", true).unwrap();
    state.set_managed_agents(&["codex".to_string()]).unwrap();
    let error = mcp::groups::set(&state, None, "codex", "b", true).unwrap_err();
    assert_eq!(error, "请先开启此 Agent 的管理");
    assert_eq!(
        mcp::groups::reapply(&state, None, "codex").unwrap_err(),
        "请先开启此 Agent 的管理"
    );
    mcp::groups::set(&state, None, "codex", "a", false).unwrap();
    assert!(!env.codex().contains("alpha"));
    // 没有启用的分组时，应用修改什么也不写，也不用管理
    assert!(mcp::groups::reapply(&state, None, "codex")
        .unwrap()
        .is_empty());
}

fn project(env: &Env, state: &AppState) -> String {
    let root = env.home.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    projects::create_project(state, "work".into(), root)
        .unwrap()
        .id
}

#[test]
#[serial]
fn deleting_a_project_clears_its_mcp_record_but_not_while_it_is_in_use() {
    let env = Env::new();
    let state = env.process();
    hub(&state, "alpha");
    let id = project(&env, &state);
    let members = Members {
        entry_ids: vec!["alpha".into()],
        ..Default::default()
    };
    mcp::projects::save(&state, None, &id, members, &[], None).unwrap();
    mcp::projects::activate(&state, None, &id, "codex", true).unwrap();
    let error = projects::delete_project(&state, id.clone()).unwrap_err();
    assert_eq!(error, "请先停用项目里所有 Agent 的 MCP");
    assert_eq!(state.config().projects.len(), 1);
    mcp::projects::activate(&state, None, &id, "codex", false).unwrap();
    projects::delete_project(&state, id.clone()).unwrap();
    assert!(state.config().projects.is_empty());
    assert!(!env.catalog().projects.contains_key(&id));
}

#[test]
#[serial]
fn a_project_deleted_while_mcp_management_is_off_is_not_written_back_later() {
    let env = Env::new();
    let state = env.process();
    hub(&state, "alpha");
    let id = project(&env, &state);
    let members = Members {
        entry_ids: vec!["alpha".into()],
        ..Default::default()
    };
    mcp::projects::save(&state, None, &id, members, &[], None).unwrap();
    mcp::projects::activate(&state, None, &id, "codex", true).unwrap();
    let file = env.home.path().join("work/.codex/config.toml");
    assert!(std::fs::read_to_string(&file).unwrap().contains("alpha"));
    let off = SettingsPatch {
        manage_mcp: Some(false),
        ..Default::default()
    };
    settings::update_settings(&state, off).unwrap();
    // 关闭管理时接入已还原；这时删除项目不用先停用
    projects::delete_project(&state, id.clone()).unwrap();
    let on = SettingsPatch {
        manage_mcp: Some(true),
        ..Default::default()
    };
    settings::update_settings(&state, on).unwrap();
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(!text.contains("alpha"), "{text}");
    assert!(!env.catalog().projects.contains_key(&id));
}

#[test]
#[serial]
fn deleting_a_project_never_touches_mcp_when_it_was_never_used() {
    let env = Env::new();
    let state = env.process();
    let id = project(&env, &state);
    projects::delete_project(&state, id).unwrap();
    assert!(!env.dir().join("mcp").exists());
}

#[test]
#[serial]
fn viewing_in_read_only_mode_writes_nothing() {
    let env = Env::new();
    {
        let state = env.process();
        hub(&state, "alpha");
    }
    std::fs::remove_file(env.dir().join("mcp/management.lock")).unwrap();
    let viewer = AppState::read_only(Store::new(env.dir())).unwrap();
    let overview = mcp::overview(&viewer, None).unwrap();
    assert_eq!(overview.catalog.entries.len(), 1);
    assert!(!env.dir().join("mcp/management.lock").exists());
    // 只读打开时任何修改都不做
    let error = mcp::groups::set(&viewer, None, "codex", "a", true).unwrap_err();
    assert_eq!(error, "当前为只读模式，不能修改");
}

#[test]
#[serial]
fn every_operation_stops_while_mcp_management_is_off() {
    let env = Env::new();
    let state = env.process();
    hub(&state, "alpha");
    let off = SettingsPatch {
        manage_mcp: Some(false),
        ..Default::default()
    };
    settings::update_settings(&state, off).unwrap();
    assert_eq!(mcp::overview(&state, None).err().unwrap(), mcp::DISABLED);
    assert_eq!(
        mcp::groups::set(&state, None, "codex", "a", true).unwrap_err(),
        mcp::DISABLED
    );
}

#[test]
#[serial]
fn disabled_agents_reject_new_targets_and_adoption_including_claude_id_mapping() {
    for (agent, app, relative, text) in [
        (
            "codex",
            "codex",
            ".codex/config.toml",
            "[mcp_servers.existing]\ncommand='native'\n",
        ),
        (
            "claude",
            "claude-code",
            ".claude.json",
            r#"{"mcpServers":{"existing":{"command":"native"}}}"#,
        ),
    ] {
        let env = Env::new();
        let path = env.home.path().join(relative);
        std::fs::write(&path, text).unwrap();
        let state = env.process();
        state.set_managed_agent(app, false).unwrap();
        let before = std::fs::read(&path).unwrap();
        let source = mcp::overview(&state, None)
            .unwrap()
            .scanned
            .discovered
            .into_iter()
            .flat_map(|d| d.sources)
            .find(|s| s.agent == agent && s.key == "existing")
            .unwrap();
        let entry = Entry {
            id: "new".into(),
            name: "new".into(),
            mode: "direct".into(),
            definition: serde_json::json!({"type":"stdio","command":"replacement"}),
            ..Default::default()
        };
        for placement in [
            Placement {
                agents: vec![agent.into()],
                ..Default::default()
            },
            Placement {
                sources: vec![SourceRef {
                    id: source.id,
                    definition: source.definition,
                }],
                ..Default::default()
            },
        ] {
            let error =
                mcp::entries::save(&state, None, entry.clone(), None, &placement).unwrap_err();
            assert_eq!(error, "请先开启此 Agent 的管理");
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert!(env.catalog().entries.is_empty());
        }
    }
}

#[test]
#[serial]
fn disabled_binding_is_not_rewritten_or_discarded_but_can_be_explicitly_withdrawn() {
    let env = Env::new();
    let state = env.process();
    hub(&state, "alpha");
    let opened = env.catalog().entries.remove(0);
    mcp::entries::save(
        &state,
        None,
        opened.clone(),
        Some(opened),
        &Placement {
            agents: vec!["codex".into()],
            ..Default::default()
        },
    )
    .unwrap();
    state.set_managed_agent("codex", false).unwrap();
    let opened = env.catalog().entries.remove(0);
    let catalog_path = env.dir().join("mcp/catalog.json");
    let before_catalog = std::fs::read(&catalog_path).unwrap();
    let before_native = env.codex();
    let mut edited = opened.clone();
    edited.definition["command"] = serde_json::json!("changed");
    let error = mcp::entries::save(
        &state,
        None,
        edited,
        Some(opened.clone()),
        &Placement {
            binding_ids: opened.bindings.iter().map(|b| b.id.clone()).collect(),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(error, "请先开启此 Agent 的管理");
    assert_eq!(env.codex(), before_native);
    assert_eq!(std::fs::read(&catalog_path).unwrap(), before_catalog);
    // Omitting bindings is an explicit withdrawal, not an automatic filtered save.
    mcp::entries::save(
        &state,
        None,
        opened.clone(),
        Some(opened),
        &Placement::default(),
    )
    .unwrap();
    assert!(env.catalog().entries[0].bindings.is_empty());
    assert!(!env.codex().contains("mcp_servers.alpha"));
}

#[test]
#[serial]
fn disabled_project_agent_blocks_resave_but_allows_empty_withdrawal() {
    let env = Env::new();
    let state = env.process();
    hub(&state, "alpha");
    hub(&state, "beta");
    let id = project(&env, &state);
    mcp::projects::save(
        &state,
        None,
        &id,
        Members {
            entry_ids: vec!["alpha".into()],
            ..Default::default()
        },
        &[],
        None,
    )
    .unwrap();
    mcp::projects::activate(&state, None, &id, "codex", true).unwrap();
    state.set_managed_agent("codex", false).unwrap();
    let path = env.home.path().join("work/.codex/config.toml");
    let catalog_path = env.dir().join("mcp/catalog.json");
    let before_native = std::fs::read(&path).unwrap();
    let before_catalog = std::fs::read(&catalog_path).unwrap();
    let error = mcp::projects::save(
        &state,
        None,
        &id,
        Members {
            entry_ids: vec!["alpha".into(), "beta".into()],
            ..Default::default()
        },
        &[],
        None,
    )
    .unwrap_err();
    assert_eq!(error, "请先开启此 Agent 的管理");
    assert_eq!(std::fs::read(&path).unwrap(), before_native);
    assert_eq!(std::fs::read(&catalog_path).unwrap(), before_catalog);
    mcp::projects::save(&state, None, &id, Members::default(), &[], None).unwrap();
    assert!(env.catalog().projects[&id].active.is_empty());
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("mcp_servers.alpha"));
}

#[test]
#[serial]
fn disabled_resume_checks_individual_group_and_project_bindings_before_any_write() {
    for placement in ["entry", "group", "project"] {
        let env = Env::new();
        let state = env.process();
        hub(&state, "alpha");
        let path = match placement {
            "entry" => {
                let opened = env.catalog().entries.remove(0);
                mcp::entries::save(
                    &state,
                    None,
                    opened.clone(),
                    Some(opened),
                    &Placement {
                        agents: vec!["codex".into()],
                        ..Default::default()
                    },
                )
                .unwrap();
                env.home.path().join(".codex/config.toml")
            }
            "group" => {
                group(&state, "g", &["alpha"]);
                mcp::groups::set(&state, None, "codex", "g", true).unwrap();
                env.home.path().join(".codex/config.toml")
            }
            _ => {
                let id = project(&env, &state);
                mcp::projects::save(
                    &state,
                    None,
                    &id,
                    Members {
                        entry_ids: vec!["alpha".into()],
                        ..Default::default()
                    },
                    &[],
                    None,
                )
                .unwrap();
                mcp::projects::activate(&state, None, &id, "codex", true).unwrap();
                env.home.path().join("work/.codex/config.toml")
            }
        };
        state.set_mcp_management(false).unwrap();
        state.set_managed_agent("codex", false).unwrap();
        let before_native = std::fs::read(&path).unwrap();
        let catalog_path = env.dir().join("mcp/catalog.json");
        let before_catalog = std::fs::read(&catalog_path).unwrap();
        let error = state.set_mcp_management(true).unwrap_err();
        assert!(
            error.to_string().contains("请先开启此 Agent 的管理"),
            "{error}"
        );
        assert!(!state.config().settings.manage_mcp);
        assert!(env.catalog().suspended);
        assert_eq!(std::fs::read(&path).unwrap(), before_native);
        assert_eq!(std::fs::read(&catalog_path).unwrap(), before_catalog);
        state.set_managed_agent("codex", true).unwrap();
        state.set_mcp_management(true).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            skill_studio_mcp::native::entry(&text, "codex", None, "alpha")
                .unwrap()
                .is_some(),
            "{placement}: {text}"
        );
    }
}

#[test]
#[serial]
fn disabled_sources_can_be_explicitly_removed_without_enabling_management() {
    for (agent, app, relative, text) in [
        ("codex", "codex", ".codex/config.toml", "model='keep'\n[mcp_servers.existing]\ncommand='native'\n[mcp_servers.other]\ncommand='other'\n"),
        ("claude", "claude-code", ".claude.json", r#"{"model":"keep","mcpServers":{"existing":{"command":"native"},"other":{"command":"other"}}}"#),
        ("grok", "grok", ".grok/config.toml", "model='keep'\ndisabled_mcp_servers=['existing']\n[mcp_servers.existing]\ncommand='native'\n[mcp_servers.other]\ncommand='other'\n"),
    ] {
        let env = Env::new();
        let path = env.home.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        let state = env.process();
        state.set_managed_agent(app, false).unwrap();
        let source = mcp::overview(&state, None).unwrap().scanned.discovered.into_iter().flat_map(|d| d.sources).find(|source| source.agent == agent && source.key == "existing").unwrap();
        mcp::entries::remove_sources(&state, &[SourceRef { id: source.id, definition: source.definition }]).unwrap();
        let actual = std::fs::read_to_string(&path).unwrap();
        assert!(skill_studio_mcp::native::entry(&actual, agent, None, "existing").unwrap().is_none());
        assert!(skill_studio_mcp::native::entry(&actual, agent, None, "other").unwrap().is_some());
        assert!(actual.contains("keep"));
        assert!(state.config().settings.disabled_agents.contains(&app.to_string()));
    }
}

#[test]
#[serial]
#[cfg(unix)]
fn mcp_management_toggles_preserve_the_config_dotfile_symlink() {
    let env = Env::new();
    let state = env.process();
    state
        .mutate(|_, config| {
            config.settings.language = "en".into();
            Ok(())
        })
        .unwrap();
    let logical = env.dir().join("config.json");
    let physical = env.home.path().join("dotfiles/config.json");
    std::fs::create_dir_all(physical.parent().unwrap()).unwrap();
    std::fs::rename(&logical, &physical).unwrap();
    std::os::unix::fs::symlink(&physical, &logical).unwrap();
    for enabled in [false, true] {
        state.set_mcp_management(enabled).unwrap();
        assert!(logical.symlink_metadata().unwrap().file_type().is_symlink());
        let config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&physical).unwrap()).unwrap();
        assert_eq!(config["settings"]["manageMcp"], enabled);
        assert_eq!(config["settings"]["language"], "en");
    }
}

#[test]
fn gateway_requirement_covers_individual_group_and_project_deployments() {
    let gateway = Entry {
        id: "gateway".into(),
        name: "Gateway".into(),
        mode: "gateway".into(),
        definition: serde_json::json!({"type":"stdio","command":"demo"}),
        ..Default::default()
    };
    let mut catalog = management::Catalog {
        entries: vec![gateway.clone()],
        ..Default::default()
    };
    assert!(
        !mcp::needs_gateway(&catalog),
        "an unplaced Hub entry alone does not require startup"
    );
    catalog.active_groups.insert(
        "codex".into(),
        vec![management::groups::ActiveGroup {
            group_id: "g".into(),
            entries: vec![gateway.clone()],
            bindings: vec![],
        }],
    );
    assert!(mcp::needs_gateway(&catalog));
    catalog.active_groups.clear();
    catalog.projects.insert(
        "p".into(),
        management::projects::Project {
            root: PathBuf::from("project"),
            active: std::collections::BTreeMap::from([(
                "codex".into(),
                management::projects::Deployment {
                    entries: vec![gateway],
                    bindings: vec![],
                },
            )]),
            ..Default::default()
        },
    );
    assert!(
        mcp::needs_gateway(&catalog),
        "project-only gateway deployments must start the gateway"
    );
    catalog
        .projects
        .get_mut("p")
        .unwrap()
        .active
        .get_mut("codex")
        .unwrap()
        .entries[0]
        .mode = "direct".into();
    assert!(!mcp::needs_gateway(&catalog));
    catalog.entries[0].bindings.push(management::Binding {
        id: "b".into(),
        agent: "codex".into(),
        path: PathBuf::from("config.toml"),
        project: None,
        key: "gateway".into(),
        original: None,
        installed: serde_json::json!({"command":"studio"}),
    });
    assert!(mcp::needs_gateway(&catalog));
}

#[test]
#[serial]
fn entry_save_response_is_the_exact_committed_revision_for_the_next_edit() {
    let env = Env::new();
    let state = env.process();
    hub(&state, "alpha");
    let opened = env.catalog().entries.remove(0);
    mcp::entries::save(
        &state,
        None,
        opened.clone(),
        Some(opened),
        &Placement {
            agents: vec!["codex".into()],
            ..Default::default()
        },
    )
    .unwrap();
    let opened = env.catalog().entries.remove(0);
    let mut submitted = opened.clone();
    submitted.definition["command"] = serde_json::json!("changed");
    let response = mcp::requests::request(
        &state,
        None,
        None,
        "saveEntry",
        &serde_json::json!({
            "entry": submitted, "expectedEntry": opened,
            "bindingIds": submitted.bindings.iter().map(|b| &b.id).collect::<Vec<_>>()
        }),
    )
    .unwrap();
    assert_eq!(
        response,
        serde_json::to_value(&env.catalog().entries[0]).unwrap()
    );
    assert_eq!(response["bindings"][0]["installed"]["command"], "changed");
    assert_ne!(
        response["bindings"][0]["installed"],
        submitted.bindings[0].installed
    );
    let committed: Entry = serde_json::from_value(response).unwrap();
    let mut next = committed.clone();
    next.name = "Next edit".into();
    mcp::entries::save(
        &state,
        None,
        next,
        Some(committed.clone()),
        &Placement {
            binding_ids: committed.bindings.iter().map(|b| b.id.clone()).collect(),
            ..Default::default()
        },
    )
    .unwrap();
}
