//! 内置 skill：装进 Hub、标为「Skill Studio 内置」、用户改过就不覆盖

#[path = "support.rs"]
mod support;

use std::fs;

use serial_test::serial;
use support::Env;

fn bundle(env: &Env, body: &str) -> std::path::PathBuf {
    let dir = env.path().join("bundle");
    env.write_skill(&dir, "skill-studio", body);
    dir
}

fn skill_body(text: &str) -> String {
    format!("---\nname: skill-studio\ndescription: 内置测试\n---\n\n{text}\n")
}

#[test]
#[serial]
fn a_builtin_skill_is_installed_marked_and_updated_only_while_unedited() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let source = bundle(&env, &skill_body("v1"));

    let installed = studio
        .sync_builtin_skills(&mut config, &source, "1.0.0")
        .unwrap();
    assert_eq!(installed.len(), 1);
    let target = env.hub().join("skill-studio");
    assert!(target.join("SKILL.md").is_file());
    let id = installed[0].id.clone();
    assert_eq!(config.builtin_skills[&id].version, "1.0.0");

    // 装进 Hub 的内置 skill 标记为 builtin，而不是「来源待确认」
    let view = studio
        .scan_skills(&config)
        .unwrap()
        .into_iter()
        .find(|view| view.skill.id == id)
        .unwrap();
    assert_eq!(view.source_ids, vec!["builtin".to_string()]);

    // 内容没变、只是应用升级：只更新记录，不重写文件
    let again = studio
        .sync_builtin_skills(&mut config, &source, "1.1.0")
        .unwrap();
    assert!(again.is_empty());
    assert_eq!(config.builtin_skills[&id].version, "1.1.0");

    // 资源更新了、用户没改：更新
    fs::write(source.join("skill-studio/SKILL.md"), skill_body("v2")).unwrap();
    let updated = studio
        .sync_builtin_skills(&mut config, &source, "1.2.0")
        .unwrap();
    assert_eq!(updated.len(), 1);
    assert!(fs::read_to_string(target.join("SKILL.md"))
        .unwrap()
        .contains("v2"));

    // 用户改过：保留他的版本，不覆盖
    fs::write(target.join("SKILL.md"), skill_body("mine")).unwrap();
    fs::write(source.join("skill-studio/SKILL.md"), skill_body("v3")).unwrap();
    let kept = studio
        .sync_builtin_skills(&mut config, &source, "1.3.0")
        .unwrap();
    assert!(kept.is_empty());
    assert!(fs::read_to_string(target.join("SKILL.md"))
        .unwrap()
        .contains("mine"));
}

#[test]
#[serial]
fn a_user_skill_with_the_same_name_is_never_taken_over() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let mine = env.write_simple_skill(&env.hub(), "skill-studio");
    fs::write(mine.join("SKILL.md"), skill_body("mine")).unwrap();
    let source = bundle(&env, &skill_body("builtin"));

    let installed = studio
        .sync_builtin_skills(&mut config, &source, "1.0.0")
        .unwrap();
    assert!(installed.is_empty());
    assert!(fs::read_to_string(mine.join("SKILL.md"))
        .unwrap()
        .contains("mine"));
    assert!(config.builtin_skills.is_empty());
    assert!(env.hub().join("skill-studio").is_dir());
}
