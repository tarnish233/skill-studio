#[path = "support.rs"]
mod support;

use serial_test::serial;
use skill_studio_core::{
    models::{
        agent::require_agent,
        project::{ProjectBinding, ProjectSelection},
        skill::{LinkMode, LinkStatus},
    },
    services::{detector, scanner},
};
use std::{collections::HashMap, fs};
use support::Env;

const AGENTS: [(&str, &str, &str); 3] = [
    ("opencode", ".config/opencode/skills", ".opencode/skills"),
    ("pi", ".pi/agent/skills", ".pi/skills"),
    ("grok", ".grok/skills", ".grok/skills"),
];

#[test]
#[serial]
fn dedicated_roots_and_metadata_match_each_agent_without_false_detection() {
    let env = Env::new();
    env.write_simple_skill(&env.agents_skills(), "shared");
    for (id, global, project) in AGENTS {
        let agent = require_agent(id).unwrap();
        let info = detector::describe_agent(agent, &HashMap::new(), true);
        assert!(
            !info.detected,
            "{id}: a shared root alone is not an installation"
        );
        assert_eq!(info.global_skill_dirs[0], env.path().join(global));
        assert_eq!(info.global_skill_dirs[1], env.agents_skills());
        assert_eq!(info.project_skill_dir.as_deref(), Some(project));
        assert!(!info.supports_native_toggle);
        env.write_simple_skill(&env.path().join(global), id);
        assert!(detector::describe_agent(agent, &HashMap::new(), true).detected);
    }
}

#[test]
#[serial]
fn overrides_and_environment_variables_preserve_multilevel_paths_and_shared_root() {
    let env = Env::new();
    std::env::set_var("XDG_CONFIG_HOME", env.path().join("xdg"));
    std::env::set_var("OPENCODE_CONFIG_DIR", env.path().join("opencode-extra"));
    std::env::set_var("PI_CODING_AGENT_DIR", env.path().join("pi-custom"));
    std::env::set_var("GROK_HOME", env.path().join("grok-custom"));
    for (id, config) in [
        ("opencode", "xdg/opencode"),
        ("pi", "pi-custom"),
        ("grok", "grok-custom"),
    ] {
        let agent = require_agent(id).unwrap();
        let roots = agent.resolved_global_roots(&HashMap::new());
        assert_eq!(roots[0], env.path().join(config).join("skills"));
        assert_eq!(roots[1], env.agents_skills());
        if id == "opencode" {
            assert_eq!(roots[2], env.path().join("opencode-extra/skills"));
        }
        let overrides = HashMap::from([(id.into(), env.path().join("chosen"))]);
        assert_eq!(
            agent.resolved_global_roots(&overrides),
            vec![env.path().join("chosen/skills"), env.agents_skills(),]
        );
    }
    for var in [
        "XDG_CONFIG_HOME",
        "OPENCODE_CONFIG_DIR",
        "PI_CODING_AGENT_DIR",
        "GROK_HOME",
    ] {
        std::env::remove_var(var);
    }
}

#[test]
#[serial]
fn shared_skills_are_deduplicated_and_available_to_all_four_consumers() {
    let env = Env::new();
    let studio = env.studio();
    let c = studio.load_config().unwrap();
    env.write_simple_skill(&env.agents_skills(), "shared");
    let skills = studio.scan_skills(&c).unwrap();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].source_ids, vec!["agent"]);
    for id in ["codex", "opencode", "pi", "grok"] {
        assert_eq!(skills[0].agents[id].status, LinkStatus::Source, "{id}");
        assert_eq!(skills[0].agents[id].entry_paths.len(), 1);
    }
}

#[test]
#[serial]
fn install_and_remove_target_only_each_agents_private_root() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let source = env.write_simple_skill(&env.hub(), "demo");
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    for mode in [LinkMode::Copy, LinkMode::Symlink] {
        for (agent, root, _) in AGENTS {
            let report = studio
                .register(
                    &mut c,
                    std::slice::from_ref(&id),
                    &[agent.into()],
                    Some(mode),
                    false,
                )
                .unwrap();
            assert!(report.failed.is_empty(), "{agent}: {:?}", report.failed);
            assert!(env.path().join(root).join("demo/SKILL.md").is_file());
            assert!(!env.agents_skills().join("demo").exists());
            let report = studio
                .unregister(&mut c, std::slice::from_ref(&id), &[agent.into()], false)
                .unwrap();
            assert!(report.failed.is_empty());
            assert!(!env.path().join(root).join("demo").exists());
            assert!(source.join("SKILL.md").is_file());
        }
    }
}

#[test]
#[serial]
fn manual_policy_does_not_block_group_switches_or_rewrite_native_configuration() {
    for (agent, root, _) in AGENTS {
        let env = Env::new();
        let studio = env.studio();
        let mut c = studio.load_config().unwrap();
        let root = env.path().join(root);
        let manual = env.write_simple_skill(&root, "manual");
        let config_path = root.parent().unwrap().join(match agent {
            "opencode" => "opencode.jsonc",
            "pi" => "settings.json",
            _ => "config.toml",
        });
        let original = b"user native settings must remain byte-for-byte intact";
        fs::write(&config_path, original).unwrap();
        c.settings.preserve_manual_skills = false;
        studio.reconcile_manual_policy(&mut c, true).unwrap();
        let manual_state = &studio.scan_skills(&c).unwrap()[0].agents[agent];
        assert!(!manual_state.disabled && !manual_state.policy_blocked);
        for (name, group) in [("first", "a"), ("second", "b")] {
            env.write_simple_skill(&env.hub(), name);
            let id = studio
                .scan_skills(&c)
                .unwrap()
                .into_iter()
                .find(|s| s.skill.name == name)
                .unwrap()
                .skill
                .id;
            studio
                .save_group(&mut c, group.into(), group, vec![id])
                .unwrap();
            studio
                .activate_agent_group(&mut c, agent, Some(group))
                .unwrap();
            assert!(root.join(name).join("SKILL.md").is_file());
            assert!(manual.join("SKILL.md").is_file());
        }
        assert!(!root.join("first").exists());
        studio.activate_agent_group(&mut c, agent, None).unwrap();
        assert!(!root.join("second").exists());
        studio
            .activate_agent_group(&mut c, agent, Some("a"))
            .unwrap();
        studio.set_agent_management(&mut c, agent, false).unwrap();
        assert!(!root.join("first").exists());
        assert!(manual.join("SKILL.md").is_file());
        assert_eq!(fs::read(&config_path).unwrap(), original);
        assert!(!c.policy_suspensions.contains_key(agent));
        studio.set_agent_management(&mut c, agent, true).unwrap();
    }
}

#[test]
#[serial]
fn adopt_release_and_deleted_skill_restore_preserve_originals_for_each_agent() {
    for (agent, root, _) in AGENTS {
        let env = Env::new();
        let studio = env.studio();
        let mut c = studio.load_config().unwrap();
        let original = env.write_simple_skill(&env.path().join(root), "manual");
        fs::write(original.join("resource.txt"), "unchanged").unwrap();
        let skill = studio.scan_skills(&c).unwrap().remove(0);
        assert_eq!(skill.source_ids, vec![agent]);
        let managed = studio.adopt_to_hub(&mut c, &skill.skill.id).unwrap();
        assert!(managed.source_path.join("SKILL.md").is_file());
        let released = studio.release_from_hub(&mut c, &managed.id).unwrap();
        assert_eq!(released.source_path, original);
        assert_eq!(
            fs::read_to_string(original.join("resource.txt")).unwrap(),
            "unchanged"
        );
        studio
            .stash_skill(&c, &format!("agent:{agent}"), &original, false)
            .unwrap();
        assert!(!original.exists());
        let backup = studio
            .skill_backups()
            .unwrap()
            .into_iter()
            .find(|r| r.original_path == original && r.scope == format!("agent:{agent}"))
            .unwrap();
        studio.restore_skill_backup(&c, &backup.id).unwrap();
        assert!(original.join("SKILL.md").is_file());
    }
}

#[test]
#[serial]
fn multi_agent_project_deployments_use_separate_roots_and_keep_manual_skills() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let root = env.path().join("project");
    fs::create_dir(&root).unwrap();
    c.projects.push(ProjectBinding::new(
        "p".into(),
        "Project".into(),
        root.clone(),
    ));
    env.write_simple_skill(&env.hub(), "demo");
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    for (_, _, relative) in AGENTS {
        env.write_simple_skill(&root.join(relative), "manual");
    }
    let report = studio
        .write_project(
            &mut c,
            "p",
            Some(ProjectSelection {
                agent_ids: AGENTS.iter().map(|(id, _, _)| (*id).into()).collect(),
                skill_ids: vec![id],
                group_ids: vec![],
                link_mode: LinkMode::Copy,
            }),
        )
        .unwrap();
    assert!(report.failed.is_empty());
    assert_eq!(c.projects[0].managed_entries.len(), 3);
    assert_eq!(scanner::scan_project(&root).unwrap().len(), 6);
    for (_, _, relative) in AGENTS {
        assert!(root.join(relative).join("demo/SKILL.md").is_file());
    }
    studio.set_project_enabled(&mut c, "p", false).unwrap();
    for (_, _, relative) in AGENTS {
        assert!(!root.join(relative).join("demo").exists());
        assert!(root.join(relative).join("manual/SKILL.md").is_file());
    }
}
