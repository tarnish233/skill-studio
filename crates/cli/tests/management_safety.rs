//! 服务层的锁与增量更新：模拟桌面和 CLI 分别持有旧快照。
use serde_json::json;
use serial_test::serial;
use skill_studio_core::{fs::paths, services::store::Store};
use skill_studio_service::{
    projects,
    settings::{self, SettingsPatch},
    state::AppState,
};

struct Env(tempfile::TempDir);
impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, dir.path());
        for name in [
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "XDG_CONFIG_HOME",
            "OPENCODE_CONFIG_DIR",
            "PI_CODING_AGENT_DIR",
            "GROK_HOME",
            "DSH_HOME",
            "DSH_AGENTS_HOME",
        ] {
            std::env::remove_var(name);
        }
        Self(dir)
    }
    fn state(&self) -> AppState {
        AppState::bootstrap(Store::new(self.0.path().join(".skill-studio"))).unwrap()
    }
}
impl Drop for Env {
    fn drop(&mut self) {
        std::env::remove_var(paths::TEST_HOME_ENV);
    }
}

#[test]
#[serial]
fn individual_agent_management_merges_with_another_process() {
    let env = Env::new();
    let cli = env.state().for_interactive_cli();
    let desktop = env.state();
    settings::update_settings(
        &desktop,
        SettingsPatch {
            disabled_agents: Some(vec!["pi".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    let patch =
        serde_json::from_value(json!({"managedAgent":{"agent":"codex","enabled":false}})).unwrap();
    settings::update_settings(&cli, patch).unwrap();
    let disabled = settings::get_settings(&desktop).unwrap().disabled_agents;
    assert!(
        disabled.contains(&"pi".into()) && disabled.contains(&"codex".into()),
        "{disabled:?}"
    );
}

#[test]
#[serial]
fn agent_directory_deltas_do_not_replace_other_agents_overrides() {
    let env = Env::new();
    let cli = env.state().for_interactive_cli();
    let desktop = env.state();
    let first = env.0.path().join("custom-codex");
    let second = env.0.path().join("custom-claude");
    settings::update_settings(
        &desktop,
        serde_json::from_value(json!({"agentDirUpdates":{"codex":first}})).unwrap(),
    )
    .unwrap();
    settings::update_settings(
        &cli,
        serde_json::from_value(json!({"agentDirUpdates":{"claude-code":second}})).unwrap(),
    )
    .unwrap();
    let dirs = settings::get_settings(&desktop)
        .unwrap()
        .agent_dir_overrides;
    assert_eq!(dirs["codex"], first);
    assert_eq!(dirs["claude-code"], second);
}

#[test]
#[serial]
fn already_started_cli_file_deletion_stops_if_the_desktop_disables_cli() {
    let env = Env::new();
    let cli = env.state().for_interactive_cli();
    let desktop = env.state();
    let skill = env.0.path().join(".skill-studio/skills/alpha");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "---\nname: alpha\n---\nbody").unwrap();
    desktop
        .mutate(|_, config| {
            config.settings.cli_skill_enabled = false;
            Ok(())
        })
        .unwrap();
    let error = projects::delete_skill_file(&cli, "hub".into(), skill.clone()).unwrap_err();
    assert!(error.contains("设置中关闭"), "{error}");
    assert!(skill.join("SKILL.md").exists());
}

#[test]
#[serial]
fn recovery_rechecks_another_processes_cli_preference_under_the_write_lock() {
    let env = Env::new();
    let desktop = env.state();
    desktop
        .mutate(|_, config| {
            config.settings.theme = "dark".into();
            Ok(())
        })
        .unwrap();
    let store = desktop.studio().store().clone();
    let recovery = AppState::recovery(store.clone()).for_interactive_cli();
    desktop
        .mutate(|_, config| {
            config.settings.cli_skill_enabled = false;
            Ok(())
        })
        .unwrap();
    let backup = store.list_backups().into_iter().next().unwrap();
    let error = recovery.restore_backup(&backup).unwrap_err().to_string();
    assert!(error.contains("设置中关闭"), "{error}");
}
