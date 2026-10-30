//! MCP 写入同样必须在配置锁内检查已经关闭的 CLI，而桌面操作仍应允许。
use serde_json::json;
use serial_test::serial;
use skill_studio_core::{fs::paths, services::store::Store};
use skill_studio_mcp::management::{groups::Group, Entry};
use skill_studio_service::{mcp, projects, state::AppState};

struct Env(tempfile::TempDir);
impl Env {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, temp.path());
        Self(temp)
    }
    fn state(&self) -> AppState {
        AppState::bootstrap(Store::new(self.0.path().join(".skill-studio"))).unwrap()
    }
    fn entry(&self, state: &AppState) -> Entry {
        let entry = Entry {
            id: "entry".into(),
            name: "entry".into(),
            mode: "direct".into(),
            definition: json!({"type":"stdio","command":"tool"}),
            ..Default::default()
        };
        mcp::entries::save(state, None, entry.clone(), None, &Default::default()).unwrap();
        entry
    }
    fn disable(&self) {
        self.state()
            .mutate(|_, config| {
                config.settings.cli_skill_enabled = false;
                Ok(())
            })
            .unwrap();
    }
}
impl Drop for Env {
    fn drop(&mut self) {
        std::env::remove_var(paths::TEST_HOME_ENV);
    }
}

#[test]
#[serial]
fn already_started_cli_cannot_edit_an_mcp_after_the_switch_is_off() {
    let env = Env::new();
    let desktop = env.state();
    let entry = env.entry(&desktop);
    let cli = env.state().for_interactive_cli();
    env.disable();
    let mut next = entry.clone();
    next.definition["command"] = json!("changed");
    let error = mcp::entries::save(&cli, None, next, Some(entry), &Default::default()).unwrap_err();
    assert!(error.contains("设置中关闭"), "{error}");
}

#[test]
#[serial]
fn already_started_cli_cannot_save_an_mcp_group_after_the_switch_is_off() {
    let env = Env::new();
    let desktop = env.state();
    env.entry(&desktop);
    let cli = env.state().for_interactive_cli();
    env.disable();
    let group = Group {
        id: "group".into(),
        name: "group".into(),
        entry_ids: vec!["entry".into()],
        references: vec![],
        sort_order: 0,
    };
    let error = mcp::groups::save(&cli, group, &[], None).unwrap_err();
    assert!(error.contains("设置中关闭"), "{error}");
}

#[test]
#[serial]
fn already_started_cli_cannot_save_project_mcps_after_the_switch_is_off() {
    let env = Env::new();
    let desktop = env.state();
    env.entry(&desktop);
    let root = env.0.path().join("work");
    std::fs::create_dir(&root).unwrap();
    let project = projects::create_project(&desktop, "app".into(), root).unwrap();
    let cli = env.state().for_interactive_cli();
    env.disable();
    let members = mcp::projects::Members {
        entry_ids: vec!["entry".into()],
        ..Default::default()
    };
    let error = mcp::projects::save(&cli, None, &project.id, members, &[], None).unwrap_err();
    assert!(error.contains("设置中关闭"), "{error}");
}

#[test]
#[serial]
fn desktop_mcp_edits_are_not_disabled_with_the_cli_switch() {
    let env = Env::new();
    let desktop = env.state();
    let entry = env.entry(&desktop);
    env.disable();
    let mut next = entry.clone();
    next.definition["command"] = json!("changed");
    mcp::entries::save(&desktop, None, next, Some(entry), &Default::default()).unwrap();
}
