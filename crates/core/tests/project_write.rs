#[path = "support.rs"]
mod support;
use serial_test::serial;
use skill_studio_core::{
    models::{
        project::{ProjectBinding, ProjectSelection},
        skill::LinkMode,
    },
    services::scanner,
};
use std::fs;
use support::Env;
fn selection(ids: Vec<String>) -> ProjectSelection {
    ProjectSelection {
        agent_ids: vec!["codex".into()],
        skill_ids: ids,
        group_ids: vec![],
        link_mode: LinkMode::Copy,
    }
}

#[test]
#[serial]
fn selection_and_files_commit_together_and_deselection_removes_only_owned_copies() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let root = env.path().join("project");
    fs::create_dir(&root).unwrap();
    c.projects
        .push(ProjectBinding::new("p".into(), "P".into(), root.clone()));
    studio.save_config(&c).unwrap();
    env.write_simple_skill(&env.hub(), "demo");
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    let manual = env.write_simple_skill(&root.join(".agents/skills"), "manual");
    studio
        .write_project(&mut c, "p", Some(selection(vec![id.clone()])))
        .unwrap();
    assert_eq!(
        studio.load_config().unwrap().projects[0].skill_ids,
        vec![id]
    );
    assert!(root.join(".agents/skills/demo/SKILL.md").is_file());
    assert_eq!(c.projects[0].managed_entries.len(), 1);
    studio
        .write_project(&mut c, "p", Some(selection(vec![])))
        .unwrap();
    assert!(!root.join(".agents/skills/demo").exists());
    assert!(manual.exists());
    assert!(studio.load_config().unwrap().projects[0]
        .skill_ids
        .is_empty());
}

#[test]
#[serial]
fn conflict_does_not_save_selection_or_touch_existing_files() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let root = env.path().join("project");
    fs::create_dir(&root).unwrap();
    c.projects
        .push(ProjectBinding::new("p".into(), "P".into(), root.clone()));
    studio.save_config(&c).unwrap();
    env.write_simple_skill(&env.hub(), "demo");
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    let foreign = env.write_simple_skill(&root.join(".agents/skills"), "demo");
    let hash = scanner::dir_content_hash(&foreign).unwrap();
    let before = fs::read(studio.store().config_path()).unwrap();
    assert!(studio
        .write_project(&mut c, "p", Some(selection(vec![id])))
        .is_err());
    assert!(c.projects[0].skill_ids.is_empty());
    assert_eq!(before, fs::read(studio.store().config_path()).unwrap());
    assert_eq!(hash, scanner::dir_content_hash(&foreign).unwrap());
}

#[test]
#[serial]
fn a_modified_owned_copy_is_left_in_place_on_deselection_and_refresh() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let root = env.path().join("project");
    fs::create_dir(&root).unwrap();
    c.projects
        .push(ProjectBinding::new("p".into(), "P".into(), root.clone()));
    let source = env.write_simple_skill(&env.hub(), "demo");
    env.write_simple_skill(&env.hub(), "other");
    let views = studio.scan_skills(&c).unwrap();
    let id = |name: &str| {
        views
            .iter()
            .find(|v| v.skill.name == name)
            .unwrap()
            .skill
            .id
            .clone()
    };
    studio
        .write_project(&mut c, "p", Some(selection(vec![id("demo")])))
        .unwrap();
    let dest = root.join(".agents/skills/demo/SKILL.md");
    fs::write(&dest, "local edits").unwrap();
    // 源也变了、又加了新成员：被改过的副本不覆盖，仍归项目管理；其余照常写入
    fs::write(source.join("SKILL.md"), "source edits").unwrap();
    let report = studio
        .write_project(&mut c, "p", Some(selection(vec![id("demo"), id("other")])))
        .unwrap();
    assert_eq!(fs::read_to_string(&dest).unwrap(), "local edits");
    assert!(root.join(".agents/skills/other/SKILL.md").is_file());
    assert_eq!(
        report
            .untouched
            .iter()
            .map(|u| (u.name.as_str(), u.reason.as_str(), u.tracked))
            .collect::<Vec<_>>(),
        [("demo", "副本已被修改", true)]
    );
    assert_eq!(c.projects[0].managed_entries.len(), 2);
    // 取消选择：不删，交还给用户，不再记为项目的部署
    let report = studio
        .write_project(&mut c, "p", Some(selection(vec![id("other")])))
        .unwrap();
    assert_eq!(fs::read_to_string(&dest).unwrap(), "local edits");
    assert_eq!(
        report
            .untouched
            .iter()
            .map(|u| (u.name.as_str(), u.tracked))
            .collect::<Vec<_>>(),
        [("demo", false)]
    );
    assert_eq!(
        c.projects[0]
            .managed_entries
            .iter()
            .map(|e| e.skill_id.clone())
            .collect::<Vec<_>>(),
        [id("other")]
    );
}

#[test]
#[serial]
fn interrupted_project_write_restores_configuration_and_deployments() {
    let env = Env::new();
    let studio = env.studio();
    let c = studio.load_config().unwrap();
    studio.save_config(&c).unwrap();
    let before = fs::read(studio.store().config_path()).unwrap();
    let dest = env.path().join("project/skill");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("SKILL.md"), "old").unwrap();
    let backup = dest.with_extension("backup");
    fs::rename(&dest, &backup).unwrap();
    fs::create_dir(&dest).unwrap();
    fs::write(dest.join("SKILL.md"), "new").unwrap();
    let config_path = studio.store().config_path();
    let config_backup = config_path.with_extension("backup");
    fs::rename(&config_path, &config_backup).unwrap();
    fs::write(&config_path, "bad").unwrap();
    let journal = studio.store().dir().join("project-write.json");
    fs::write(
        &journal,
        serde_json::to_vec(&serde_json::json!({
            "journal": journal, "committed": false,
            "entries": [
                {"target": dest, "backup": backup, "existed": true},
                {"target": config_path, "backup": config_backup, "existed": true}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    studio.load_config().unwrap();
    assert_eq!(fs::read(studio.store().config_path()).unwrap(), before);
    assert_eq!(fs::read_to_string(dest.join("SKILL.md")).unwrap(), "old");
}

#[test]
#[serial]
fn project_toggle_preserves_selection_and_manual_files_and_leaves_modified_deployments() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let root = env.path().join("project");
    fs::create_dir(&root).unwrap();
    c.projects
        .push(ProjectBinding::new("p".into(), "P".into(), root.clone()));
    env.write_simple_skill(&env.hub(), "demo");
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    let manual = env.write_simple_skill(&root.join(".agents/skills"), "manual");
    let mut selected = selection(vec![id.clone()]);
    selected.agent_ids.push("claude-code".into());
    studio.write_project(&mut c, "p", Some(selected)).unwrap();
    let saved_agents = c.projects[0].agent_ids.clone();
    for _ in 0..2 {
        studio.set_project_enabled(&mut c, "p", false).unwrap();
        c = studio.load_config().unwrap();
        assert!(c.projects[0].managed_entries.is_empty());
        assert_eq!(c.projects[0].skill_ids, vec![id.clone()]);
        assert_eq!(c.projects[0].agent_ids, saved_agents);
        assert!(manual.join("SKILL.md").exists());
        assert!(!root.join(".agents/skills/demo").exists());
        assert!(!root.join(".claude/skills/demo").exists());
        studio.set_project_enabled(&mut c, "p", true).unwrap();
        assert_eq!(c.projects[0].managed_entries.len(), 2);
    }
    let modified = root.join(".claude/skills/demo/SKILL.md");
    fs::write(&modified, "user changes").unwrap();
    // 被改过的副本不再拦住停用：原样留下并报告，其余照常撤回
    let report = studio.set_project_enabled(&mut c, "p", false).unwrap();
    assert_eq!(fs::read_to_string(modified).unwrap(), "user changes");
    assert!(!root.join(".agents/skills/demo").exists());
    assert!(c.projects[0].managed_entries.is_empty());
    assert_eq!(
        report
            .untouched
            .iter()
            .map(|u| (u.reason.as_str(), u.tracked))
            .collect::<Vec<_>>(),
        [("副本已被修改", false)]
    );
}

#[test]
#[serial]
fn deployments_per_agent_separate_stale_copies_from_ones_that_need_checking() {
    use skill_studio_core::services::project_write::project_deployments;
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let root = env.path().join("project");
    fs::create_dir(&root).unwrap();
    c.projects
        .push(ProjectBinding::new("p".into(), "P".into(), root.clone()));
    let source = env.write_simple_skill(&env.hub(), "demo");
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    let mut selected = selection(vec![id.clone()]);
    selected.agent_ids.push("claude-code".into());
    studio.write_project(&mut c, "p", Some(selected)).unwrap();
    let view = |c: &skill_studio_core::models::config::AppConfig| {
        let mut list = project_deployments(&c.projects[0]);
        list.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
        list.into_iter()
            .map(|d| (d.agent_id, d.skill_ids, d.stale, d.issue))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        view(&c),
        [
            ("claude-code".into(), vec![id.clone()], false, None),
            ("codex".into(), vec![id.clone()], false, None),
        ]
    );
    // 源更新了：两边的副本都过期，需要重新写入
    fs::write(source.join("SKILL.md"), "---\nname: demo\n---\nnewer").unwrap();
    assert!(view(&c).iter().all(|d| d.2 && d.3.is_none()));
    // Claude 的副本被本地改过：需要检查，不算过期
    fs::write(root.join(".claude/skills/demo/SKILL.md"), "local").unwrap();
    let list = view(&c);
    assert_eq!(list[0].3.as_deref(), Some("demo 的副本已被修改"));
    assert!(list[1].2 && list[1].3.is_none());
    // 副本被删掉
    fs::remove_dir_all(root.join(".agents/skills/demo")).unwrap();
    assert_eq!(view(&c)[1].3.as_deref(), Some("demo 已不存在"));
    // 没有部署的 Agent 不出现
    studio.set_project_enabled(&mut c, "p", false).unwrap();
    assert!(project_deployments(&c.projects[0]).is_empty());
}
