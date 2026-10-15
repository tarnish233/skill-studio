#[path = "support.rs"]
mod support;
use serial_test::serial;
use skill_studio_core::{
    models::{
        project::ProjectBinding,
        skill::{LinkMode, LinkStatus},
    },
    services::{
        marketplace::{self, CatalogPreparation},
        scanner, variants,
    },
};
use std::{
    fs,
    io::{Cursor, Write},
};
use support::Env;

fn archive(paths: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, body) in paths {
        zip.start_file(
            format!("repo/{path}/SKILL.md"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        write!(zip, "---\nname: demo\ndescription: {body}\n---\n{body}").unwrap();
        zip.start_file(
            format!("repo/{path}/scripts/tool.txt"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        write!(zip, "{body}").unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn bundle(generic: bool) -> marketplace::PreparedSkill {
    let mut paths = vec![
        (".claude/skills/demo", "claude"),
        (".codex/skills/demo", "codex"),
    ];
    if generic {
        paths.push(("skills/demo", "generic"));
    }
    marketplace::prepare_archive("owner/repo", "demo", &archive(&paths)).unwrap()
}
fn body(path: &std::path::Path) -> String {
    fs::read_to_string(path.join("scripts/tool.txt")).unwrap()
}

#[test]
#[serial]
fn damaged_sources_preserve_identity_and_allow_cleanup_of_intact_copies() {
    for missing in [false, true] {
        let env = Env::new();
        let studio = env.studio();
        let mut c = studio.load_config().unwrap();
        let skill = studio.install_catalog_skill(&mut c, &bundle(true)).unwrap();
        let ids = vec![skill.id.clone()];
        let agents = vec!["claude-code".into(), "codex".into()];
        studio
            .register(&mut c, &ids, &agents, Some(LinkMode::Copy), false)
            .unwrap();
        let path = variants::payload(&skill.source_path, "codex").unwrap();
        if missing {
            fs::remove_dir_all(path).unwrap();
        } else {
            fs::write(path.join("SKILL.md"), "---\nname: [broken\n---").unwrap();
        }
        let views = studio.scan_skills(&c).unwrap();
        assert_eq!(views.len(), 1);
        assert!(views[0].diagnostics.is_empty());
        assert!(views[0].agents["codex"].unavailable_reason.is_some());
        assert!(views[0].agents["codex"].status.is_registered());
        assert!(views[0].agents["claude-code"].unavailable_reason.is_none());
        studio
            .set_skill_enabled(&c, &skill.id, "codex", false)
            .unwrap();
        let report = studio.unregister(&mut c, &ids, &agents, false).unwrap();
        assert!(report.failed.is_empty());
        assert!(!env.codex_skills().join("demo").exists());
        assert!(!env.claude_skills().join("demo").exists());
        assert!(studio.load_config().unwrap().registrations.is_empty());
    }
}

#[test]
#[serial]
fn missing_source_does_not_authorize_deleting_modified_copy_and_management_can_exit() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let skill = studio.install_catalog_skill(&mut c, &bundle(true)).unwrap();
    studio
        .register(
            &mut c,
            std::slice::from_ref(&skill.id),
            &["codex".into()],
            Some(LinkMode::Copy),
            false,
        )
        .unwrap();
    fs::remove_dir_all(variants::payload(&skill.source_path, "codex").unwrap()).unwrap();
    let target = env.codex_skills().join("demo");
    fs::write(target.join("scripts/tool.txt"), "local edit").unwrap();
    assert!(!studio
        .unregister(
            &mut c,
            std::slice::from_ref(&skill.id),
            &["codex".into()],
            false
        )
        .unwrap()
        .failed
        .is_empty());
    assert!(target.exists());
    assert!(c.registration(&skill.id, "codex").is_some());
    assert!(studio.set_agent_management(&mut c, "codex", false).is_err());
    fs::write(target.join("scripts/tool.txt"), "codex").unwrap();
    studio.set_agent_management(&mut c, "codex", false).unwrap();
    assert!(!target.exists());
    assert!(c.settings.disabled_agents.contains(&"codex".into()));
}

#[test]
#[serial]
fn unregister_rolls_back_files_when_config_cannot_be_saved() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let skill = studio.install_catalog_skill(&mut c, &bundle(true)).unwrap();
    studio
        .register(
            &mut c,
            std::slice::from_ref(&skill.id),
            &["codex".into()],
            Some(LinkMode::Copy),
            false,
        )
        .unwrap();
    let path = studio.store().config_path();
    let bytes = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(studio
        .unregister(
            &mut c,
            std::slice::from_ref(&skill.id),
            &["codex".into()],
            false
        )
        .is_err());
    assert_eq!(body(&env.codex_skills().join("demo")), "codex");
    assert!(c.registration(&skill.id, "codex").is_some());
    fs::remove_dir(&path).unwrap();
    fs::write(path, bytes).unwrap();
}

#[test]
#[serial]
fn preview_damage_does_not_block_healthy_group_or_project_and_project_removal_ignores_source_damage(
) {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let skill = studio.install_catalog_skill(&mut c, &bundle(true)).unwrap();
    fs::write(
        skill.source_path.join("SKILL.md"),
        "---\nname: [broken\n---",
    )
    .unwrap();
    studio
        .save_group(&mut c, "g".into(), "Group", vec![skill.id.clone()])
        .unwrap();
    studio
        .activate_agent_group(&mut c, "codex", Some("g"))
        .unwrap();
    studio.activate_agent_group(&mut c, "codex", None).unwrap();
    fs::remove_file(skill.source_path.join("SKILL.md")).unwrap();
    let views = studio.scan_skills(&c).unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].skill.id, skill.id);
    let root = env.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let mut project = ProjectBinding::new("p".into(), "Project".into(), root.clone());
    project.agent_ids = vec!["claude-code".into(), "codex".into()];
    project.skill_ids = vec![skill.id.clone()];
    c.projects.push(project);
    studio.apply_project(&mut c, "p").unwrap();
    fs::remove_dir_all(variants::payload(&skill.source_path, "codex").unwrap()).unwrap();
    let report = studio
        .unapply_project(&mut c, "p", std::slice::from_ref(&skill.id), false)
        .unwrap();
    assert!(report.failed.is_empty());
    assert!(!root.join(".claude/skills/demo").exists());
    assert!(!root.join(".agents/skills/demo").exists());
    assert!(studio.load_config().unwrap().projects[0]
        .managed_entries
        .is_empty());
}

#[cfg(unix)]
#[test]
#[serial]
fn missing_preview_keeps_variant_links_attached_to_the_hub_identity() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let skill = studio.install_catalog_skill(&mut c, &bundle(true)).unwrap();
    studio
        .register(
            &mut c,
            std::slice::from_ref(&skill.id),
            &["codex".into(), "pi".into()],
            Some(LinkMode::Symlink),
            false,
        )
        .unwrap();
    fs::remove_file(skill.source_path.join("SKILL.md")).unwrap();
    let views = studio.scan_skills(&c).unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].skill.id, skill.id);
    assert_eq!(views[0].skill.name, "demo");
    assert_eq!(views[0].agents["codex"].status, LinkStatus::Linked);
    assert_eq!(views[0].agents["pi"].status, LinkStatus::Linked);
    let payload = variants::payload(&skill.source_path, "codex").unwrap();
    let outside = env.path().join("outside");
    fs::rename(&payload, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &payload).unwrap();
    let views = studio.scan_skills(&c).unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].skill.id, skill.id);
    assert!(views[0].agents["codex"].unavailable_reason.is_some());
    studio
        .unregister(
            &mut c,
            std::slice::from_ref(&skill.id),
            &["codex".into(), "pi".into()],
            false,
        )
        .unwrap();
    assert!(outside.join("SKILL.md").is_file());
    assert!(!env.codex_skills().join("demo").exists());
}

#[test]
#[serial]
fn deployment_checks_reject_bad_actual_sources_for_bundles_and_single_skills() {
    for bundled in [false, true] {
        let env = Env::new();
        let studio = env.studio();
        let mut c = studio.load_config().unwrap();
        let prepared = if bundled {
            bundle(true)
        } else {
            marketplace::prepare_archive(
                "owner/repo",
                "demo",
                &archive(&[("skills/demo", "generic")]),
            )
            .unwrap()
        };
        let skill = studio.install_catalog_skill(&mut c, &prepared).unwrap();
        let source = if bundled {
            variants::payload(&skill.source_path, "codex").unwrap()
        } else {
            skill.source_path.clone()
        };
        fs::write(source.join("SKILL.md"), "---\nname: [broken\n---").unwrap();
        studio
            .save_group(&mut c, "g".into(), "Group", vec![skill.id.clone()])
            .unwrap();
        assert!(studio
            .activate_agent_group(&mut c, "codex", Some("g"))
            .is_err());
        assert!(!env.codex_skills().join("demo").exists());
        let root = env.path().join("project");
        fs::create_dir_all(&root).unwrap();
        let mut project = ProjectBinding::new("p".into(), "Project".into(), root.clone());
        project.agent_ids = vec!["codex".into()];
        project.skill_ids = vec![skill.id.clone()];
        c.projects.push(project);
        assert!(studio.apply_project(&mut c, "p").is_err());
        assert!(!root.join(".agents/skills/demo").exists());
        assert_eq!(
            studio
                .register(
                    &mut c,
                    std::slice::from_ref(&skill.id),
                    &["codex".into()],
                    Some(LinkMode::Copy),
                    false
                )
                .unwrap()
                .failed
                .len(),
            1
        );
    }
}

#[test]
#[serial]
fn variants_share_one_identity_but_deploy_distinct_payloads_and_fallback() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let skill = studio.install_catalog_skill(&mut c, &bundle(true)).unwrap();
    assert_eq!(c.skill_installations[&skill.id].variants.len(), 3);
    let agents = vec!["claude-code".into(), "codex".into(), "opencode".into()];
    let report = studio
        .register(
            &mut c,
            std::slice::from_ref(&skill.id),
            &agents,
            Some(LinkMode::Copy),
            false,
        )
        .unwrap();
    assert_eq!(report.failed.len(), 0);
    assert_eq!(body(&env.claude_skills().join("demo")), "claude");
    assert_eq!(body(&env.codex_skills().join("demo")), "codex");
    let views = studio.scan_skills(&c).unwrap();
    assert_eq!(views.len(), 1);
    let other = &views[0].agents["opencode"].target_path;
    assert_eq!(body(other), "generic");
    assert!(!other.join(variants::DIRECTORY).exists());
    for id in agents {
        assert_eq!(views[0].agents[&id].status, LinkStatus::Copied);
    }
    c.settings.preserve_manual_skills = true;
    studio
        .set_skill_enabled(&c, &skill.id, "codex", false)
        .unwrap();
    assert!(studio.scan_skills(&c).unwrap()[0].agents["codex"].disabled);
    studio
        .set_skill_enabled(&c, &skill.id, "codex", true)
        .unwrap();
    assert!(!studio.scan_skills(&c).unwrap()[0].agents["codex"].disabled);
    let payload = variants::payload(&skill.source_path, "codex").unwrap();
    fs::write(payload.join("scripts/tool.txt"), "codex updated").unwrap();
    let views = studio.scan_skills(&c).unwrap();
    assert_eq!(views[0].agents["codex"].status, LinkStatus::CopyStale);
    assert_eq!(views[0].agents["claude-code"].status, LinkStatus::Copied);
    assert_eq!(
        studio.load_config().unwrap().skill_installations[&skill.id]
            .variants
            .len(),
        3
    );
}

#[test]
#[serial]
fn dedicated_only_never_falls_back_to_another_agent_and_project_failure_is_atomic() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let skill = studio
        .install_catalog_skill(&mut c, &bundle(false))
        .unwrap();
    let report = studio
        .register(
            &mut c,
            std::slice::from_ref(&skill.id),
            &["pi".into()],
            Some(LinkMode::Copy),
            false,
        )
        .unwrap();
    assert_eq!(report.failed.len(), 1);
    assert!(studio.scan_skills(&c).unwrap()[0].agents["pi"]
        .unavailable_reason
        .is_some());
    let root = env.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let mut project = ProjectBinding::new("p".into(), "Project".into(), root.clone());
    project.agent_ids = vec!["claude-code".into(), "pi".into()];
    project.skill_ids = vec![skill.id.clone()];
    c.projects.push(project);
    assert!(studio.apply_project(&mut c, "p").is_err());
    assert!(!root.join(".claude/skills/demo").exists());
    assert!(c.projects[0].managed_entries.is_empty());
}

#[test]
#[serial]
fn groups_and_projects_record_resolved_paths_and_protect_bundle_deletion() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let skill = studio
        .install_catalog_skill(&mut c, &bundle(false))
        .unwrap();
    studio
        .save_group(&mut c, "g".into(), "Group", vec![skill.id.clone()])
        .unwrap();
    studio
        .activate_agent_group(&mut c, "codex", Some("g"))
        .unwrap();
    assert_eq!(body(&env.codex_skills().join("demo")), "codex");
    assert_eq!(c.active_groups["codex"].entries[0].skill_id, skill.id);
    assert!(c.active_groups["codex"].entries[0]
        .source_path
        .ends_with(".skill-studio-variants/codex"));
    assert!(studio
        .stash_skill(&c, "hub", &skill.source_path, false)
        .is_err());
    studio.activate_agent_group(&mut c, "codex", None).unwrap();
    let root = env.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let mut project = ProjectBinding::new("p".into(), "Project".into(), root.clone());
    project.agent_ids = vec!["claude-code".into(), "codex".into()];
    project.skill_ids = vec![skill.id.clone()];
    c.projects.push(project);
    studio.apply_project(&mut c, "p").unwrap();
    assert_eq!(body(&root.join(".claude/skills/demo")), "claude");
    assert_eq!(body(&root.join(".agents/skills/demo")), "codex");
    assert!(c.projects[0]
        .managed_entries
        .iter()
        .all(|e| e.skill_id == skill.id
            && e.source_path.starts_with(
                skill
                    .source_path
                    .join(variants::DIRECTORY)
                    .canonicalize()
                    .unwrap()
            )));
    assert!(studio
        .stash_skill(&c, "hub", &skill.source_path, false)
        .is_err());
    studio.set_project_enabled(&mut c, "p", false).unwrap();
    studio
        .stash_skill(&c, "hub", &skill.source_path, false)
        .unwrap();
    let backup = studio.skill_backups().unwrap().pop().unwrap();
    studio.restore_skill_backup(&c, &backup.id).unwrap();
    let views = studio.scan_skills(&c).unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(
        body(&variants::payload(&views[0].skill.source_path, "codex").unwrap()),
        "codex"
    );
    studio
        .activate_agent_group(&mut c, "codex", Some("g"))
        .unwrap();
}

#[cfg(unix)]
#[test]
#[serial]
fn symlinks_do_not_create_extra_hub_cards_and_block_bundle_stashing() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let skill = studio
        .install_catalog_skill(&mut c, &bundle(false))
        .unwrap();
    studio
        .register(
            &mut c,
            std::slice::from_ref(&skill.id),
            &["codex".into()],
            Some(LinkMode::Symlink),
            false,
        )
        .unwrap();
    assert_eq!(studio.scan_skills(&c).unwrap().len(), 1);
    assert_eq!(
        studio.scan_skills(&c).unwrap()[0].agents["codex"].status,
        LinkStatus::Linked
    );
    assert!(studio
        .stash_skill(&c, "hub", &skill.source_path, false)
        .is_err());
    studio
        .unregister(
            &mut c,
            std::slice::from_ref(&skill.id),
            &["codex".into()],
            false,
        )
        .unwrap();
    assert!(!env.codex_skills().join("demo").exists());
    studio
        .stash_skill(&c, "hub", &skill.source_path, false)
        .unwrap();
}

#[test]
fn duplicate_scopes_require_choices_and_unknown_examples_are_not_candidates() {
    let bytes = archive(&[
        ("skills/demo", "generic-a"),
        (".agents/skills/demo", "generic-b"),
        (".codex/skills/demo", "codex"),
    ]);
    let CatalogPreparation::SelectionRequired(candidates) =
        marketplace::prepare_catalog_archive_variants("owner/repo", "demo", &bytes, &[]).unwrap()
    else {
        panic!("expected choices")
    };
    assert_eq!(candidates.len(), 3);
    let CatalogPreparation::Ready(prepared) = marketplace::prepare_catalog_archive_variants(
        "owner/repo",
        "demo",
        &bytes,
        &["skills/demo".into()],
    )
    .unwrap() else {
        panic!("expected bundle")
    };
    assert_eq!(prepared.variants.len(), 2);
    assert!(marketplace::prepare_catalog_archive_variants(
        "owner/repo",
        "demo",
        &bytes,
        &["skills/demo".into(), ".agents/skills/demo".into()]
    )
    .is_err());
    assert!(marketplace::prepare_catalog_archive_variants(
        "owner/repo",
        "demo",
        &bytes,
        &["../escape".into()]
    )
    .is_err());
    let unknown = archive(&[
        ("examples/foo/demo", "unknown"),
        (".codex/skills/demo", "codex"),
    ]);
    let CatalogPreparation::Ready(prepared) =
        marketplace::prepare_catalog_archive_variants("owner/repo", "demo", &unknown, &[]).unwrap()
    else {
        panic!("only the supported candidate should remain")
    };
    assert_eq!(prepared.variants.len(), 1);
    assert_eq!(prepared.variants[0].key, "codex");
    assert!(marketplace::prepare_catalog_archive_variants(
        "owner/repo",
        "demo",
        &unknown,
        &["examples/foo/demo".into()]
    )
    .is_err());
}

#[test]
#[serial]
fn installation_rejects_corrupt_payloads_and_foreign_repository_overwrites() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let mut prepared = bundle(true);
    prepared.variants[0].key = "../outside".into();
    assert!(studio.install_catalog_skill(&mut c, &prepared).is_err());
    let prepared = bundle(true);
    fs::write(
        variants::payload(&prepared.directory, "codex")
            .unwrap()
            .join("SKILL.md"),
        "tampered",
    )
    .unwrap();
    assert!(studio.install_catalog_skill(&mut c, &prepared).is_err());
    assert!(!env.hub().join("demo").exists());
    let prepared = bundle(true);
    let skill = studio.install_catalog_skill(&mut c, &prepared).unwrap();
    let hash = scanner::dir_content_hash(&skill.source_path).unwrap();
    let mut foreign = bundle(true);
    foreign.source = "another/repo".into();
    assert!(studio.install_catalog_skill(&mut c, &foreign).is_err());
    assert_eq!(scanner::dir_content_hash(&skill.source_path).unwrap(), hash);
    assert!(studio.release_from_hub(&mut c, &skill.id).is_err());
    assert_eq!(
        studio.install_catalog_skill(&mut c, &prepared).unwrap().id,
        skill.id
    );
}
