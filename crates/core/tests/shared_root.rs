//! Shared roots retain only a link to Hub. Leaving Agent management must never
//! recreate an independent Skill directory from the original backup.
#[path = "support.rs"]
mod support;
use serial_test::serial;
use skill_studio_core::{
    models::{agent::in_shared_root, skill::LinkStatus},
    services::scanner,
};
use std::collections::HashMap;
#[cfg(unix)]
use std::fs;
use support::Env;

#[test]
#[serial]
fn shared_collection_keeps_the_same_link_and_only_one_hub_source() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let original = env.write_simple_skill(&env.agents_skills(), "yuque");
    let source = studio.scan_skills(&config).unwrap().remove(0).skill;
    let hub = studio.adopt_to_hub(&mut config, &source.id).unwrap();
    assert!(scanner::is_symlink_or_junction(&original));
    assert_eq!(config.registrations[&hub.id]["codex"].target_path, original);
    assert!(!env.codex_skills().join("yuque").exists());
    let views = studio.scan_skills(&config).unwrap();
    assert_eq!(views.len(), 1);
    for agent in ["codex", "pi", "opencode", "grok"] {
        assert_eq!(views[0].agents[agent].status, LinkStatus::Linked);
    }
}

#[test]
#[serial]
#[cfg(unix)]
fn leaving_and_reenabling_codex_preserves_shared_and_private_backlinks() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let original = env.write_simple_skill(&env.agents_skills(), "yuque");
    fs::create_dir_all(env.codex_skills()).unwrap();
    let alias = env.codex_skills().join("yuque");
    std::os::unix::fs::symlink(&original, &alias).unwrap();
    let source = studio.scan_skills(&config).unwrap().remove(0).skill;
    let hub = studio.adopt_to_hub(&mut config, &source.id).unwrap();
    config.settings.preserve_manual_skills = false;
    studio.reconcile_manual_policy(&mut config, true).unwrap();
    studio
        .set_agent_management(&mut config, "codex", false)
        .unwrap();
    assert!(scanner::is_symlink_or_junction(&original));
    assert!(scanner::is_symlink_or_junction(&alias));
    let views = studio.scan_skills(&config).unwrap();
    assert_eq!(views.len(), 1);
    assert!(!views[0].agents["codex"].disabled);
    assert!(views[0].agents["pi"].disabled);
    studio
        .set_agent_management(&mut config, "codex", true)
        .unwrap();
    let views = studio.scan_skills(&config).unwrap();
    assert_eq!(views[0].skill.id, hub.id);
    assert!(views[0].agents["codex"].disabled);
    assert!(config.active_groups.is_empty());
}

#[test]
#[serial]
fn explicit_release_returns_the_only_source_to_its_original_shared_directory() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let original = env.write_simple_skill(&env.agents_skills(), "yuque");
    let source = studio.scan_skills(&config).unwrap().remove(0).skill;
    let hub = studio.adopt_to_hub(&mut config, &source.id).unwrap();
    let released = studio.release_from_hub(&mut config, &hub.id).unwrap();
    assert_eq!(released.source_path, original);
    assert!(!scanner::is_symlink_or_junction(&original));
    assert!(!hub.source_path.exists());
    assert!(!env.codex_skills().join("yuque").exists());
    assert_eq!(studio.scan_skills(&config).unwrap().len(), 1);
}

#[test]
#[serial]
fn shared_root_ownership_is_direct_child_only() {
    let env = Env::new();
    std::env::set_var("DSH_AGENTS_HOME", env.path().join("custom-shared"));
    let root = env.path().join("custom-shared/skills");
    let overrides = HashMap::new();
    assert!(!in_shared_root(&root, &overrides));
    assert!(in_shared_root(&root.join("demo"), &overrides));
    assert!(!in_shared_root(&root.join("nested/demo"), &overrides));
    std::env::remove_var("DSH_AGENTS_HOME");
}
