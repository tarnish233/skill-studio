#[path = "support.rs"]
mod support;
use serial_test::serial;
use std::fs;
use support::Env;
#[test]
#[serial]
fn policy_without_groups_restores_only_its_own_suspensions_and_finds_new_skills() {
    for agent in ["claude-code", "codex"] {
        let env = Env::new();
        let studio = env.studio();
        let mut c = studio.load_config().unwrap();
        let root = if agent == "codex" {
            env.codex_skills()
        } else {
            env.claude_skills()
        };
        env.write_simple_skill(&root, "manual");
        env.write_simple_skill(&root, "already-off");
        let off = studio
            .scan_skills(&c)
            .unwrap()
            .into_iter()
            .find(|v| v.skill.name == "already-off")
            .unwrap()
            .skill
            .id;
        studio.set_skill_enabled(&c, &off, agent, false).unwrap();
        c.settings.preserve_manual_skills = false;
        studio.reconcile_manual_policy(&mut c, true).unwrap();
        assert!(studio
            .scan_skills(&c)
            .unwrap()
            .iter()
            .all(|v| v.agents[agent].manual && v.agents[agent].disabled));
        let mut c = studio.load_config().unwrap();
        env.write_simple_skill(&root, "new-manual");
        studio.reconcile_manual_policy(&mut c, false).unwrap();
        assert!(studio
            .scan_skills(&c)
            .unwrap()
            .iter()
            .all(|v| v.agents[agent].disabled));
        c.settings.preserve_manual_skills = true;
        studio.reconcile_manual_policy(&mut c, true).unwrap();
        for v in studio.scan_skills(&c).unwrap() {
            assert_eq!(v.agents[agent].disabled, v.skill.id == off);
        }
        assert!(c.policy_suspensions.is_empty());
        c.settings.preserve_manual_skills = false;
        studio.reconcile_manual_policy(&mut c, true).unwrap();
        studio.set_agent_management(&mut c, agent, false).unwrap();
        for v in studio.scan_skills(&c).unwrap() {
            assert_eq!(v.agents[agent].disabled, v.skill.id == off);
        }
        studio.reconcile_manual_policy(&mut c, false).unwrap();
        assert!(studio
            .scan_skills(&c)
            .unwrap()
            .iter()
            .any(|v| !v.agents[agent].disabled));
    }
}
#[test]
#[serial]
fn cross_agent_policy_failure_rolls_back_native_files_and_config() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    env.write_simple_skill(&env.claude_skills(), "a");
    env.write_simple_skill(&env.codex_skills(), "b");
    studio.save_config(&c).unwrap();
    let before = fs::read(studio.store().config_path()).unwrap();
    let native = env.path().join(".claude/settings.json");
    fs::write(&native, "{\"other\":42}").unwrap();
    fs::write(env.path().join(".codex/config.toml"), "[broken").unwrap();
    c.settings.preserve_manual_skills = false;
    assert!(studio.reconcile_manual_policy(&mut c, true).is_err());
    assert_eq!(fs::read_to_string(native).unwrap(), "{\"other\":42}");
    assert_eq!(fs::read(studio.store().config_path()).unwrap(), before);
    assert!(c.policy_suspensions.is_empty());
}
#[test]
#[serial]
fn adopted_manual_remains_manual_but_studio_registration_is_not() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    env.write_simple_skill(&env.claude_skills(), "manual");
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    studio.adopt_to_hub(&mut c, &id).unwrap();
    env.write_simple_skill(&env.hub(), "studio");
    let id = studio
        .scan_skills(&c)
        .unwrap()
        .into_iter()
        .find(|v| v.skill.name == "studio")
        .unwrap()
        .skill
        .id;
    studio
        .register(&mut c, &[id], &["claude-code".into()], None, false)
        .unwrap();
    for v in studio.scan_skills(&c).unwrap() {
        assert_eq!(v.agents["claude-code"].manual, v.skill.name == "manual");
    }
}

#[test]
#[serial]
fn changing_policy_immediately_respects_active_group_and_stop_does_not_restore_outsiders() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    env.write_simple_skill(&env.claude_skills(), "member");
    env.write_simple_skill(&env.claude_skills(), "outside");
    let member = studio
        .scan_skills(&c)
        .unwrap()
        .into_iter()
        .find(|v| v.skill.name == "member")
        .unwrap()
        .skill
        .id;
    studio
        .save_group(&mut c, "g".into(), "g", vec![member])
        .unwrap();
    studio
        .activate_agent_groups(&mut c, "claude-code", &["g".to_string()])
        .unwrap();
    c.settings.preserve_manual_skills = false;
    studio.reconcile_manual_policy(&mut c, true).unwrap();
    for view in studio.scan_skills(&c).unwrap() {
        assert_eq!(
            view.agents["claude-code"].disabled,
            view.skill.name == "outside"
        );
        assert!(view.agents["claude-code"].manual);
    }
    studio
        .activate_agent_groups(&mut c, "claude-code", &[])
        .unwrap();
    assert!(studio
        .scan_skills(&c)
        .unwrap()
        .iter()
        .all(|v| v.agents["claude-code"].disabled));
    c.settings.preserve_manual_skills = true;
    studio.reconcile_manual_policy(&mut c, true).unwrap();
    assert!(studio
        .scan_skills(&c)
        .unwrap()
        .iter()
        .all(|v| !v.agents["claude-code"].disabled));
}

#[test]
#[cfg(unix)]
#[serial]
fn native_config_permissions_survive_policy_reconcile() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    env.write_simple_skill(&env.claude_skills(), "manual");
    let native = env.path().join(".claude/settings.json");
    fs::write(&native, "{\"env\":{\"SECRET\":\"test\"}}").unwrap();
    fs::set_permissions(&native, fs::Permissions::from_mode(0o600)).unwrap();
    c.settings.preserve_manual_skills = false;
    studio.reconcile_manual_policy(&mut c, true).unwrap();
    assert_eq!(
        fs::metadata(&native).unwrap().permissions().mode() & 0o777,
        0o600
    );
}
