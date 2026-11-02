#[path = "support.rs"]
mod support;
use serial_test::serial;
use skill_studio_core::models::project::{ProjectBinding, ProjectEntry};
use std::fs;
use support::Env;

#[test]
#[serial]
fn collection_migrates_original_and_deleting_backlink_preserves_hub() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let root = env.path().join("repo");
    let source = env.write_simple_skill(&root.join(".agents/skills"), "release");
    let project = ProjectBinding::new("p".into(), "repo".into(), root);
    config.projects.push(project.clone());
    let entries = studio.project_local_skills(&config, &project).unwrap();
    assert!(!entries[0].collected);
    let skill = studio
        .collect_project_skill(&mut config, "p", &source)
        .unwrap();
    assert!(source.join("SKILL.md").exists());
    assert!(skill_studio_core::services::scanner::is_symlink_or_junction(&source));
    assert!(skill_studio_core::fs::paths::paths_alias(
        &config.skill_provenance[&skill.id].original_path,
        &source
    ));
    assert!(studio.project_local_skills(&config, &project).unwrap()[0].collected);
    let other_root = env.path().join("other-repo");
    env.write_simple_skill(&other_root.join(".agents/skills"), "release");
    let other = ProjectBinding::new("other".into(), "other".into(), other_root);
    assert!(!studio.project_local_skills(&config, &other).unwrap()[0].collected);
    studio
        .delete_project_local_skill(&config, "p", &source)
        .unwrap();
    assert!(studio.skill_backups().unwrap().is_empty());
    assert!(!source.exists());
    assert!(skill.source_path.join("SKILL.md").exists());
    assert!(studio
        .project_local_skills(&config, &project)
        .unwrap()
        .is_empty());
    studio.release_from_hub(&mut config, &skill.id).unwrap();
    assert!(!studio.project_local_skills(&config, &project).unwrap()[0].collected);
}

#[test]
#[serial]
fn deletion_rejects_non_project_paths_and_managed_entries() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let root = env.path().join("repo");
    let source = env.write_simple_skill(&root.join(".claude/skills"), "demo");
    let outside = env.write_simple_skill(&env.claude_skills(), "outside");
    let mut project = ProjectBinding::new("p".into(), "repo".into(), root);
    project.managed_entries.push(ProjectEntry {
        skill_id: "s".into(),
        source_path: outside.clone(),
        target_path: source.clone(),
    });
    config.projects.push(project);
    assert!(studio
        .delete_project_local_skill(&config, "p", &outside)
        .is_err());
    assert!(studio
        .delete_project_local_skill(&config, "p", &source)
        .is_err());
    assert!(source.exists() && outside.exists());
}

#[test]
#[cfg(unix)]
#[serial]
fn deleting_link_preserves_target_and_rejects_external_parent() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let root = env.path().join("repo");
    let dir = root.join(".agents/skills");
    fs::create_dir_all(&dir).unwrap();
    let outside = env.write_simple_skill(&env.path().join("external"), "demo");
    let link = dir.join("demo");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    config
        .projects
        .push(ProjectBinding::new("p".into(), "repo".into(), root));
    studio
        .delete_project_local_skill(&config, "p", &link)
        .unwrap();
    assert!(studio.skill_backups().unwrap().is_empty());
    assert!(outside.join("SKILL.md").exists());
    fs::remove_dir(&dir).unwrap();
    std::os::unix::fs::symlink(outside.parent().unwrap(), &dir).unwrap();
    assert!(studio
        .delete_project_local_skill(&config, "p", &link)
        .is_err());
    assert!(outside.join("SKILL.md").exists());
}
