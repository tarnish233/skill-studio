#[path = "support.rs"]
mod support;
use serial_test::serial;
use skill_studio_core::{models::config::AppConfig, services::studio::Studio};
use std::fs;
use support::Env;

fn make(env: &Env, studio: &Studio, config: &mut AppConfig, group: &str, names: &[&str]) -> String {
    for name in names {
        if !env.hub().join(name).exists() {
            env.write_simple_skill(&env.hub(), name);
        }
    }
    let all = studio.scan_skills(config).unwrap();
    let ids = names
        .iter()
        .map(|name| {
            all.iter()
                .find(|v| v.skill.name == *name)
                .unwrap()
                .skill
                .id
                .clone()
        })
        .collect();
    studio.save_group(config, group.into(), group, ids).unwrap();
    group.into()
}

#[test]
#[serial]
fn switching_preserves_manual_and_shared_members_and_survives_reload() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let manual = env.write_simple_skill(&env.codex_skills(), "manual");
    let before = fs::read(manual.join("SKILL.md")).unwrap();
    let a = make(&env, &studio, &mut c, "a", &["one", "shared"]);
    let b = make(&env, &studio, &mut c, "b", &["two", "shared"]);
    studio
        .activate_agent_group(&mut c, "codex", Some(&a))
        .unwrap();
    let stamp = fs::read(env.codex_skills().join("shared/.skill-studio-copy.json")).unwrap();
    studio
        .activate_agent_group(&mut c, "codex", Some(&b))
        .unwrap();
    assert!(!env.codex_skills().join("one").exists());
    assert!(env.codex_skills().join("two/SKILL.md").is_file());
    assert_eq!(
        stamp,
        fs::read(env.codex_skills().join("shared/.skill-studio-copy.json")).unwrap()
    );
    assert_eq!(before, fs::read(manual.join("SKILL.md")).unwrap());
    assert_eq!(
        studio.load_config().unwrap().active_groups["codex"].group_id,
        "b"
    );
    studio.activate_agent_group(&mut c, "codex", None).unwrap();
    assert!(!env.codex_skills().join("two").exists());
    assert!(!env.codex_skills().join("shared").exists());
    assert!(manual.is_dir());
    assert!(env.hub().join("one").is_dir());
}

#[test]
#[serial]
fn manual_member_is_never_claimed_and_shared_group_is_independent_per_agent() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    env.write_simple_skill(&env.codex_skills(), "manual");
    let manual_id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    studio
        .save_group(&mut c, "a".into(), "a", vec![manual_id])
        .unwrap();
    let b = make(&env, &studio, &mut c, "b", &["two"]);
    studio
        .activate_agent_group(&mut c, "codex", Some("a"))
        .unwrap();
    assert!(c.active_groups["codex"].entries.is_empty());
    // "b" isn't scoped to one agent: codex can switch to it, and claude-code can
    // run the very same group at the same time without either one disturbing it.
    studio
        .activate_agent_group(&mut c, "codex", Some(&b))
        .unwrap();
    studio
        .activate_agent_group(&mut c, "claude-code", Some(&b))
        .unwrap();
    assert_eq!(c.active_groups["codex"].group_id, b);
    assert_eq!(c.active_groups["claude-code"].group_id, b);
    assert!(env.codex_skills().join("two/SKILL.md").is_file());
    assert!(env.claude_skills().join("two/SKILL.md").is_file());
    studio.activate_agent_group(&mut c, "codex", None).unwrap();
    assert!(env.codex_skills().join("manual/SKILL.md").exists());
    assert!(!env.codex_skills().join("two").exists());
    // Stopping codex's copy must not touch claude-code's independent one.
    assert!(env.claude_skills().join("two/SKILL.md").exists());
    assert_eq!(c.active_groups["claude-code"].group_id, b);
}

#[test]
#[serial]
fn local_edits_block_switch_without_changing_files_or_active_group() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    make(&env, &studio, &mut c, "a", &["one"]);
    make(&env, &studio, &mut c, "b", &["two"]);
    studio
        .activate_agent_group(&mut c, "codex", Some("a"))
        .unwrap();
    fs::write(env.codex_skills().join("one/local.txt"), "keep me").unwrap();
    assert!(studio
        .activate_agent_group(&mut c, "codex", Some("b"))
        .is_err());
    assert_eq!(c.active_groups["codex"].group_id, "a");
    assert_eq!(
        fs::read_to_string(env.codex_skills().join("one/local.txt")).unwrap(),
        "keep me"
    );
    assert!(!env.codex_skills().join("two").exists());
}

#[test]
#[serial]
fn native_write_failure_rolls_back_removed_group_and_partial_new_copy() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    make(&env, &studio, &mut c, "a", &["one"]);
    make(&env, &studio, &mut c, "b", &["two"]);
    studio
        .activate_agent_group(&mut c, "codex", Some("a"))
        .unwrap();
    let config_before = fs::read(studio.store().config_path()).unwrap();
    let native = env.path().join(".codex/config.toml");
    fs::write(&native, "[invalid").unwrap();
    assert!(studio
        .activate_agent_group(&mut c, "codex", Some("b"))
        .is_err());
    assert!(env.codex_skills().join("one/SKILL.md").is_file());
    assert!(!env.codex_skills().join("two").exists());
    assert_eq!(fs::read_to_string(native).unwrap(), "[invalid");
    assert_eq!(
        fs::read(studio.store().config_path()).unwrap(),
        config_before
    );
    assert_eq!(c.active_groups["codex"].group_id, "a");
}

#[test]
#[serial]
fn edit_is_draft_until_apply_and_repeated_activation_is_idempotent() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    make(&env, &studio, &mut c, "a", &["one"]);
    studio
        .activate_agent_group(&mut c, "codex", Some("a"))
        .unwrap();
    make(&env, &studio, &mut c, "a", &["two"]);
    assert!(env.codex_skills().join("one").exists());
    assert!(!env.codex_skills().join("two").exists());
    studio
        .activate_agent_group(&mut c, "codex", Some("a"))
        .unwrap();
    let before = fs::read(env.codex_skills().join("two/.skill-studio-copy.json")).unwrap();
    studio
        .activate_agent_group(&mut c, "codex", Some("a"))
        .unwrap();
    assert_eq!(
        before,
        fs::read(env.codex_skills().join("two/.skill-studio-copy.json")).unwrap()
    );
    assert!(!env.codex_skills().join("one").exists());
}

#[test]
#[serial]
fn v1_shared_groups_and_project_references_survive_upgrade() {
    let env = Env::new();
    let studio = env.studio();
    let path = studio.store().config_path();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path,r#"{"version":1,"groups":[{"id":"old","name":"Old","skillIds":["s"]}],"projects":[{"id":"p","name":"P","root":"/tmp/project","groupIds":["old"]}]}"#).unwrap();
    let c = studio.load_config().unwrap();
    assert_eq!(c.version, skill_studio_core::models::config::CONFIG_VERSION);
    assert_eq!(c.groups[0].name, "Old");
    assert_eq!(c.projects[0].group_ids, vec!["old"]);
    assert!(c.active_groups.is_empty());
}

#[test]
#[serial]
fn invalid_member_leaves_previous_combination_active() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    make(&env, &studio, &mut c, "a", &["one"]);
    studio
        .activate_agent_group(&mut c, "codex", Some("a"))
        .unwrap();
    studio
        .save_group(&mut c, "b".into(), "b", vec!["missing".into()])
        .unwrap();
    assert!(studio
        .activate_agent_group(&mut c, "codex", Some("b"))
        .is_err());
    assert_eq!(c.active_groups["codex"].group_id, "a");
    assert!(env.codex_skills().join("one").exists());
}

#[test]
#[serial]
fn claude_uses_directory_name_and_does_not_change_unrelated_native_rules() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    env.write_skill(
        &env.hub(),
        "folder",
        "---\nname: declared\ndescription: test\n---\nbody",
    );
    make(&env, &studio, &mut c, "a", &["folder"]);
    fs::create_dir_all(env.path().join(".claude")).unwrap();
    fs::write(
        env.path().join(".claude/settings.json"),
        r#"{"skillOverrides":{"folder":"off","unrelated":"off"},"other":42}"#,
    )
    .unwrap();
    studio
        .activate_agent_group(&mut c, "claude-code", Some("a"))
        .unwrap();
    let views = studio.scan_skills(&c).unwrap();
    assert!(!views[0].agents["claude-code"].disabled);
    let v: serde_json::Value =
        serde_json::from_slice(&fs::read(env.path().join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(v["skillOverrides"]["unrelated"], "off");
    assert_eq!(v["other"], 42);
}

#[test]
#[serial]
fn project_applies_a_shared_group_to_every_selected_agent() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    make(&env, &studio, &mut c, "a", &["one"]);
    let mut p = skill_studio_core::models::project::ProjectBinding::new(
        "p".into(),
        "P".into(),
        env.path().join("project"),
    );
    p.agent_ids = vec!["codex".into(), "claude-code".into()];
    p.group_ids = vec!["a".into()];
    fs::create_dir_all(&p.root).unwrap();
    c.projects.push(p);
    let r = studio.apply_project(&mut c, "p").unwrap();
    assert!(r.failed.is_empty());
    let mut agents: Vec<_> = r.success.iter().map(|x| x.agent_id.clone()).collect();
    agents.sort();
    assert_eq!(agents, vec!["claude-code".to_string(), "codex".to_string()]);
}

#[test]
#[serial]
fn active_skill_cannot_be_adopted_until_group_stops() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    env.write_simple_skill(&env.claude_skills(), "manual");
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    studio
        .save_group(&mut c, "a".into(), "a", vec![id.clone()])
        .unwrap();
    studio
        .activate_agent_group(&mut c, "codex", Some("a"))
        .unwrap();
    assert!(studio.adopt_to_hub(&mut c, &id).is_err());
    studio.activate_agent_group(&mut c, "codex", None).unwrap();
    studio.adopt_to_hub(&mut c, &id).unwrap();
}

#[test]
#[serial]
fn startup_recovers_interrupted_group_switch_journal() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    make(&env, &studio, &mut c, "a", &["one"]);
    studio
        .activate_agent_group(&mut c, "codex", Some("a"))
        .unwrap();
    let target = env.codex_skills().join("one");
    let backup = env.codex_skills().join(".one.test-backup");
    fs::rename(&target, &backup).unwrap();
    fs::write(studio.store().dir().join("group-switch.json"),serde_json::to_vec(&serde_json::json!({"journal":studio.store().dir().join("group-switch.json"),"entries":[{"target":target,"backup":backup,"existed":true}],"committed":false})).unwrap()).unwrap();
    let loaded = studio.load_config().unwrap();
    assert_eq!(loaded.active_groups["codex"].group_id, "a");
    assert!(target.join("SKILL.md").is_file());
    assert!(!backup.exists());
}

#[test]
#[serial]
fn disabled_manual_skill_is_not_enabled_or_claimed_silently() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    env.write_simple_skill(&env.codex_skills(), "manual");
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    studio.set_skill_enabled(&c, &id, "codex", false).unwrap();
    let before = fs::read(env.path().join(".codex/config.toml")).unwrap();
    studio
        .save_group(&mut c, "a".into(), "a", vec![id])
        .unwrap();
    assert!(studio
        .activate_agent_group(&mut c, "codex", Some("a"))
        .is_err());
    assert!(c.active_groups.is_empty());
    assert_eq!(
        before,
        fs::read(env.path().join(".codex/config.toml")).unwrap()
    );
}

#[test]
#[serial]
fn exclusive_groups_suspend_manual_skills_and_restore_after_reload() {
    for agent_id in ["codex", "claude-code"] {
        let env = Env::new();
        let studio = env.studio();
        let mut c = studio.load_config().unwrap();
        assert!(c.settings.preserve_manual_skills);
        c.settings.preserve_manual_skills = false;
        let root = if agent_id == "codex" {
            env.codex_skills()
        } else {
            env.claude_skills()
        };
        let manual = env.write_simple_skill(&root, "manual");
        env.write_simple_skill(&root, "already-off");
        let views = studio.scan_skills(&c).unwrap();
        let manual_id = views
            .iter()
            .find(|s| s.skill.name == "manual")
            .unwrap()
            .skill
            .id
            .clone();
        let off_id = views
            .iter()
            .find(|s| s.skill.name == "already-off")
            .unwrap()
            .skill
            .id
            .clone();
        studio
            .set_skill_enabled(&c, &off_id, agent_id, false)
            .unwrap();
        let before = fs::read(manual.join("SKILL.md")).unwrap();
        make(&env, &studio, &mut c, "a", &["one"]);
        studio
            .save_group(&mut c, "b".into(), "b", vec![manual_id.clone()])
            .unwrap();
        studio
            .activate_agent_group(&mut c, agent_id, Some("a"))
            .unwrap();
        assert!(
            studio
                .scan_skills(&c)
                .unwrap()
                .iter()
                .find(|v| v.skill.id == manual_id)
                .unwrap()
                .agents[agent_id]
                .disabled
        );
        assert_eq!(c.active_groups[agent_id].suspended_manual.len(), 1);
        let mut c = studio.load_config().unwrap();
        // Choosing a suspended manual skill as a member restores it without copying or claiming it.
        studio
            .activate_agent_group(&mut c, agent_id, Some("b"))
            .unwrap();
        assert!(
            !studio
                .scan_skills(&c)
                .unwrap()
                .iter()
                .find(|v| v.skill.id == manual_id)
                .unwrap()
                .agents[agent_id]
                .disabled
        );
        assert!(c.active_groups[agent_id].entries.is_empty());
        studio
            .activate_agent_group(&mut c, agent_id, Some("a"))
            .unwrap();
        studio.activate_agent_group(&mut c, agent_id, None).unwrap();
        let views = studio.scan_skills(&c).unwrap();
        assert!(
            views
                .iter()
                .find(|v| v.skill.id == manual_id)
                .unwrap()
                .agents[agent_id]
                .disabled
        );
        assert!(views.iter().find(|v| v.skill.id == off_id).unwrap().agents[agent_id].disabled);
        assert_eq!(before, fs::read(manual.join("SKILL.md")).unwrap());
        // Reapplying after changing the setting also restores manual skills.
        studio
            .activate_agent_group(&mut c, agent_id, Some("a"))
            .unwrap();
        c.settings.preserve_manual_skills = true;
        studio
            .activate_agent_group(&mut c, agent_id, Some("a"))
            .unwrap();
        assert!(
            !studio
                .scan_skills(&c)
                .unwrap()
                .iter()
                .find(|v| v.skill.id == manual_id)
                .unwrap()
                .agents[agent_id]
                .disabled
        );
        assert!(c.active_groups[agent_id].suspended_manual.is_empty());
    }
}

#[test]
#[serial]
fn exclusive_switch_failure_keeps_manual_state_and_active_group() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let manual = env.write_simple_skill(&env.codex_skills(), "manual");
    make(&env, &studio, &mut c, "a", &["one"]);
    make(&env, &studio, &mut c, "b", &["two"]);
    studio
        .activate_agent_group(&mut c, "codex", Some("a"))
        .unwrap();
    c.settings.preserve_manual_skills = false;
    let native = env.path().join(".codex/config.toml");
    fs::write(&native, "[invalid").unwrap();
    let before = fs::read(manual.join("SKILL.md")).unwrap();
    assert!(studio
        .activate_agent_group(&mut c, "codex", Some("b"))
        .is_err());
    assert_eq!(c.active_groups["codex"].group_id, "a");
    assert!(c.active_groups["codex"].suspended_manual.is_empty());
    assert_eq!(fs::read_to_string(native).unwrap(), "[invalid");
    assert_eq!(before, fs::read(manual.join("SKILL.md")).unwrap());
    assert!(env.codex_skills().join("one/SKILL.md").is_file());
    assert!(!env.codex_skills().join("two").exists());
}

#[test]
#[serial]
fn reapply_refreshes_stale_owned_copy_without_changing_manual_skills() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let g = make(&env, &studio, &mut c, "dev", &["demo"]);
    studio
        .activate_agent_group(&mut c, "codex", Some(&g))
        .unwrap();
    fs::write(env.hub().join("demo/new.txt"), "new content").unwrap();
    studio
        .activate_agent_group(&mut c, "codex", Some(&g))
        .unwrap();
    assert_eq!(
        fs::read_to_string(env.codex_skills().join("demo/new.txt")).unwrap(),
        "new content"
    );
    studio.activate_agent_group(&mut c, "codex", None).unwrap();
    assert!(!env.codex_skills().join("demo").exists());
}

#[test]
#[serial]
fn leaving_management_restores_manual_and_adopted_content_and_keeps_hub() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let manual = env.write_simple_skill(&env.codex_skills(), "manual");
    let bytes = fs::read(manual.join("SKILL.md")).unwrap();
    let source = studio
        .scan_skills(&c)
        .unwrap()
        .into_iter()
        .find(|v| v.skill.name == "manual")
        .unwrap()
        .skill;
    let hub = studio.adopt_to_hub(&mut c, &source.id).unwrap();
    c.settings.preserve_manual_skills = false;
    let group = make(&env, &studio, &mut c, "g", &["group-only"]);
    studio
        .activate_agent_group(&mut c, "codex", Some(&group))
        .unwrap();
    studio.set_agent_management(&mut c, "codex", false).unwrap();
    assert!(!skill_studio_core::services::scanner::is_symlink_or_junction(&manual));
    assert_eq!(fs::read(manual.join("SKILL.md")).unwrap(), bytes);
    assert!(hub.source_path.join("SKILL.md").exists());
    assert!(!env.codex_skills().join("group-only").exists());
    assert!(!c.active_groups.contains_key("codex"));
    assert!(studio
        .load_config()
        .unwrap()
        .settings
        .disabled_agents
        .contains(&"codex".into()));
    let views = studio.scan_skills(&c).unwrap();
    assert!(views
        .iter()
        .filter(|v| v.skill.name == "manual")
        .any(|v| !v.agents["codex"].disabled));
    assert!(studio
        .activate_agent_group(&mut c, "codex", Some(&group))
        .is_err());
    studio.set_agent_management(&mut c, "codex", true).unwrap();
    assert!(!c.active_groups.contains_key("codex"));
}

#[test]
#[serial]
fn leaving_management_conflict_keeps_management_and_files_unchanged() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let group = make(&env, &studio, &mut c, "g", &["modified"]);
    studio
        .activate_agent_group(&mut c, "codex", Some(&group))
        .unwrap();
    let dest = env.codex_skills().join("modified/SKILL.md");
    fs::write(&dest, "local changes").unwrap();
    let before = fs::read(studio.store().config_path()).unwrap();
    assert!(studio.set_agent_management(&mut c, "codex", false).is_err());
    assert!(c.settings.disabled_agents.is_empty());
    assert!(c.active_groups.contains_key("codex"));
    assert_eq!(fs::read(studio.store().config_path()).unwrap(), before);
    assert_eq!(fs::read_to_string(dest).unwrap(), "local changes");
}

#[test]
#[serial]
fn leaving_management_keeps_other_agent_group_and_rejects_missing_backup() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let manual = env.write_simple_skill(&env.codex_skills(), "adopted");
    let original = studio
        .scan_skills(&c)
        .unwrap()
        .into_iter()
        .find(|v| v.skill.name == "adopted")
        .unwrap()
        .skill;
    let hub = studio.adopt_to_hub(&mut c, &original.id).unwrap();
    let other = make(&env, &studio, &mut c, "other", &["other-only"]);
    studio
        .activate_agent_group(&mut c, "claude-code", Some(&other))
        .unwrap();
    let backup = c.skill_provenance[&hub.id]
        .backup_path
        .join("content/SKILL.md");
    let bytes = fs::read(&backup).unwrap();
    fs::write(&backup, "corrupt").unwrap();
    assert!(studio.set_agent_management(&mut c, "codex", false).is_err());
    assert!(c.settings.disabled_agents.is_empty());
    assert!(
        skill_studio_core::services::linker::link_status(&hub.source_path, &manual).is_registered()
    );
    fs::write(&backup, bytes).unwrap();
    studio.set_agent_management(&mut c, "codex", false).unwrap();
    assert!(c.active_groups.contains_key("claude-code"));
    assert!(env.claude_skills().join("other-only/SKILL.md").exists());
    assert!(hub.source_path.join("SKILL.md").exists());
    assert!(studio
        .register(
            &mut c,
            std::slice::from_ref(&hub.id),
            &["codex".into()],
            None,
            false
        )
        .is_err());
}

#[test]
#[serial]
#[cfg(unix)]
fn restored_shared_skill_keeps_one_identity_and_group_policy_after_reenable() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let original = env.write_simple_skill(&env.agents_skills(), "shared-manual");
    fs::create_dir_all(env.codex_skills()).unwrap();
    let alias = env.codex_skills().join("shared-manual");
    std::os::unix::fs::symlink(&original, &alias).unwrap();
    let source = studio.scan_skills(&c).unwrap().remove(0).skill;
    let hub = studio.adopt_to_hub(&mut c, &source.id).unwrap();
    let group = studio
        .save_group(&mut c, "restored".into(), "restored", vec![hub.id.clone()])
        .unwrap();
    c.settings.preserve_manual_skills = false;
    studio.reconcile_manual_policy(&mut c, true).unwrap();
    for _ in 0..2 {
        studio.set_agent_management(&mut c, "codex", false).unwrap();
        studio.set_agent_management(&mut c, "codex", true).unwrap();
        c = studio.load_config().unwrap();
        let views = studio.scan_skills(&c).unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].skill.id, hub.id);
        assert!(views[0].agents["codex"].manual);
        assert!(views[0].agents["codex"].disabled);
        assert_eq!(views[0].agents["codex"].entry_paths.len(), 2);
        studio
            .activate_agent_group(&mut c, "codex", Some(&group.id))
            .unwrap();
        assert!(!studio.scan_skills(&c).unwrap()[0].agents["codex"].disabled);
        studio.activate_agent_group(&mut c, "codex", None).unwrap();
        assert!(studio.scan_skills(&c).unwrap()[0].agents["codex"].disabled);
    }
    c.settings.preserve_manual_skills = true;
    studio.reconcile_manual_policy(&mut c, true).unwrap();
    assert!(!studio.scan_skills(&c).unwrap()[0].agents["codex"].disabled);
    fs::write(
        alias.join("SKILL.md"),
        "---\nname: shared-manual\n---\nChanged",
    )
    .unwrap();
    assert_eq!(
        studio.scan_skills(&c).unwrap().len(),
        2,
        "modified originals must stay independent"
    );
}

/// 分组切换把 registrations/active_groups 整片重写，是最该能回退的操作之一，
/// 而它走的是"先把 config.json 纳入回滚日志、再写新内容"这条路径 —— 顺序错了的话
/// `backups/` 里什么都不会多出来，用户在「维护」页找不回切换前的配置。
#[test]
#[serial]
fn activating_a_group_leaves_a_restorable_config_backup() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    make(&env, &studio, &mut c, "a", &["one"]);
    studio.save_config(&c).unwrap();
    let before = studio.store().list_backups().len();

    studio
        .activate_agent_group(&mut c, "codex", Some("a"))
        .unwrap();

    let backups = studio.store().list_backups();
    assert!(
        backups.len() > before,
        "分组切换应留下备份，切换前 {before} 份，切换后 {} 份",
        backups.len()
    );
    // 备份得是能读回来的完整配置，不是空壳
    let restored = studio
        .store()
        .restore_backup(backups.last().unwrap())
        .unwrap();
    assert!(restored.groups.iter().any(|g| g.id == "a"));
}

/// 新分组排在当前最后一个之后：删掉前面的分组后 `len()` 会和已有的序号撞车。
#[test]
#[serial]
fn new_group_is_appended_after_deletions() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    for id in ["a", "b", "c"] {
        studio.save_group(&mut c, id.into(), id, vec![]).unwrap();
    }
    c.groups.retain(|g| g.id == "c");
    let d = studio.save_group(&mut c, "d".into(), "d", vec![]).unwrap();
    let last = c.groups.iter().find(|g| g.id == "c").unwrap().sort_order;
    assert!(d.sort_order > last, "c={last} d={}", d.sort_order);
}
