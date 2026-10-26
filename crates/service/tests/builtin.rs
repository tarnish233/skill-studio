//! 随应用打包的内置 skill：真的能装进 Hub、能扫描、能再次同步

use serial_test::serial;
use skill_studio_core::fs::paths;
use skill_studio_core::services::store::Store;
use skill_studio_service::settings::{self, SettingsPatch};
use skill_studio_service::skills;
use skill_studio_service::state::AppState;

#[test]
#[serial]
fn the_bundle_ships_a_working_builtin_skill() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var(paths::TEST_HOME_ENV, home.path());
    let state = AppState::bootstrap(Store::new(home.path().join(".skill-studio"))).unwrap();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src-tauri/resources/builtin-skills");

    let installed = skills::sync_builtin_skills(&state, &source, "test").unwrap();
    assert_eq!(installed.len(), 1, "应安装一个内置 skill");
    assert_eq!(installed[0].name, "skill-studio");
    let target = home.path().join(".skill-studio/skills/skill-studio");
    assert!(target.join("references/commands.md").is_file());

    let views = skills::scan_skills(&state).unwrap();
    let builtin = views
        .iter()
        .find(|view| view.skill.name == "skill-studio")
        .unwrap();
    assert_eq!(builtin.source_ids, vec!["builtin".to_string()]);
    assert!(
        builtin.skill.description.is_some(),
        "内置 skill 要有 description，Agent 才知道什么时候用"
    );

    // 内容与版本都没变：什么都不写
    assert!(skills::sync_builtin_skills(&state, &source, "test")
        .unwrap()
        .is_empty());
    std::env::remove_var(paths::TEST_HOME_ENV);
}

#[test]
#[serial]
fn the_single_setting_disables_cli_and_safely_uninstalls_then_reinstalls_the_bundle() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var(paths::TEST_HOME_ENV, home.path());
    let store = Store::new(home.path().join(".skill-studio"));
    let state = AppState::bootstrap(store.clone()).unwrap();
    let bundle = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src-tauri/resources/builtin-skills");
    skills::sync_builtin_skills(&state, &bundle, "test").unwrap();
    let target = home.path().join(".skill-studio/skills/skill-studio");

    let off = settings::update_settings(
        &state,
        SettingsPatch {
            cli_skill_enabled: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!off.cli_skill_enabled);
    assert!(off.cli_skill_warning.is_none());
    assert!(!target.exists());
    assert!(
        !settings::get_settings(&AppState::bootstrap(store.clone()).unwrap())
            .unwrap()
            .cli_skill_enabled
    );
    assert!(skills::sync_builtin_skills(&state, &bundle, "next")
        .unwrap()
        .is_empty());
    assert!(!target.exists());

    let on = settings::update_settings_with_builtin(
        &state,
        SettingsPatch {
            cli_skill_enabled: Some(true),
            ..Default::default()
        },
        Some(&bundle),
        "next",
    )
    .unwrap();
    assert!(on.cli_skill_enabled);
    assert!(target.join("SKILL.md").is_file());
    std::env::remove_var(paths::TEST_HOME_ENV);
}

#[test]
#[serial]
fn cli_started_before_the_switch_cannot_write_after_it_is_disabled() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var(paths::TEST_HOME_ENV, home.path());
    let store = Store::new(home.path().join(".skill-studio"));
    let old_cli = AppState::bootstrap(store.clone())
        .unwrap()
        .for_interactive_cli();
    let desktop = AppState::bootstrap(store.clone()).unwrap();
    settings::update_settings(
        &desktop,
        SettingsPatch {
            cli_skill_enabled: Some(false),
            ..Default::default()
        },
    )
    .unwrap();

    assert!(old_cli
        .mutate(|_, config| {
            config.settings.language = "en".into();
            Ok(())
        })
        .unwrap_err()
        .to_string()
        .contains("关闭"));
    assert_eq!(
        AppState::bootstrap(store)
            .unwrap()
            .config()
            .settings
            .language,
        "zh"
    );
    std::env::remove_var(paths::TEST_HOME_ENV);
}

#[test]
#[serial]
fn warning_for_an_edited_builtin_survives_reload_but_is_never_written_to_config() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var(paths::TEST_HOME_ENV, home.path());
    let store = Store::new(home.path().join(".skill-studio"));
    let state = AppState::bootstrap(store.clone()).unwrap();
    let bundle = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src-tauri/resources/builtin-skills");
    skills::sync_builtin_skills(&state, &bundle, "test").unwrap();
    let target = home
        .path()
        .join(".skill-studio/skills/skill-studio/SKILL.md");
    std::fs::write(
        &target,
        "---\nname: skill-studio\ndescription: 手改\n---\n\n我的内容\n",
    )
    .unwrap();

    let off = settings::update_settings(
        &state,
        SettingsPatch {
            cli_skill_enabled: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!off.cli_skill_enabled);
    assert!(off.cli_skill_warning.as_deref().unwrap().contains("修改"));
    assert!(target.is_file());
    let restarted = AppState::bootstrap(store.clone()).unwrap();
    assert!(settings::get_settings(&restarted)
        .unwrap()
        .cli_skill_warning
        .is_some());
    assert!(!std::fs::read_to_string(store.config_path())
        .unwrap()
        .contains("cliSkillWarning"));
    std::env::remove_var(paths::TEST_HOME_ENV);
}
