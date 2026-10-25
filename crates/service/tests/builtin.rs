//! 随应用打包的内置 skill：真的能装进 Hub、能扫描、能再次同步

use serial_test::serial;
use skill_studio_core::fs::paths;
use skill_studio_core::services::store::Store;
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
