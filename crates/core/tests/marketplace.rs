#[path = "support.rs"]
mod support;
use serial_test::serial;
use skill_studio_core::services::{
    marketplace::{self, prepare_archive},
    scanner,
};
use std::io::{Cursor, Write};
use support::Env;

fn archive(files: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, text) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
#[serial]
fn install_standard_skill_is_atomic_idempotent_and_has_studio_origin() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let bytes = archive(&[
        (
            "repo-HEAD/skills/demo/SKILL.md",
            "---\nname: demo\ndescription: test\n---\nbody",
        ),
        ("repo-HEAD/skills/demo/scripts/test.txt", "resource"),
        ("repo-HEAD/other/SKILL.md", "---\nname: other\n---"),
    ]);
    let prepared = prepare_archive("owner/repo", "demo", &bytes).unwrap();
    let skill = studio
        .install_catalog_skill(&mut config, &prepared)
        .unwrap();
    assert_eq!(skill.source_path, env.hub().join("demo"));
    assert_eq!(
        std::fs::read_to_string(skill.source_path.join("scripts/test.txt")).unwrap(),
        "resource"
    );
    assert!(!env.hub().join("other").exists());
    assert!(config.registrations.is_empty());
    assert!(!env.codex_skills().exists());
    assert!(!env.claude_skills().exists());
    let mut reloaded = studio.load_config().unwrap();
    let views = studio.scan_skills(&reloaded).unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].source_ids, vec!["studio"]);
    assert!(views[0].provenance.is_none());
    assert_eq!(
        views[0].installation.as_ref().unwrap().repository_path,
        "skills/demo"
    );
    assert_eq!(
        views[0].installation.as_ref().unwrap().content_hash,
        scanner::dir_content_hash(&skill.source_path).unwrap()
    );
    assert_eq!(
        studio
            .install_catalog_skill(&mut reloaded, &prepared)
            .unwrap()
            .id,
        skill.id
    );
    assert!(studio.release_from_hub(&mut reloaded, &skill.id).is_err());
}

#[test]
#[serial]
fn same_name_collision_never_overwrites_existing_hub_skill() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let existing = env.write_simple_skill(&env.hub(), "demo");
    let original = std::fs::read(existing.join("SKILL.md")).unwrap();
    let bytes = archive(&[("repo/skills/demo/SKILL.md", "---\nname: demo\n---\nnew")]);
    let prepared = prepare_archive("owner/repo", "demo", &bytes).unwrap();
    assert!(studio
        .install_catalog_skill(&mut config, &prepared)
        .is_err());
    assert_eq!(std::fs::read(existing.join("SKILL.md")).unwrap(), original);
    assert!(config.skill_installations.is_empty());
}

#[test]
fn malformed_ambiguous_and_unsafe_archives_are_rejected() {
    for files in [
        vec![("../escape", "bad")],
        vec![
            ("root/skills/a/SKILL.md", "---\nname: demo\n---"),
            ("root/skills/b/SKILL.md", "---\nname: demo\n---"),
        ],
        vec![("root/skills/demo/SKILL.md", "---\nname: [invalid\n---")],
        vec![("root/README.md", "no skill")],
    ] {
        assert!(prepare_archive("owner/repo", "demo", &archive(&files)).is_err());
    }
    assert!(prepare_archive("owner/repo", "demo", b"not a zip").is_err());
    for (source, id) in [
        ("../repo", "demo"),
        ("owner/repo/extra", "demo"),
        ("owner/repo", "../demo"),
        ("https://evil/repo", "demo"),
    ] {
        assert!(marketplace::validate_coordinates(source, id).is_err());
    }
}

#[test]
fn catalog_discovery_and_variant_scopes_share_registry_whitelist() {
    use skill_studio_core::{models::agent::AGENTS, services::variants};
    let roots = variants::repository_roots();
    assert_eq!(AGENTS.len(), 6);
    for agent in AGENTS {
        assert!(roots.values().any(|key| *key == agent.id));
    }
    for (root, key) in roots {
        let path = format!("{root}/demo");
        let file = format!("envelope/{path}/SKILL.md");
        let bytes = archive(&[
            (&file, "---\nname: demo\n---\nsupported"),
            (
                "envelope/.future-agent/skills/demo/SKILL.md",
                "---\nname: [invalid\n---",
            ),
            (
                "envelope/examples/.claude/skills/demo/SKILL.md",
                "---\nname: demo\n---\nexample",
            ),
        ]);
        let prepared = prepare_archive("owner/repo", "demo", &bytes).unwrap();
        assert_eq!(prepared.repository_path, path);
        assert_eq!(variants::repository_key(&path).as_deref(), Some(key));
        assert!(marketplace::prepare_catalog_archive(
            "owner/repo",
            "demo",
            &bytes,
            Some(".future-agent/skills/demo")
        )
        .is_err());
    }
    for path in [
        ".future-agent/skills/demo",
        ".openclaw/skills/demo",
        "plugins/demo",
        "examples/.codex/skills/demo",
        "skills/nested/demo",
        "demo",
        ".agents/rules/demo",
    ] {
        let file = format!("envelope/{path}/SKILL.md");
        assert!(
            prepare_archive(
                "owner/repo",
                "demo",
                &archive(&[(&file, "---\nname: demo\n---")])
            )
            .is_err(),
            "unexpected source: {path}"
        );
    }
}

#[test]
fn repository_root_skill_is_a_supported_generic_source() {
    let prepared = prepare_archive(
        "owner/repo",
        "demo",
        &archive(&[("envelope/SKILL.md", "---\nname: demo\n---\nroot skill")]),
    )
    .unwrap();
    assert_eq!(prepared.repository_path, "");
    assert!(prepared.variants.is_empty());
}

#[test]
fn multiple_repository_envelopes_are_rejected() {
    assert!(prepare_archive(
        "owner/repo",
        "demo",
        &archive(&[
            ("one/skills/demo/SKILL.md", "---\nname: demo\n---"),
            ("two/skills/demo/SKILL.md", "---\nname: demo\n---")
        ])
    )
    .is_err());
}

#[test]
#[serial]
fn catalog_candidates_use_complete_content_and_install_only_the_selected_path() {
    use marketplace::{prepare_catalog_archive, CatalogPreparation};
    let document = "---\nname: demo\ndescription: shared description\n---\nbody";
    let bytes = archive(&[
        ("root/skills/demo/SKILL.md", document),
        ("root/skills/demo/scripts/tool.txt", "codex resource"),
        ("root/.agents/skills/demo/SKILL.md", document),
        (
            "root/.agents/skills/demo/scripts/tool.txt",
            "claude resource",
        ),
        ("root/.openclaw/skills/demo/SKILL.md", document),
        ("root/skills/other/SKILL.md", "---\nname: other\n---"),
    ]);
    let CatalogPreparation::SelectionRequired(candidates) =
        prepare_catalog_archive("owner/repo", "demo", &bytes, None).unwrap()
    else {
        panic!("expected selection")
    };
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].repository_path, ".agents/skills/demo");
    assert_eq!(candidates[1].repository_path, "skills/demo");
    assert_eq!(
        candidates[0].description.as_deref(),
        Some("shared description")
    );
    assert_ne!(candidates[0].content_hash, candidates[1].content_hash);
    for invalid in [
        "../demo",
        "/.codex/skills/demo",
        ".codex/skills/demo/../demo",
        "skills/other",
        "missing",
        ".openclaw/skills/demo",
    ] {
        assert!(prepare_catalog_archive("owner/repo", "demo", &bytes, Some(invalid)).is_err());
    }
    let CatalogPreparation::Ready(prepared) =
        prepare_catalog_archive("owner/repo", "demo", &bytes, Some("skills/demo")).unwrap()
    else {
        panic!("expected ready")
    };
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let skill = studio
        .install_catalog_skill(&mut config, &prepared)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(skill.source_path.join("scripts/tool.txt")).unwrap(),
        "codex resource"
    );
    assert_eq!(
        config.skill_installations[&skill.id].repository_path,
        "skills/demo"
    );
    let CatalogPreparation::Ready(other) =
        prepare_catalog_archive("owner/repo", "demo", &bytes, Some(".agents/skills/demo")).unwrap()
    else {
        panic!("expected ready")
    };
    assert!(studio.install_catalog_skill(&mut config, &other).is_err());
}

#[test]
fn malformed_sibling_is_reported_but_does_not_block_explicit_healthy_selection() {
    let bytes = archive(&[
        ("root/skills/demo/SKILL.md", "---\nname: demo\n---\nhealthy"),
        (
            "root/.codex/skills/demo/SKILL.md",
            "---\nname: [broken\n---",
        ),
    ]);
    let marketplace::CatalogPreparation::SelectionRequired(candidates) =
        marketplace::prepare_catalog_archive("owner/repo", "demo", &bytes, None).unwrap()
    else {
        panic!("must acknowledge unavailable candidate")
    };
    assert_eq!(candidates.len(), 2);
    assert!(candidates[0]
        .error
        .as_ref()
        .unwrap()
        .contains(".codex/skills/demo"));
    assert!(candidates[1].error.is_none());
    assert!(matches!(
        marketplace::prepare_catalog_archive("owner/repo", "demo", &bytes, Some("skills/demo"))
            .unwrap(),
        marketplace::CatalogPreparation::Ready(_)
    ));
    let error = marketplace::prepare_catalog_archive(
        "owner/repo",
        "demo",
        &bytes,
        Some(".codex/skills/demo"),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains(".codex/skills/demo"));
    let all_bad = archive(&[("root/skills/demo/SKILL.md", "---\nname: [broken\n---")]);
    assert!(
        marketplace::prepare_catalog_archive("owner/repo", "demo", &all_bad, None)
            .err()
            .unwrap()
            .to_string()
            .contains("skills/demo")
    );
}

#[test]
fn unhashable_sibling_does_not_block_a_healthy_candidate() {
    let deep = format!(
        "root/.codex/skills/demo/{}tool.txt",
        "nested/".repeat(scanner::MAX_SCAN_DEPTH + 2)
    );
    let document = "---\nname: demo\n---\nhealthy";
    let bytes = archive(&[
        ("root/skills/demo/SKILL.md", document),
        ("root/.codex/skills/demo/SKILL.md", document),
        (&deep, "resource"),
    ]);
    let marketplace::CatalogPreparation::SelectionRequired(candidates) =
        marketplace::prepare_catalog_archive("owner/repo", "demo", &bytes, None).unwrap()
    else {
        panic!("expected candidate error")
    };
    assert!(candidates[0]
        .error
        .as_ref()
        .unwrap()
        .contains(".codex/skills/demo"));
    assert!(matches!(
        marketplace::prepare_catalog_archive("owner/repo", "demo", &bytes, Some("skills/demo"))
            .unwrap(),
        marketplace::CatalogPreparation::Ready(_)
    ));
}

#[test]
fn identical_candidates_report_matching_hashes() {
    let document = "---\nname: demo\n---\nbody";
    let bytes = archive(&[
        ("root/skills/a/SKILL.md", document),
        ("root/skills/b/SKILL.md", document),
    ]);
    let marketplace::CatalogPreparation::SelectionRequired(candidates) =
        marketplace::prepare_catalog_archive("owner/repo", "demo", &bytes, None).unwrap()
    else {
        panic!("expected selection")
    };
    assert_eq!(candidates[0].content_hash, candidates[1].content_hash);
}

#[test]
#[serial]
#[ignore = "requires live skills.sh and GitHub network"]
fn live_search_download_and_install_in_temporary_home() {
    let hits = marketplace::search("defuddle").unwrap();
    let hit = hits
        .iter()
        .find(|s| s.source == "kepano/obsidian-skills" && s.skill_id == "defuddle")
        .unwrap();
    let prepared = marketplace::prepare(&hit.source, &hit.skill_id).unwrap();
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let installed = studio
        .install_catalog_skill(&mut config, &prepared)
        .unwrap();
    assert!(installed.source_path.join("SKILL.md").is_file());
    assert_eq!(
        studio.scan_skills(&studio.load_config().unwrap()).unwrap()[0].source_ids,
        vec!["studio"]
    );
    assert!(config.registrations.is_empty());
    println!("Live search, repository resolution, complete install and persisted Studio origin passed: {}/{}", hit.source, hit.skill_id);
}

#[test]
#[serial]
fn local_collection_import_preserves_source_and_persists_studio_origin() {
    let env = Env::new();
    let collection = env.path().join("personal-agent-skills");
    let source = env.write_simple_skill(&collection.join("nested"), "demo");
    std::fs::write(source.join("reference.txt"), "keep me").unwrap();
    env.write_simple_skill(&collection, "second");
    env.write_simple_skill(&collection.join(".git"), "hidden");
    let found = marketplace::discover_local(&collection).unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(marketplace::discover_local(&source).unwrap().len(), 1);
    let before = scanner::dir_content_hash(&source).unwrap();
    let prepared = marketplace::prepare_local(&source).unwrap();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let installed = studio
        .install_catalog_skill(&mut config, &prepared)
        .unwrap();
    assert_eq!(installed.source_path, env.hub().join("demo"));
    assert_eq!(scanner::dir_content_hash(&source).unwrap(), before);
    assert_eq!(
        scanner::dir_content_hash(&installed.source_path).unwrap(),
        before
    );
    let view = studio
        .scan_skills(&studio.load_config().unwrap())
        .unwrap()
        .remove(0);
    assert_eq!(view.source_ids, vec!["studio"]);
    assert_eq!(
        view.installation.unwrap().source,
        format!("local:{}", source.canonicalize().unwrap().display())
    );
    assert!(config.registrations.is_empty());
    studio
        .install_catalog_skill(&mut config, &prepared)
        .unwrap();
    assert_eq!(studio.scan_skills(&config).unwrap().len(), 1);
}

#[test]
#[serial]
fn local_import_rejects_invalid_and_conflicting_skills() {
    let env = Env::new();
    let source = env.write_skill(
        &env.path().join("local"),
        "demo",
        "---\nname: [invalid\n---",
    );
    assert!(marketplace::discover_local(&source).unwrap()[0]
        .error
        .is_some());
    assert!(marketplace::prepare_local(&source).is_err());
    env.write_simple_skill(&env.path().join("local"), "demo");
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let existing = env.write_simple_skill(&env.hub(), "demo");
    let before = scanner::dir_content_hash(&existing).unwrap();
    let prepared = marketplace::prepare_local(&source).unwrap();
    assert!(studio
        .install_catalog_skill(&mut config, &prepared)
        .is_err());
    assert_eq!(before, scanner::dir_content_hash(&existing).unwrap());
    assert!(config.skill_installations.is_empty());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/hosts", source.join("outside")).unwrap();
        assert!(marketplace::prepare_local(&source).is_err());
    }
}
