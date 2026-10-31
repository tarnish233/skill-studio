//! Harness 标准目录的接入边界；不启动真实 Harness，不读取用户 Profile。
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

const DSH: &str = "deepseek-harness";
struct Variables;
impl Drop for Variables {
    fn drop(&mut self) {
        std::env::remove_var("DSH_HOME");
        std::env::remove_var("DSH_AGENTS_HOME");
    }
}

#[test]
#[serial]
fn harness_roots_honor_independent_overrides_and_tilde_expansion() {
    let env = Env::new();
    let _variables = Variables;
    let agent = require_agent(DSH).unwrap();
    assert_eq!(agent.cli_command, "dsh");
    assert!(!detector::describe_agent(agent, &HashMap::new(), true).detected);
    std::env::set_var("DSH_HOME", "~/harness-home");
    std::env::set_var("DSH_AGENTS_HOME", "~/harness-shared");
    assert_eq!(
        agent.resolved_global_roots(&HashMap::new()),
        vec![
            env.path().join("harness-home/skills"),
            env.path().join("harness-shared/skills")
        ]
    );
    assert_eq!(
        agent.resolved_global_roots(&HashMap::from([(DSH.into(), env.path().join("chosen"))])),
        vec![
            env.path().join("chosen/skills"),
            env.path().join("harness-shared/skills")
        ]
    );
    assert_eq!(
        require_agent("codex")
            .unwrap()
            .resolved_shared_roots(&HashMap::new()),
        vec![env.agents_skills()]
    );
    std::env::set_var("DSH_HOME", "  ");
    std::env::set_var("DSH_AGENTS_HOME", "  ");
    assert_eq!(
        agent.primary_global_root(&HashMap::new()),
        env.path().join(".dsh/skills")
    );
}

#[test]
#[serial]
fn harness_reads_shared_skills_without_claiming_the_shared_root_is_an_installation() {
    let env = Env::new();
    let studio = env.studio();
    let config = studio.load_config().unwrap();
    env.write_simple_skill(&env.agents_skills(), "shared");
    let skills = studio.scan_skills(&config).unwrap();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].source_ids, vec!["agent"]);
    assert_eq!(skills[0].agents[DSH].status, LinkStatus::Source);
    assert!(!detector::describe_agent(require_agent(DSH).unwrap(), &HashMap::new(), true).detected);
}

#[test]
#[serial]
fn custom_shared_original_is_adopted_and_restored_only_in_harness_private_root() {
    let env = Env::new();
    let _variables = Variables;
    let shared_home = env.path().join("shared-home");
    std::env::set_var("DSH_AGENTS_HOME", &shared_home);
    let original = env.write_simple_skill(&shared_home.join("skills"), "manual");
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let view = studio.scan_skills(&config).unwrap().remove(0);
    let hub = studio.adopt_to_hub(&mut config, &view.skill.id).unwrap();
    assert!(!original.exists());
    assert_eq!(
        config.registrations[&hub.id][DSH].target_path,
        env.path().join(".dsh/skills/manual")
    );
    studio.release_from_hub(&mut config, &hub.id).unwrap();
    assert!(
        !original.exists(),
        "restoration must not write a shared root"
    );
    assert!(env.path().join(".dsh/skills/manual/SKILL.md").is_file());
}

#[test]
#[serial]
fn project_repository_subdirectory_is_rejected_before_configuration_or_files_change() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let repo = env.path().join("repo");
    let sub = repo.join("packages/app");
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::create_dir_all(&sub).unwrap();
    env.write_simple_skill(&env.hub(), "demo");
    let id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();
    config.projects.push(ProjectBinding::new(
        "p".into(),
        "Subproject".into(),
        sub.clone(),
    ));
    let before = serde_json::to_value(&config).unwrap();
    let error = studio
        .write_project(
            &mut config,
            "p",
            Some(ProjectSelection {
                agent_ids: vec![DSH.into()],
                skill_ids: vec![id],
                group_ids: vec![],
                link_mode: LinkMode::Copy,
            }),
        )
        .unwrap_err();
    assert!(error.to_string().contains("项目根"), "{error}");
    assert_eq!(serde_json::to_value(&config).unwrap(), before);
    assert!(!sub.join(".dsh").exists() && !repo.join(".dsh").exists());
}

#[test]
#[serial]
fn repository_root_change_does_not_prevent_withdrawing_owned_project_files() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let project = env.path().join("repo/app");
    fs::create_dir_all(&project).unwrap();
    env.write_simple_skill(&env.hub(), "demo");
    let id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();
    config.projects.push(ProjectBinding::new(
        "p".into(),
        "App".into(),
        project.clone(),
    ));
    studio
        .write_project(
            &mut config,
            "p",
            Some(ProjectSelection {
                agent_ids: vec![DSH.into()],
                skill_ids: vec![id],
                group_ids: vec![],
                link_mode: LinkMode::Copy,
            }),
        )
        .unwrap();
    studio.set_project_enabled(&mut config, "p", false).unwrap();
    assert!(!project.join(".dsh/skills/demo").exists());
    studio.set_project_enabled(&mut config, "p", true).unwrap();
    fs::create_dir(env.path().join("repo/.git")).unwrap();
    studio
        .write_project(
            &mut config,
            "p",
            Some(ProjectSelection {
                agent_ids: vec![DSH.into()],
                skill_ids: vec![],
                group_ids: vec![],
                link_mode: LinkMode::Copy,
            }),
        )
        .unwrap();
    assert!(!project.join(".dsh/skills/demo").exists());
}

#[test]
#[serial]
fn harness_frontmatter_is_checked_before_group_and_project_deployment() {
    for frontmatter in [
        "name: demo",
        "name: Demo\ndescription: test",
        "name: a--b\ndescription: test",
        "name: demo\ndescription: test\nuser-invocable: maybe",
        "name: demo\ndescription: test\nuserInvocable: false",
    ] {
        let env = Env::new();
        let studio = env.studio();
        let mut config = studio.load_config().unwrap();
        env.write_skill(
            &env.hub(),
            "demo",
            &format!("---\n{frontmatter}\n---\nbody\n"),
        );
        let id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();
        studio
            .save_group(&mut config, "g".into(), "Group", vec![id.clone()])
            .unwrap();
        let before = serde_json::to_value(&config).unwrap();
        let error = studio
            .activate_agent_groups(&mut config, DSH, &["g".into()])
            .unwrap_err();
        assert!(error.to_string().contains("DeepSeek Harness"), "{error}");
        assert_eq!(serde_json::to_value(&config).unwrap(), before);
        assert!(!env.path().join(".dsh").exists());
        let project = env.path().join("project");
        fs::create_dir(&project).unwrap();
        config.projects.push(ProjectBinding::new(
            "p".into(),
            "Project".into(),
            project.clone(),
        ));
        assert!(studio
            .write_project(
                &mut config,
                "p",
                Some(ProjectSelection {
                    agent_ids: vec![DSH.into()],
                    skill_ids: vec![id],
                    group_ids: vec![],
                    link_mode: LinkMode::Copy,
                })
            )
            .unwrap_err()
            .to_string()
            .contains("DeepSeek Harness"));
        assert!(!project.join(".dsh").exists());
        // Harness compatibility must not narrow the existing Agents' accepted format.
        studio
            .activate_agent_groups(&mut config, "opencode", &["g".into()])
            .unwrap();
    }
}

#[test]
#[serial]
fn harness_invocation_flags_are_not_rewritten_into_a_fake_native_toggle() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let source = env.write_skill(&env.hub(), "demo", "---\nname: demo\ndescription: test\ndisable-model-invocation: 'YES'\nuser-invocable: 0\n---\nbody\n");
    let original = fs::read(source.join("SKILL.md")).unwrap();
    let id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();
    studio
        .save_group(&mut config, "g".into(), "Group", vec![id])
        .unwrap();
    studio
        .activate_agent_groups(&mut config, DSH, &["g".into()])
        .unwrap();
    assert_eq!(
        fs::read(env.path().join(".dsh/skills/demo/SKILL.md")).unwrap(),
        original
    );
    studio.activate_agent_groups(&mut config, DSH, &[]).unwrap();
    assert_eq!(fs::read(source.join("SKILL.md")).unwrap(), original);
}

#[test]
#[serial]
fn flat_files_and_system_skills_are_not_silently_adopted_as_directory_bundles() {
    let env = Env::new();
    let root = env.path().join(".dsh/skills");
    env.write_simple_skill(&root, "directory");
    env.write_simple_skill(&root.join(".system"), "builtin");
    let flat = root.join("flat.md");
    fs::write(&flat, "---\nname: flat\ndescription: test\n---\nbody\n").unwrap();
    let bytes = fs::read(&flat).unwrap();
    let entries = scanner::scan_root(&root, require_agent(DSH).unwrap()).unwrap();
    assert_eq!(
        entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["directory"]
    );
    assert_eq!(fs::read(flat).unwrap(), bytes);
}

#[test]
#[serial]
fn harness_env_paths_keep_significant_spaces_but_blank_values_stay_unset() {
    let env = Env::new();
    let _variables = Variables;
    let private = env.path().join("private-root ");
    let shared = env.path().join("shared-root ");
    std::env::set_var("DSH_HOME", &private);
    std::env::set_var("DSH_AGENTS_HOME", &shared);
    assert_eq!(
        require_agent(DSH)
            .unwrap()
            .resolved_global_roots(&HashMap::new()),
        vec![private.join("skills"), shared.join("skills")],
        "Harness preserves nonblank root paths, including significant spaces"
    );
    std::env::set_var("DSH_HOME", "   ");
    std::env::set_var("DSH_AGENTS_HOME", "\t ");
    assert_eq!(
        require_agent(DSH)
            .unwrap()
            .resolved_global_roots(&HashMap::new()),
        vec![env.path().join(".dsh/skills"), env.agents_skills()],
        "whitespace-only values are unset, not roots named with spaces"
    );
}

#[test]
#[serial]
fn custom_dsh_shared_root_is_a_neutral_agent_source() {
    let env = Env::new();
    let _variables = Variables;
    let shared = env.path().join("custom-shared");
    std::env::set_var("DSH_AGENTS_HOME", &shared);
    env.write_simple_skill(&shared.join("skills"), "demo");
    env.write_simple_skill(&env.path().join(".dsh/skills"), "private");
    let studio = env.studio();
    let config = studio.load_config().unwrap();
    let views = studio.scan_skills(&config).unwrap();
    assert_eq!(views.len(), 2);
    let shared_view = views.iter().find(|v| v.skill.name == "demo").unwrap();
    let private_view = views.iter().find(|v| v.skill.name == "private").unwrap();
    assert_eq!(
        shared_view.source_ids,
        vec!["agent".to_string()],
        "a resolved shared root is neutral even when its path has no .agents component"
    );
    assert_eq!(
        private_view.source_ids,
        vec![DSH.to_string()],
        "the private Harness root keeps its private source label"
    );
    assert_eq!(shared_view.agents[DSH].status, LinkStatus::Source);
    let second = env.path().join("second-shared");
    std::env::set_var("DSH_AGENTS_HOME", &second);
    env.write_simple_skill(&second.join("skills"), "second");
    let views = studio.scan_skills(&config).unwrap();
    assert_eq!(views.len(), 2);
    assert!(views.iter().any(|v| v.skill.name == "second"));
    assert!(!views.iter().any(|v| v.skill.name == "demo"));
    assert_eq!(
        views
            .iter()
            .find(|v| v.skill.name == "second")
            .unwrap()
            .source_ids,
        vec!["agent".to_string()],
        "shared roots are resolved afresh for every scan"
    );
}

#[cfg(unix)]
#[test]
#[serial]
fn shared_root_alias_keeps_physical_private_entries_recognized() {
    let env = Env::new();
    let _variables = Variables;
    let private = env.path().join("private-home");
    let alias = env.path().join("alias-home");
    env.write_simple_skill(&private.join("skills"), "demo");
    std::os::unix::fs::symlink(&private, &alias).unwrap();
    std::env::set_var("DSH_HOME", &private);
    std::env::set_var("DSH_AGENTS_HOME", &alias);
    let studio = env.studio();
    let views = studio.scan_skills(&studio.load_config().unwrap()).unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(
        views[0].source_ids,
        vec!["agent".to_string()],
        "physical aliases cannot be replaced by string equality"
    );
}

#[cfg(unix)]
#[test]
#[serial]
fn historical_agent_directory_source_labelling_survives_external_links() {
    let env = Env::new();
    let _variables = Variables;
    let external = env.write_simple_skill(&env.path().join(".agent/library"), "demo");
    let root = require_agent(DSH)
        .unwrap()
        .primary_global_root(&HashMap::new());
    fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&external, root.join("demo")).unwrap();
    let studio = env.studio();
    let views = studio.scan_skills(&studio.load_config().unwrap()).unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(
        views[0].source_ids,
        vec!["agent".to_string(), DSH.to_string()]
    );
}

#[test]
#[serial]
fn emptied_group_withdraws_even_after_git_root_appears() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let project = env.path().join("repo/app");
    fs::create_dir_all(&project).unwrap();
    env.write_simple_skill(&env.hub(), "demo");
    let id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();
    studio
        .save_group(&mut config, "g".into(), "Group", vec![id])
        .unwrap();
    config.projects.push(ProjectBinding::new(
        "p".into(),
        "App".into(),
        project.clone(),
    ));
    let selection = || ProjectSelection {
        agent_ids: vec![DSH.into()],
        skill_ids: vec![],
        group_ids: vec!["g".into()],
        link_mode: LinkMode::Copy,
    };
    studio
        .write_project(&mut config, "p", Some(selection()))
        .unwrap();
    assert!(project.join(".dsh/skills/demo").exists());
    config.groups[0].skill_ids.clear();
    fs::create_dir(env.path().join("repo/.git")).unwrap();
    studio
        .write_project(&mut config, "p", Some(selection()))
        .unwrap();
    assert!(!project.join(".dsh/skills/demo").exists());
}

#[test]
#[serial]
fn nonempty_group_project_rejects_git_subdirectory_atomically() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let repo = env.path().join("repo");
    let project = repo.join("app");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir(repo.join(".git")).unwrap();
    env.write_simple_skill(&env.hub(), "demo");
    let id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();
    studio
        .save_group(&mut config, "g".into(), "Group", vec![id])
        .unwrap();
    config.projects.push(ProjectBinding::new(
        "p".into(),
        "App".into(),
        project.clone(),
    ));
    let before = serde_json::to_value(&config).unwrap();
    assert!(studio
        .write_project(
            &mut config,
            "p",
            Some(ProjectSelection {
                agent_ids: vec![DSH.into()],
                skill_ids: vec![],
                group_ids: vec!["g".into()],
                link_mode: LinkMode::Copy,
            })
        )
        .is_err());
    assert_eq!(serde_json::to_value(&config).unwrap(), before);
    assert!(!project.join(".dsh").exists());
}
