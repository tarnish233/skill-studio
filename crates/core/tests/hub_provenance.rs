#[path = "support.rs"]
mod support;
use serial_test::serial;
use skill_studio_core::{
    models::{
        group::Group,
        skill::{LinkMode, LinkStatus},
    },
    services::{linker, scanner},
};
use std::fs;
use support::Env;

#[test]
#[serial]
fn sources_and_full_backup_survive_collection_reload_and_release() {
    for mode in [LinkMode::Symlink, LinkMode::Copy] {
        let env = Env::new();
        let studio = env.studio();
        let mut c = studio.load_config().unwrap();
        let source = env.write_simple_skill(&env.agents_skills(), "demo");
        fs::write(source.join("resource.txt"), "original").unwrap();
        fs::create_dir_all(env.codex_skills()).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&source, env.codex_skills().join("alias")).unwrap();
        let view = studio.scan_skills(&c).unwrap().remove(0);
        let ids = view.source_ids.clone();
        assert!(ids.contains(&"agent".into()));
        #[cfg(unix)]
        assert!(ids.contains(&"codex".into()));
        c.settings.default_link_mode = mode;
        let mut group = Group::new("g".into(), "g".into());
        group.skill_ids.push(view.skill.id.clone());
        c.groups.push(group);
        let adopted = studio.adopt_to_hub(&mut c, &view.skill.id).unwrap();
        let record = c.skill_provenance[&adopted.id].clone();
        assert_eq!(record.source_ids, ids);
        assert_eq!(record.original_path, source);
        assert_eq!(
            fs::read_to_string(record.backup_path.join("content/resource.txt")).unwrap(),
            "original"
        );
        assert!(record.backup_path.join("config.json").is_file());
        assert!(record.backup_path.join("provenance.json").is_file());
        fs::write(adopted.source_path.join("resource.txt"), "latest").unwrap();
        let mut c = studio.load_config().unwrap();
        assert_eq!(studio.scan_skills(&c).unwrap()[0].source_ids, ids);
        let restored = studio.release_from_hub(&mut c, &adopted.id).unwrap();
        assert_eq!(restored.id, view.skill.id);
        assert_eq!(restored.source_path, source);
        assert!(!scanner::is_symlink_or_junction(&source));
        assert!(!source.join(scanner::COPY_SIDECAR).exists());
        assert_eq!(
            fs::read_to_string(source.join("resource.txt")).unwrap(),
            "latest"
        );
        assert!(!adopted.source_path.exists());
        assert_eq!(c.groups[0].skill_ids, vec![restored.id.clone()]);
        assert_eq!(studio.scan_skills(&c).unwrap()[0].source_ids, ids);
        #[cfg(unix)]
        assert_eq!(
            linker::link_status(&source, &env.codex_skills().join("alias")),
            LinkStatus::Linked
        );
        assert_eq!(
            fs::read_to_string(record.backup_path.join("content/resource.txt")).unwrap(),
            "original"
        );
        assert_eq!(
            fs::read_dir(studio.store().dir().join("skill-backups"))
                .unwrap()
                .count(),
            2
        );
    }
}

#[test]
#[serial]
fn generated_registrations_do_not_change_provenance_and_legacy_hub_is_unknown() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    env.write_simple_skill(&env.claude_skills(), "demo");
    let view = studio.scan_skills(&c).unwrap().remove(0);
    studio
        .register(
            &mut c,
            std::slice::from_ref(&view.skill.id),
            &["codex".into()],
            Some(LinkMode::Copy),
            false,
        )
        .unwrap();
    assert_eq!(
        studio.scan_skills(&c).unwrap()[0].source_ids,
        vec!["claude-code"]
    );
    let adopted = studio.adopt_to_hub(&mut c, &view.skill.id).unwrap();
    assert_eq!(
        studio.scan_skills(&c).unwrap()[0].source_ids,
        vec!["claude-code"]
    );
    studio.release_from_hub(&mut c, &adopted.id).unwrap();
    let copy = scanner::read_copy_sidecar(&env.codex_skills().join("demo")).unwrap();
    assert_eq!(copy.skill_id, view.skill.id);
    assert_eq!(copy.source_path, view.skill.source_path);
    env.write_simple_skill(&env.hub(), "legacy");
    let legacy = studio
        .scan_skills(&c)
        .unwrap()
        .into_iter()
        .find(|v| v.skill.name == "legacy")
        .unwrap();
    assert_eq!(legacy.source_ids, vec!["unknown"]);
    assert!(studio.release_from_hub(&mut c, &legacy.skill.id).is_err());
    assert!(legacy.skill.source_path.join("SKILL.md").is_file());
}

#[test]
#[serial]
fn modified_original_copy_blocks_release_without_changes() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let source = env.write_simple_skill(&env.claude_skills(), "demo");
    c.settings.default_link_mode = LinkMode::Copy;
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    let adopted = studio.adopt_to_hub(&mut c, &id).unwrap();
    fs::write(source.join("local.txt"), "do not overwrite").unwrap();
    let before = fs::read(studio.store().config_path()).unwrap();
    assert!(studio.release_from_hub(&mut c, &adopted.id).is_err());
    assert_eq!(
        fs::read_to_string(source.join("local.txt")).unwrap(),
        "do not overwrite"
    );
    assert!(adopted.source_path.join("SKILL.md").is_file());
    assert_eq!(fs::read(studio.store().config_path()).unwrap(), before);
}

#[test]
#[serial]
#[cfg(unix)]
fn failed_release_rolls_back_files_metadata_and_provenance() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let source = env.write_simple_skill(&env.claude_skills(), "demo");
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    studio
        .register(
            &mut c,
            std::slice::from_ref(&id),
            &["codex".into()],
            Some(LinkMode::Copy),
            false,
        )
        .unwrap();
    let adopted = studio.adopt_to_hub(&mut c, &id).unwrap();
    let dest = env.codex_skills().join("demo");
    let config_before = fs::read(studio.store().config_path()).unwrap();
    let sidecar_before = fs::read(dest.join(scanner::COPY_SIDECAR)).unwrap();
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o555)).unwrap();
    let result = studio.release_from_hub(&mut c, &adopted.id);
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.is_err());
    assert_eq!(
        linker::link_status(&adopted.source_path, &source),
        LinkStatus::Linked
    );
    assert_eq!(
        config_before,
        fs::read(studio.store().config_path()).unwrap()
    );
    assert_eq!(
        sidecar_before,
        fs::read(dest.join(scanner::COPY_SIDECAR)).unwrap()
    );
    assert!(c.skill_provenance.contains_key(&adopted.id));
    assert_eq!(
        fs::read_dir(studio.store().dir().join("skill-backups"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
#[serial]
fn repeated_collection_keeps_history_and_disabled_state() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let source = env.write_simple_skill(&env.codex_skills(), "cycle");
    let original = studio.scan_skills(&c).unwrap().remove(0);
    studio
        .set_skill_enabled(&c, &original.skill.id, "codex", false)
        .unwrap();
    for cycle in 0..3 {
        let adopted = studio.adopt_to_hub(&mut c, &original.skill.id).unwrap();
        let mut reload = studio.load_config().unwrap();
        let view = studio.scan_skills(&reload).unwrap().remove(0);
        assert_eq!(view.source_ids, vec!["codex"]);
        assert!(
            view.agents["codex"].disabled,
            "disabled state lost on collection"
        );
        let backup = reload.skill_provenance[&adopted.id].backup_path.clone();
        assert!(backup.join("content/SKILL.md").is_file());
        fs::write(
            adopted.source_path.join("resource.txt"),
            format!("cycle {cycle}"),
        )
        .unwrap();
        studio.release_from_hub(&mut reload, &adopted.id).unwrap();
        c = studio.load_config().unwrap();
        let view = studio.scan_skills(&c).unwrap().remove(0);
        assert_eq!(view.skill.id, original.skill.id);
        assert!(
            view.agents["codex"].disabled,
            "disabled state lost on release"
        );
        assert_eq!(
            fs::read_to_string(source.join("resource.txt")).unwrap(),
            format!("cycle {cycle}")
        );
    }
    assert_eq!(
        fs::read_dir(studio.store().dir().join("skill-backups"))
            .unwrap()
            .count(),
        6
    );
}

#[test]
#[serial]
fn occupied_original_is_protected_and_missing_original_can_be_restored() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let source = env.write_simple_skill(&env.claude_skills(), "occupied");
    let original = studio.scan_skills(&c).unwrap().remove(0);
    let adopted = studio.adopt_to_hub(&mut c, &original.skill.id).unwrap();
    linker::remove_symlink_or_junction(&source).unwrap();
    env.write_skill(&env.claude_skills(), "occupied", "unrelated user content");
    let before = fs::read(studio.store().config_path()).unwrap();
    assert!(studio.release_from_hub(&mut c, &adopted.id).is_err());
    assert_eq!(
        fs::read_to_string(source.join("SKILL.md")).unwrap(),
        "unrelated user content"
    );
    assert_eq!(before, fs::read(studio.store().config_path()).unwrap());
    // Remove only this test's deliberately injected conflicting directory.
    fs::remove_dir_all(&source).unwrap();
    studio.release_from_hub(&mut c, &adopted.id).unwrap();
    assert!(source.join("SKILL.md").is_file());
    assert!(!adopted.source_path.exists());
}

#[test]
#[serial]
fn active_group_blocks_release_until_stopped() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    env.write_simple_skill(&env.claude_skills(), "active");
    let original = studio.scan_skills(&c).unwrap().remove(0);
    let adopted = studio.adopt_to_hub(&mut c, &original.skill.id).unwrap();
    studio
        .save_group(
            &mut c,
            "g".into(),
            "test",
            vec![adopted.id.clone()],
        )
        .unwrap();
    studio
        .activate_agent_group(&mut c, "codex", Some("g"))
        .unwrap();
    let before = fs::read(studio.store().config_path()).unwrap();
    assert!(studio.release_from_hub(&mut c, &adopted.id).is_err());
    assert_eq!(before, fs::read(studio.store().config_path()).unwrap());
    assert!(adopted.source_path.join("SKILL.md").is_file());
    studio.activate_agent_group(&mut c, "codex", None).unwrap();
    let restored = studio.release_from_hub(&mut c, &adopted.id).unwrap();
    assert_eq!(c.groups[0].skill_ids, vec![restored.id]);
}

#[test]
#[serial]
fn release_updates_project_references_without_overwriting_project_edits() {
    use skill_studio_core::models::project::ProjectBinding;
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let source = env.write_simple_skill(&env.claude_skills(), "project-skill");
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    let root = env.path().join("project");
    fs::create_dir(&root).unwrap();
    let mut project = ProjectBinding::new("p".into(), "project".into(), root.clone());
    project.agent_ids.push("claude-code".into());
    project.skill_ids.push(id.clone());
    c.projects.push(project);
    assert!(studio.apply_project(&mut c, "p").unwrap().is_all_ok());
    let dest = root.join(".claude/skills/project-skill");
    let baseline = scanner::read_copy_sidecar(&dest).unwrap().source_hash;
    let adopted = studio.adopt_to_hub(&mut c, &id).unwrap();
    fs::write(dest.join("local.txt"), "project edits").unwrap();
    let restored = studio.release_from_hub(&mut c, &adopted.id).unwrap();
    assert_eq!(c.projects[0].skill_ids, vec![restored.id.clone()]);
    let meta = scanner::read_copy_sidecar(&dest).unwrap();
    assert_eq!(meta.skill_id, restored.id);
    assert_eq!(meta.source_path, source);
    assert_eq!(meta.source_hash, baseline);
    assert_eq!(
        fs::read_to_string(dest.join("local.txt")).unwrap(),
        "project edits"
    );
    assert_eq!(
        linker::link_status(&source, &dest),
        LinkStatus::CopyModified
    );
}
