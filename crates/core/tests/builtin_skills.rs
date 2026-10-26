//! 内置 skill：装进 Hub、标为「Skill Studio 内置」、用户改过就不覆盖

#[path = "support.rs"]
mod support;

use std::fs;

use serial_test::serial;
use skill_studio_core::models::{group::Group, project::ProjectBinding};
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

#[test]
#[serial]
fn disabling_removes_only_an_unmodified_unused_builtin_and_reenabling_restores_it() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let source = bundle(&env, &skill_body("v1"));
    studio
        .sync_builtin_skills(&mut config, &source, "1.0")
        .unwrap();
    let target = env.hub().join("skill-studio");
    assert!(target.is_dir());

    studio
        .set_cli_skill_enabled(&mut config, false, None, "1.0")
        .unwrap();
    assert!(!config.settings.cli_skill_enabled);
    assert!(!target.exists());
    assert!(config.builtin_skills.is_empty());
    assert!(!studio.load_config().unwrap().settings.cli_skill_enabled);
    assert!(studio
        .sync_builtin_skills(&mut config, &source, "1.1")
        .unwrap()
        .is_empty());
    assert!(!target.exists(), "启动时不能把已关闭的 skill 又安装回来");

    studio
        .set_cli_skill_enabled(&mut config, true, Some(&source), "1.1")
        .unwrap();
    assert!(config.settings.cli_skill_enabled);
    assert!(target.join("SKILL.md").is_file());
    assert_eq!(
        config.builtin_skills.values().next().unwrap().version,
        "1.1"
    );
}

#[test]
#[serial]
fn disabling_preserves_modified_builtin_and_reports_why_it_remains() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let source = bundle(&env, &skill_body("v1"));
    studio
        .sync_builtin_skills(&mut config, &source, "1.0")
        .unwrap();
    let target = env.hub().join("skill-studio");
    fs::write(target.join("SKILL.md"), skill_body("my edits")).unwrap();

    studio
        .set_cli_skill_enabled(&mut config, false, None, "1.0")
        .unwrap();
    assert!(!config.settings.cli_skill_enabled);
    assert!(fs::read_to_string(target.join("SKILL.md"))
        .unwrap()
        .contains("my edits"));
    let notice = studio.builtin_retained(&config).unwrap();
    assert_eq!(notice.len(), 1);
    assert!(notice[0].reason.contains("修改"), "{notice:?}");
    assert_eq!(studio.load_config().unwrap().builtin_skills.len(), 1);

    // 重新开启也不能覆盖用户修改。
    fs::write(source.join("skill-studio/SKILL.md"), skill_body("v2")).unwrap();
    studio
        .set_cli_skill_enabled(&mut config, true, Some(&source), "2.0")
        .unwrap();
    assert!(fs::read_to_string(target.join("SKILL.md"))
        .unwrap()
        .contains("my edits"));
}

#[test]
#[serial]
fn disabling_preserves_saved_group_project_and_direct_agent_references() {
    for usage in ["group", "project", "registration"] {
        let env = Env::new();
        let studio = env.studio();
        let mut config = studio.load_config().unwrap();
        let source = bundle(&env, &skill_body("v1"));
        let installed = studio
            .sync_builtin_skills(&mut config, &source, "1.0")
            .unwrap();
        let id = installed[0].id.clone();
        match usage {
            "group" => {
                let mut group = Group::new("g1".into(), "saved".into());
                group.add_skill(id.clone());
                config.groups.push(group);
            }
            "project" => {
                let mut project =
                    ProjectBinding::new("p1".into(), "saved".into(), env.path().join("project"));
                project.skill_ids.push(id.clone());
                config.projects.push(project);
            }
            _ => {
                config.registrations.insert(id.clone(), Default::default());
            }
        }
        studio.save_config(&config).unwrap();
        studio
            .set_cli_skill_enabled(&mut config, false, None, "1.0")
            .unwrap();
        assert!(env.hub().join("skill-studio/SKILL.md").is_file(), "{usage}");
        let notice = studio.builtin_retained(&config).unwrap();
        assert_eq!(notice.len(), 1, "{usage}");
        assert!(notice[0].reason.contains("引用"), "{usage}: {notice:?}");
        if usage == "group" {
            // Once the user removes the last reference, the file is still on disk
            // until the next reconciliation. Do not make the warning disappear early.
            config.groups.clear();
            studio.save_config(&config).unwrap();
            let pending = studio.builtin_retained(&config).unwrap();
            assert_eq!(pending.len(), 1);
            assert!(pending[0].reason.contains("下次启动"), "{pending:?}");
        }
    }
}

#[test]
#[serial]
fn disabling_does_not_follow_a_replaced_builtin_directory_symlink() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let source = bundle(&env, &skill_body("v1"));
    studio
        .sync_builtin_skills(&mut config, &source, "1.0")
        .unwrap();
    let target = env.hub().join("skill-studio");
    let user = env.path().join("user-skill");
    fs::create_dir(&user).unwrap();
    fs::write(user.join("SKILL.md"), skill_body("user data")).unwrap();
    fs::remove_dir_all(&target).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&user, &target).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&user, &target).unwrap();

    studio
        .set_cli_skill_enabled(&mut config, false, None, "1.0")
        .unwrap();
    assert!(target.symlink_metadata().unwrap().file_type().is_symlink());
    assert!(user.join("SKILL.md").is_file());
    assert_eq!(studio.builtin_retained(&config).unwrap().len(), 1);
}

#[cfg(unix)]
#[test]
#[serial]
fn disabling_does_not_break_an_unregistered_agent_link_to_the_builtin() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let source = bundle(&env, &skill_body("v1"));
    studio
        .sync_builtin_skills(&mut config, &source, "1.0")
        .unwrap();
    let target = env.hub().join("skill-studio");
    fs::create_dir_all(env.claude_skills()).unwrap();
    let link = env.claude_skills().join("skill-studio");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    studio
        .set_cli_skill_enabled(&mut config, false, None, "1.0")
        .unwrap();
    assert!(target.join("SKILL.md").exists());
    assert!(studio.builtin_retained(&config).unwrap()[0]
        .reason
        .contains("链接"));
    fs::remove_file(link).unwrap();
    studio
        .sync_builtin_skills(&mut config, &source, "1.1")
        .unwrap();
    assert!(!target.exists(), "移走最后一个引用后下次同步可安全撤下");
}

#[test]
#[serial]
fn a_failed_config_save_rolls_back_builtin_removal_and_does_not_flip_the_switch() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let source = bundle(&env, &skill_body("v1"));
    studio
        .sync_builtin_skills(&mut config, &source, "1.0")
        .unwrap();
    let target = env.hub().join("skill-studio");
    let path = studio.store().config_path();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();

    assert!(studio
        .set_cli_skill_enabled(&mut config, false, None, "1.0")
        .is_err());
    assert!(config.settings.cli_skill_enabled);
    assert!(target.join("SKILL.md").exists(), "文件应被事务还原");
    assert_eq!(config.builtin_skills.len(), 1);
}

#[test]
#[serial]
fn reenabling_without_a_valid_bundle_keeps_the_switch_off() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let source = bundle(&env, &skill_body("v1"));
    studio
        .sync_builtin_skills(&mut config, &source, "1.0")
        .unwrap();
    studio
        .set_cli_skill_enabled(&mut config, false, None, "1.0")
        .unwrap();
    fs::remove_dir_all(source).unwrap();
    assert!(studio
        .set_cli_skill_enabled(&mut config, true, Some(&env.path().join("bundle")), "1.1")
        .is_err());
    assert!(!config.settings.cli_skill_enabled);
    assert!(!studio.load_config().unwrap().settings.cli_skill_enabled);
}
