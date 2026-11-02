#[path = "support.rs"]
mod support;
use serial_test::serial;
use skill_studio_core::{
    models::{
        group::Group,
        project::ProjectBinding,
        skill::{LinkMode, LinkStatus},
    },
    services::{linker, scanner},
};
use std::fs;
use support::Env;

#[test]
#[serial]
#[cfg(unix)]
fn migration_preserves_relative_absolute_external_and_dangling_links() {
    let env = Env::new();
    let source = env.write_simple_skill(&env.claude_skills(), "demo");
    fs::create_dir(source.join("nested")).unwrap();
    fs::write(source.join("real.txt"), "resource").unwrap();
    fs::write(env.claude_skills().join("external.txt"), "external").unwrap();
    for (target, name) in [
        ("../real.txt".into(), "nested/relative"),
        (source.join("real.txt"), "absolute"),
        ("../external.txt".into(), "external"),
        ("missing".into(), "dangling"),
        (".".into(), "loop"),
    ] {
        std::os::unix::fs::symlink(target, source.join(name)).unwrap();
    }
    let studio = env.studio();
    let mut cfg = studio.load_config().unwrap();
    cfg.settings.default_link_mode = LinkMode::Copy;
    let id = studio.scan_skills(&cfg).unwrap()[0].skill.id.clone();
    let adopted = studio.adopt_to_hub(&mut cfg, &id).unwrap();
    for root in [&adopted.source_path, &source] {
        assert_eq!(
            fs::read_link(root.join("loop")).unwrap(),
            std::path::PathBuf::from(".")
        );
        assert_eq!(
            fs::read_to_string(root.join("nested/relative")).unwrap(),
            "resource"
        );
        assert_eq!(
            fs::read_to_string(root.join("absolute")).unwrap(),
            "resource"
        );
        assert_eq!(
            fs::read_to_string(root.join("external")).unwrap(),
            "external"
        );
        assert!(root
            .join("dangling")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(root
            .join("loop")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());
    }
    assert_eq!(
        linker::link_status(&adopted.source_path, &source),
        LinkStatus::Linked
    );
    assert_eq!(
        studio.load_config().unwrap().registrations[&adopted.id]["claude-code"].target_path,
        source
    );
}

#[test]
#[serial]
fn migration_updates_global_and_project_metadata_without_resetting_baseline() {
    let env = Env::new();
    let source = env.write_simple_skill(&env.claude_skills(), "demo");
    let studio = env.studio();
    let mut cfg = studio.load_config().unwrap();
    cfg.settings.default_link_mode = LinkMode::Copy;
    let id = studio.scan_skills(&cfg).unwrap()[0].skill.id.clone();
    assert!(studio
        .register(
            &mut cfg,
            std::slice::from_ref(&id),
            &["codex".into()],
            Some(LinkMode::Copy),
            false
        )
        .unwrap()
        .is_all_ok());
    let root = env.path().join("project");
    fs::create_dir(&root).unwrap();
    let mut project = ProjectBinding::new("p".into(), "project".into(), root.clone());
    project.agent_ids.push("claude-code".into());
    project.skill_ids.push(id.clone());
    cfg.projects.push(project);
    let mut group = Group::new("g".into(), "group".into());
    group.skill_ids.push(id.clone());
    cfg.groups.push(group);
    assert!(studio.apply_project(&mut cfg, "p").unwrap().is_all_ok());
    let dest = env.codex_skills().join("demo");
    let project_dest = root.join(".claude/skills/demo");
    let baseline = scanner::read_copy_sidecar(&dest).unwrap().source_hash;
    fs::write(source.join("new.txt"), "source changed").unwrap();
    let adopted = studio.adopt_to_hub(&mut cfg, &id).unwrap();
    for target in [&dest, &project_dest] {
        let meta = scanner::read_copy_sidecar(target).unwrap();
        assert_eq!(meta.skill_id, adopted.id);
        assert_eq!(meta.source_path, adopted.source_path);
        assert_eq!(meta.source_hash, baseline);
        assert_eq!(
            linker::link_status(&adopted.source_path, target),
            LinkStatus::CopyStale
        );
    }
    assert_eq!(cfg.projects[0].skill_ids, vec![adopted.id.clone()]);
    assert_eq!(cfg.groups[0].skill_ids, vec![adopted.id.clone()]);
    assert!(studio
        .register(
            &mut cfg,
            std::slice::from_ref(&adopted.id),
            &["codex".into()],
            Some(LinkMode::Copy),
            false
        )
        .unwrap()
        .is_all_ok());
    assert!(studio
        .unapply_project(&mut cfg, "p", &[adopted.id], false)
        .unwrap()
        .is_all_ok());
}

#[test]
#[serial]
fn modified_conflicted_and_damaged_copies_are_protected() {
    let env = Env::new();
    let source = env.write_simple_skill(&env.claude_skills(), "demo");
    let dest = env.codex_skills().join("demo");
    linker::register(&source, &dest, LinkMode::Copy, "id", false).unwrap();
    fs::write(dest.join("SKILL.md"), "local work").unwrap();
    assert_eq!(
        linker::link_status(&source, &dest),
        LinkStatus::CopyModified
    );
    assert!(linker::register(&source, &dest, LinkMode::Copy, "id", false).is_err());
    assert!(linker::unregister(&source, &dest, false).is_err());
    fs::write(source.join("new.txt"), "upstream").unwrap();
    assert_eq!(
        linker::link_status(&source, &dest),
        LinkStatus::CopyConflict
    );
    assert!(linker::register(&source, &dest, LinkMode::Auto, "id", false).is_err());
    assert_eq!(
        fs::read_to_string(dest.join("SKILL.md")).unwrap(),
        "local work"
    );
    fs::remove_file(dest.join("SKILL.md")).unwrap();
    assert_eq!(linker::link_status(&source, &dest), LinkStatus::CopyDamaged);
    assert!(linker::unregister(&source, &dest, false).is_err());
}

#[test]
#[serial]
#[cfg(unix)]
fn migration_retargets_existing_links_even_when_owner_uses_copy() {
    let env = Env::new();
    let source = env.write_simple_skill(&env.claude_skills(), "demo");
    let studio = env.studio();
    let mut cfg = studio.load_config().unwrap();
    cfg.settings.default_link_mode = LinkMode::Copy;
    let id = studio.scan_skills(&cfg).unwrap()[0].skill.id.clone();
    studio
        .register(
            &mut cfg,
            std::slice::from_ref(&id),
            &["codex".into()],
            Some(LinkMode::Symlink),
            false,
        )
        .unwrap();
    let adopted = studio.adopt_to_hub(&mut cfg, &id).unwrap();
    assert_eq!(
        linker::link_status(&adopted.source_path, &env.codex_skills().join("demo")),
        LinkStatus::Linked
    );
    assert_eq!(
        linker::link_status(&adopted.source_path, &source),
        LinkStatus::Linked
    );
}

#[test]
#[serial]
fn excessive_depth_fails_without_removing_source_or_old_copy() {
    let env = Env::new();
    let source = env.write_simple_skill(&env.claude_skills(), "demo");
    let dest = env.codex_skills().join("demo");
    linker::register(&source, &dest, LinkMode::Copy, "id", false).unwrap();
    let old = fs::read(dest.join("SKILL.md")).unwrap();
    let mut deep = source.clone();
    for _ in 0..18 {
        deep.push("nested");
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("file"), "preserve").unwrap();
    assert!(linker::replace_dest_with_copy(&source, &dest, "id").is_err());
    let studio = env.studio();
    let mut cfg = studio.load_config().unwrap();
    let id = studio.scan_skills(&cfg).unwrap()[0].skill.id.clone();
    assert!(studio.adopt_to_hub(&mut cfg, &id).is_err());
    assert_eq!(fs::read(dest.join("SKILL.md")).unwrap(), old);
    assert_eq!(fs::read_to_string(deep.join("file")).unwrap(), "preserve");
    assert!(!env.hub().join("demo").exists());
}

#[test]
#[serial]
#[cfg(unix)]
fn sidecar_write_failure_rolls_back_source_hub_and_config() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    let source = env.write_simple_skill(&env.claude_skills(), "demo");
    let studio = env.studio();
    let mut cfg = studio.load_config().unwrap();
    cfg.settings.default_link_mode = LinkMode::Copy;
    let id = studio.scan_skills(&cfg).unwrap()[0].skill.id.clone();
    studio
        .register(
            &mut cfg,
            std::slice::from_ref(&id),
            &["codex".into()],
            Some(LinkMode::Copy),
            false,
        )
        .unwrap();
    studio.save_config(&cfg).unwrap();
    let dest = env.codex_skills().join("demo");
    let config_before = fs::read(studio.store().config_path()).unwrap();
    let meta_before = fs::read(dest.join(scanner::COPY_SIDECAR)).unwrap();
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o555)).unwrap();
    let result = studio.adopt_to_hub(&mut cfg, &id);
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.is_err());
    assert!(!source.symlink_metadata().unwrap().file_type().is_symlink());
    assert!(!source.join(scanner::COPY_SIDECAR).exists());
    assert!(!env.hub().join("demo").exists());
    assert_eq!(
        fs::read(studio.store().config_path()).unwrap(),
        config_before
    );
    assert_eq!(
        fs::read(dest.join(scanner::COPY_SIDECAR)).unwrap(),
        meta_before
    );
    assert!(cfg.registrations.contains_key(&id));
    assert!(!studio.store().dir().join("migration.json").exists());
    studio.adopt_to_hub(&mut cfg, &id).unwrap();
}

#[test]
#[serial]
#[cfg(unix)]
fn replacing_a_copied_symlink_with_regular_text_is_detected() {
    let env = Env::new();
    let source = env.write_simple_skill(&env.claude_skills(), "demo");
    std::fs::write(source.join("resource"), "data").unwrap();
    std::os::unix::fs::symlink("resource", source.join("alias")).unwrap();
    let dest = env.codex_skills().join("demo");
    linker::register(&source, &dest, LinkMode::Copy, "id", false).unwrap();
    std::fs::remove_file(dest.join("alias")).unwrap();
    std::fs::write(dest.join("alias"), "symlink:internal:resource").unwrap();
    assert_eq!(
        linker::link_status(&source, &dest),
        LinkStatus::CopyModified
    );
    assert!(linker::register(&source, &dest, LinkMode::Copy, "id", false).is_err());
}

/// `.skill-studio-replace-*.json` files are gitignored, not hidden, so they travel
/// with a cloned repository — and startup sweeps every bound project's agent dir for
/// them by filename alone, then executes the absolute paths inside. A journal that
/// names a path outside its own directory must be refused, and startup must survive
/// refusing it.
#[test]
#[serial]
fn a_project_journal_naming_an_outside_path_is_refused_without_blocking_startup() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let root = env.path().join("cloned-repo");
    let scanned = root.join(".agents/skills");
    fs::create_dir_all(&scanned).unwrap();
    config
        .projects
        .push(ProjectBinding::new("p".into(), "P".into(), root));
    studio.save_config(&config).unwrap();

    let outside = env.path().join("precious");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep.txt"), "mine").unwrap();
    let journal = scanned.join(".skill-studio-replace-demo.json");
    fs::write(
        &journal,
        serde_json::to_vec(&serde_json::json!({
            "journal": journal,
            "committed": false,
            "entries": [{
                "target": outside,
                "backup": env.path().join("nowhere"),
                "existed": false,
            }],
        }))
        .unwrap(),
    )
    .unwrap();

    // Startup still completes, and the directory the journal pointed at is untouched.
    studio.load_config().unwrap();
    assert!(outside.join("keep.txt").is_file());
    assert!(journal.is_file(), "越界的日志应原样保留，不静默删除");
}
