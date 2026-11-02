#[path = "support.rs"]
mod support;
use serial_test::serial;
use skill_studio_core::{models::project::ProjectBinding, services::scanner};
use std::fs;
use support::Env;

#[test]
#[serial]
fn rejects_parent_paths_even_when_parent_contains_skill_document() {
    let env = Env::new();
    let studio = env.studio();
    let config = studio.load_config().unwrap();
    let hub = env.hub();
    fs::create_dir_all(&hub).unwrap();
    let document = hub.parent().unwrap().join("SKILL.md");
    fs::write(&document, "must remain untouched").unwrap();
    assert!(studio
        .stash_skill(&config, "hub", &hub.join(".."), false)
        .is_err());
    assert_eq!(
        fs::read_to_string(document).unwrap(),
        "must remain untouched"
    );
    assert!(studio.skill_backups().unwrap().is_empty());
}

#[test]
#[serial]
fn hub_and_agent_backups_preserve_bytes_sidecars_and_scope_and_refuse_overwrite() {
    let env = Env::new();
    let studio = env.studio();
    let config = studio.load_config().unwrap();
    for (scope, root) in [
        ("hub", env.hub()),
        ("agent:codex", env.codex_skills()),
        ("agent:claude-code", env.claude_skills()),
    ] {
        let path = env.write_simple_skill(&root, "demo");
        fs::write(path.join(scanner::COPY_SIDECAR), "metadata").unwrap();
        let hash = scanner::dir_content_hash(&path).unwrap();
        studio.stash_skill(&config, scope, &path, false).unwrap();
        assert!(!path.exists());
        let r = studio
            .skill_backups()
            .unwrap()
            .into_iter()
            .find(|r| r.scope == scope)
            .unwrap();
        fs::create_dir(&path).unwrap();
        fs::write(path.join("keep"), "untouched").unwrap();
        assert!(studio.restore_skill_backup(&config, &r.id).is_err());
        assert_eq!(fs::read_to_string(path.join("keep")).unwrap(), "untouched");
        fs::remove_dir_all(&path).unwrap();
        studio.restore_skill_backup(&config, &r.id).unwrap();
        assert_eq!(scanner::dir_content_hash(&path).unwrap(), hash);
        assert_eq!(
            fs::read_to_string(path.join(scanner::COPY_SIDECAR)).unwrap(),
            "metadata"
        );
        assert!(studio.skill_backups().unwrap().is_empty());
        studio.stash_skill(&config, scope, &path, false).unwrap();
        let r = studio.skill_backups().unwrap().remove(0);
        studio.purge_skill_backup(&r.id).unwrap();
        assert!(studio.skill_backups().unwrap().is_empty());
        assert!(studio.restore_skill_backup(&config, &r.id).is_err());
    }
}
#[test]
#[serial]
fn project_disable_delete_restore_stays_disabled_then_enables() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let root = env.path().join("repo");
    let path = env.write_simple_skill(&root.join(".agents/skills"), "demo");
    let project = ProjectBinding::new("p".into(), "repo".into(), root);
    config.projects.push(project.clone());
    fs::write(path.join("extra.txt"), "additional text").unwrap();
    let expected_tokens = skill_studio_core::services::tokens::estimate_skill(&path);
    assert!(expected_tokens.skill_md > 0 && expected_tokens.extras > 0);
    studio
        .stash_skill(&config, "project:p", &path, true)
        .unwrap();
    assert!(!path.exists());
    assert!(studio.project_local_skills(&config, &project).unwrap()[0].disabled);
    assert_eq!(
        studio.project_local_skills(&config, &project).unwrap()[0].tokens,
        expected_tokens
    );
    studio
        .stash_skill(&config, "project:p", &path, false)
        .unwrap();
    let r = studio.skill_backups().unwrap().remove(0);
    assert!(!r.disabled);
    assert!(r.restore_disabled);
    assert!(studio
        .project_local_skills(&config, &project)
        .unwrap()
        .is_empty());
    studio.restore_skill_backup(&config, &r.id).unwrap();
    assert!(!path.exists());
    assert!(studio.project_local_skills(&config, &project).unwrap()[0].disabled);
    studio.restore_skill_backup(&config, &r.id).unwrap();
    assert!(path.join("SKILL.md").exists());
    assert!(!studio.project_local_skills(&config, &project).unwrap()[0].disabled);
    assert!(studio.skill_backups().unwrap().is_empty());
}
#[test]
#[serial]
fn imports_legacy_backup_with_original_path() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let root = env.path().join("repo");
    let path = env.write_simple_skill(&root.join(".agents/skills"), "legacy");
    config
        .projects
        .push(ProjectBinding::new("p".into(), "repo".into(), root));
    let dir = studio.store().dir().join("deleted-project-skills/old");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("origin.json"), serde_json::to_vec(&path).unwrap()).unwrap();
    fs::rename(&path, dir.join("legacy")).unwrap();
    studio.migrate_project_backups(&config).unwrap();
    studio.migrate_project_backups(&config).unwrap();
    let r = studio.skill_backups().unwrap().remove(0);
    assert_eq!(r.original_path, path);
    assert_eq!(r.scope, "project:p");
    studio.restore_skill_backup(&config, &r.id).unwrap();
    assert!(path.join("SKILL.md").exists());
}
#[test]
#[cfg(unix)]
#[serial]
fn links_are_not_deleted_with_targets_and_real_backups_preserve_nested_links() {
    let env = Env::new();
    let studio = env.studio();
    let config = studio.load_config().unwrap();
    let path = env.write_simple_skill(&env.hub(), "demo");
    fs::write(path.join("extra"), "bytes").unwrap();
    std::os::unix::fs::symlink("extra", path.join("link")).unwrap();
    fs::create_dir_all(env.codex_skills()).unwrap();
    let link = env.codex_skills().join("demo");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(studio.stash_skill(&config, "hub", &path, false).is_err());
    studio
        .stash_skill(&config, "agent:codex", &link, false)
        .unwrap();
    assert!(studio.skill_backups().unwrap().is_empty());
    assert!(path.exists());
    studio.stash_skill(&config, "hub", &path, false).unwrap();
    let r = studio.skill_backups().unwrap().remove(0);
    studio.restore_skill_backup(&config, &r.id).unwrap();
    assert_eq!(
        fs::read_link(path.join("link")).unwrap(),
        std::path::PathBuf::from("extra")
    );
    assert!(studio
        .stash_skill(&config, "hub", &env.hub().join(".."), false)
        .is_err());
}

#[test]
#[cfg(unix)]
#[serial]
fn relative_external_symlink_survives_backup_roundtrip() {
    let env = Env::new();
    let studio = env.studio();
    let config = studio.load_config().unwrap();
    let path = env.write_simple_skill(&env.hub(), "demo");
    fs::write(env.hub().join("shared"), "shared data").unwrap();
    std::os::unix::fs::symlink("../shared", path.join("link")).unwrap();
    studio.stash_skill(&config, "hub", &path, false).unwrap();
    let r = studio.skill_backups().unwrap().remove(0);
    studio.restore_skill_backup(&config, &r.id).unwrap();
    assert_eq!(
        fs::read_link(path.join("link")).unwrap(),
        std::path::PathBuf::from("../shared")
    );
    assert_eq!(
        fs::read_to_string(path.join("link")).unwrap(),
        "shared data"
    );
}

#[test]
#[serial]
fn reserved_backup_sibling_does_not_break_listing() {
    let env = Env::new();
    let studio = env.studio();
    let config = studio.load_config().unwrap();
    let path = env.write_simple_skill(&env.hub(), "demo");
    studio.stash_skill(&config, "hub", &path, false).unwrap();
    let record = studio.skill_backups().unwrap().remove(0);
    let dir = studio.backup_payload(&record).parent().unwrap().to_owned();
    let reserved = dir.with_file_name(format!(
        ".{}.backup-123-456",
        skill_studio_core::models::skill::skill_id_for(&dir)
    ));
    fs::rename(&dir, &reserved).unwrap();
    assert!(studio.skill_backups().unwrap().is_empty());
    fs::rename(reserved, dir).unwrap();
    assert_eq!(studio.skill_backups().unwrap()[0].id, record.id);
}

#[test]
#[serial]
fn malformed_backup_directory_names_still_report_record_mismatches() {
    let env = Env::new();
    let studio = env.studio();
    let config = studio.load_config().unwrap();
    let path = env.write_simple_skill(&env.hub(), "demo");
    studio.stash_skill(&config, "hub", &path, false).unwrap();
    let record = studio.skill_backups().unwrap().remove(0);
    let dir = studio.backup_payload(&record).parent().unwrap().to_owned();
    for name in [
        ".hidden-backup",
        ".0123456789abcdeg.backup-123-456",
        ".0123456789abcdef.backup-invalid-456",
        ".0123456789abcdef.backup-123-456-extra",
        ".0123456789abcdef.staging-123-456",
    ] {
        let renamed = dir.with_file_name(name);
        fs::rename(&dir, &renamed).unwrap();
        assert!(
            matches!(
                studio.skill_backups(),
                Err(skill_studio_core::Error::InvalidInput(message)) if message == "备份记录不匹配"
            ),
            "{name}"
        );
        fs::rename(renamed, &dir).unwrap();
    }
    assert_eq!(studio.skill_backups().unwrap()[0].id, record.id);
}
