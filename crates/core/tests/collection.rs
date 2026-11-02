#[path = "support.rs"]
mod support;
use serial_test::serial;
use skill_studio_core::{
    models::{
        agent::require_agent,
        config::SkillInstallation,
        group::Group,
        skill::{skill_id_for, LinkMode, LinkStatus, SkillOrigin},
    },
    services::{linker, scanner},
};
use std::fs;
use support::Env;

#[test]
#[serial]
fn discovery_resolves_the_hub_identity_through_original_and_parent_aliases() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let original = env.write_simple_skill(&env.path().join("local"), "demo");
    assert!(studio.discover_local_skills(&config, &original).unwrap()[0]
        .hub_id
        .is_none());
    let hub = studio
        .import_local_skill(&mut config, &original)
        .unwrap()
        .skill;
    let found = studio.discover_local_skills(&config, &original).unwrap();
    assert_eq!(found[0].path, original);
    assert_eq!(found[0].hub_id.as_deref(), Some(hub.id.as_str()));
    #[cfg(unix)]
    {
        let alias = env.path().join("alias-parent");
        std::os::unix::fs::symlink(original.parent().unwrap(), &alias).unwrap();
        assert_eq!(
            studio
                .discover_local_skills(&config, &alias.join("demo"))
                .unwrap()[0]
                .hub_id
                .as_deref(),
            Some(hub.id.as_str())
        );
    }
}

#[test]
#[cfg(unix)]
#[serial]
fn valid_long_directory_names_do_not_overflow_transaction_backup_names() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let original = env.write_simple_skill(&env.path().join("local"), &"x".repeat(240));
    let hub = studio
        .import_local_skill(&mut config, &original)
        .unwrap()
        .skill;
    assert!(scanner::is_symlink_or_junction(&original));
    assert_eq!(
        linker::link_status(&hub.source_path, &original),
        LinkStatus::Linked
    );
    assert!(hub.source_path.join("SKILL.md").exists());
}

#[test]
#[serial]
fn collection_moves_the_complete_source_and_ignores_copy_deployment_preference() {
    let env = Env::new();
    let source = env.write_simple_skill(&env.path().join("local"), "demo");
    fs::create_dir_all(source.join(".hidden/subdir")).unwrap();
    fs::write(source.join(".hidden/subdir/data.bin"), [0, 1, 2, 255]).unwrap();
    let before = scanner::dir_content_hash(&source).unwrap();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    config.settings.default_link_mode = LinkMode::Copy;
    let result = studio.import_local_skill(&mut config, &source).unwrap();
    assert!(result.changed);
    assert!(skill_studio_core::fs::paths::paths_alias(
        &result.original_path,
        &source
    ));
    assert_eq!(result.skill.source_path, env.hub().join("demo"));
    assert!(scanner::is_symlink_or_junction(&source));
    assert_eq!(
        linker::link_status(&result.skill.source_path, &source),
        LinkStatus::Linked
    );
    assert_eq!(
        scanner::dir_content_hash(&result.skill.source_path).unwrap(),
        before
    );
    assert!(!source.join(scanner::COPY_SIDECAR).exists());
    assert!(config.skill_installations.is_empty());
    let record = &config.skill_provenance[&result.skill.id];
    assert!(skill_studio_core::fs::paths::paths_alias(
        &record.original_path,
        &source
    ));
    assert_eq!(record.original_origin, SkillOrigin::Local);
    assert_eq!(
        scanner::dir_content_hash(&record.backup_path.join("content")).unwrap(),
        before
    );
    assert_eq!(studio.scan_skills(&config).unwrap().len(), 1);
    let saved = fs::read(studio.store().config_path()).unwrap();
    assert!(
        !studio
            .import_local_skill(&mut config, &source)
            .unwrap()
            .changed
    );
    assert_eq!(fs::read(studio.store().config_path()).unwrap(), saved);
    let restored = studio
        .release_from_hub(&mut config, &result.skill.id)
        .unwrap();
    assert_eq!(restored.origin, SkillOrigin::Local);
    assert!(!scanner::is_symlink_or_junction(&source));
    assert!(!result.skill.source_path.exists());
    assert_eq!(
        studio.scan_skills(&config).unwrap()[0].skill.id,
        restored.id
    );
}

#[test]
#[serial]
fn policy_suspensions_migrate_without_enabling_the_skill_or_changing_native_config() {
    for agent_id in ["claude-code", "codex", "pi"] {
        let env = Env::new();
        let studio = env.studio();
        let mut config = studio.load_config().unwrap();
        let agent = require_agent(agent_id).unwrap();
        let root = agent.primary_global_root(&config.settings.agent_dir_overrides);
        let original = env.write_simple_skill(&root, "rex");
        let old_id = skill_id_for(&original);
        config.settings.preserve_manual_skills = false;
        studio.reconcile_manual_policy(&mut config, true).unwrap();
        let native = agent
            .toggle_config_path(&config.settings.agent_dir_overrides)
            .unwrap();
        let bytes = fs::read(&native).unwrap();
        assert!(studio.scan_skills(&config).unwrap()[0].agents[agent_id].policy_suspended);
        let hub = studio.adopt_to_hub(&mut config, &old_id).unwrap();
        assert!(scanner::is_symlink_or_junction(&original));
        assert!(!config.settings.preserve_manual_skills);
        assert!(config.active_groups.is_empty());
        assert_eq!(fs::read(&native).unwrap(), bytes);
        let ledger = &config.policy_suspensions[agent_id];
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].skill_id, hub.id);
        assert_eq!(ledger[0].source_path, hub.source_path);
        assert_eq!(ledger[0].target_path, original);
        for _ in 0..2 {
            let views = studio.scan_skills_with_policy(&mut config).unwrap();
            let state = &views[0].agents[agent_id];
            assert_eq!(state.status, LinkStatus::Linked);
            assert!(state.disabled && state.policy_suspended);
            assert_eq!(fs::read(&native).unwrap(), bytes);
        }
        let restored = studio.release_from_hub(&mut config, &hub.id).unwrap();
        assert_eq!(restored.id, old_id);
        assert_eq!(fs::read(&native).unwrap(), bytes);
        assert_eq!(config.policy_suspensions[agent_id][0].source_path, original);
        assert_eq!(config.policy_suspensions[agent_id][0].skill_id, old_id);
        assert!(studio.scan_skills(&config).unwrap()[0].agents[agent_id].disabled);
    }
}

#[test]
#[serial]
fn user_disabled_skills_are_not_mislabelled_as_policy_suspensions() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let source = env.write_simple_skill(&env.claude_skills(), "manual-off");
    let old_id = skill_id_for(&source);
    studio
        .set_skill_enabled(&config, &old_id, "claude-code", false)
        .unwrap();
    let native = env.path().join(".claude/settings.json");
    let before = fs::read(&native).unwrap();
    let hub = studio.adopt_to_hub(&mut config, &old_id).unwrap();
    let view = studio.scan_skills(&config).unwrap().remove(0);
    assert_eq!(view.skill.id, hub.id);
    assert!(view.agents["claude-code"].disabled);
    assert!(!view.agents["claude-code"].policy_suspended);
    assert_eq!(fs::read(native).unwrap(), before);
}

#[test]
#[serial]
fn an_identical_imported_snapshot_converges_without_losing_references() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let original = env.write_simple_skill(&env.claude_skills(), "rex");
    let copy = env.write_simple_skill(&env.hub(), "rex");
    let old_id = skill_id_for(&original);
    let hub_id = skill_id_for(&copy);
    let mut group = Group::new("g".into(), "g".into());
    group.skill_ids = vec![old_id.clone(), hub_id.clone()];
    config.groups.push(group);
    config.skill_installations.insert(
        hub_id.clone(),
        SkillInstallation {
            source: format!("local:{}", original.display()),
            skill_id: "rex".into(),
            repository_path: String::new(),
            installed_at: 1,
            content_hash: scanner::dir_content_hash(&original).unwrap(),
            variants: vec![],
        },
    );
    studio
        .register(
            &mut config,
            std::slice::from_ref(&hub_id),
            &["pi".into()],
            Some(LinkMode::Copy),
            false,
        )
        .unwrap();
    let hub = studio.adopt_to_hub(&mut config, &old_id).unwrap();
    assert_eq!(hub.id, hub_id);
    assert_eq!(config.groups[0].skill_ids, vec![hub_id.clone()]);
    assert!(scanner::is_symlink_or_junction(&original));
    assert!(!config.skill_installations.contains_key(&hub_id));
    assert_eq!(config.registrations[&hub_id]["pi"].mode, LinkMode::Copy);
    assert_eq!(studio.scan_skills(&config).unwrap().len(), 1);
    assert_eq!(config.skill_provenance[&hub_id].original_path, original);
}

#[test]
#[serial]
fn edited_or_unrelated_hub_content_is_never_overwritten() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let original = env.write_simple_skill(&env.claude_skills(), "rex");
    let copy = env.write_simple_skill(&env.hub(), "rex");
    let id = skill_id_for(&copy);
    config.skill_installations.insert(
        id,
        SkillInstallation {
            source: format!("local:{}", original.display()),
            skill_id: "rex".into(),
            repository_path: String::new(),
            installed_at: 1,
            content_hash: scanner::dir_content_hash(&original).unwrap(),
            variants: vec![],
        },
    );
    fs::write(copy.join("edited.txt"), "keep my edits").unwrap();
    let before = scanner::dir_content_hash(&copy).unwrap();
    assert!(studio.import_local_skill(&mut config, &original).is_err());
    assert!(!scanner::is_symlink_or_junction(&original));
    assert_eq!(scanner::dir_content_hash(&copy).unwrap(), before);
    assert!(config.skill_provenance.is_empty());
}

#[test]
#[cfg(unix)]
#[serial]
fn a_late_filesystem_failure_rolls_back_the_source_and_hub() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    config.settings.default_link_mode = LinkMode::Copy;
    let name = "demo";
    let source = env.write_simple_skill(&env.path().join("local"), name);
    fs::create_dir_all(studio.store().config_path()).unwrap();
    // Saving configuration fails after the back-link has been created. The
    // enclosing journal must restore the real source and remove the new Hub.
    assert!(studio.import_local_skill(&mut config, &source).is_err());
    assert!(source.join("SKILL.md").exists());
    assert!(!scanner::is_symlink_or_junction(&source));
    assert!(!env.hub().join(name).exists());
    assert!(studio.store().config_path().is_dir());
    assert!(config.skill_provenance.is_empty());
}

#[test]
#[serial]
fn a_recreated_original_is_not_hidden_as_another_hub_identity() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let original = env.write_simple_skill(&env.claude_skills(), "rex");
    let hub = studio
        .adopt_to_hub(&mut config, &skill_id_for(&original))
        .unwrap();
    fs::remove_file(&original).unwrap();
    env.write_simple_skill(&env.claude_skills(), "rex");
    let views = studio.scan_skills(&config).unwrap();
    assert_eq!(views.len(), 2);
    assert_eq!(
        views
            .iter()
            .find(|view| view.skill.id == hub.id)
            .unwrap()
            .agents["claude-code"]
            .status,
        LinkStatus::Foreign
    );
}

#[test]
#[serial]
fn active_group_diagnostics_name_the_actual_agent_and_group() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let source = env.write_simple_skill(&env.claude_skills(), "demo");
    let id = skill_id_for(&source);
    studio
        .save_group(&mut config, "g".into(), "Research", vec![id.clone()])
        .unwrap();
    studio
        .activate_agent_groups(&mut config, "codex", &["g".into()])
        .unwrap();
    let error = studio
        .adopt_to_hub(&mut config, &id)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Codex") && error.contains("Research"));
    assert!(!scanner::is_symlink_or_junction(&source));
}
