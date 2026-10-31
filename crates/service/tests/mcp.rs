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
