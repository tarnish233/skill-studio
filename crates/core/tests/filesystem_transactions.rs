//! Permission and recovery boundaries use disposable directories, never real Agent files.
#![cfg(unix)]

#[path = "support.rs"]
mod support;

use serial_test::serial;
use skill_studio_core::{
    models::{
        project::{ProjectBinding, ProjectSelection},
        skill::{skill_id_for, LinkMode},
    },
    services::{linker, scanner, store::Store},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use support::Env;

// Restore test-only permissions even when a RED assertion panics, so TempDir can clean up.
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        fn unlock(path: &Path) {
            let Ok(meta) = path.symlink_metadata() else {
                return;
            };
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return;
            }
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    unlock(&entry.path());
                }
            }
        }
        unlock(&self.0);
    }
}

fn read_only_tree(path: &Path, body: &str) {
    fs::create_dir_all(path.join("references")).unwrap();
    fs::write(path.join("SKILL.md"), body).unwrap();
    fs::write(path.join("references/guide.md"), "reference").unwrap();
    for dir in [path.to_path_buf(), path.join("references")] {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o555)).unwrap();
    }
}

fn assert_copy(source: &Path, dest: &Path) {
    assert!(dest.join(scanner::COPY_SIDECAR).is_file());
    assert_eq!(
        scanner::dir_content_hash(source).unwrap(),
        scanner::dir_content_hash(dest).unwrap()
    );
    for root in [source, dest] {
        for dir in [root.to_path_buf(), root.join("references")] {
            assert_eq!(
                fs::metadata(dir).unwrap().permissions().mode() & 0o777,
                0o555
            );
        }
    }
}

fn assert_no_journals(root: &Path) {
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            !name.contains(".staging-") && !name.contains(".backup-"),
            "leftover: {name}"
        );
        assert!(
            !(name.starts_with(".skill-studio-replace-") && name.ends_with(".json")),
            "journal: {name}"
        );
    }
}

#[test]
fn read_only_copy_can_be_registered_replaced_and_removed() {
    let temp = tempfile::tempdir().unwrap();
    let _cleanup = Cleanup(temp.path().to_path_buf());
    let source = temp.path().join("source");
    let dest = temp.path().join("agent/demo");
    read_only_tree(&source, "---\nname: demo\n---\nbody");
    for _ in 0..2 {
        linker::register(&source, &dest, LinkMode::Copy, "demo", false).unwrap();
        assert_copy(&source, &dest);
        assert_no_journals(dest.parent().unwrap());
    }
    assert!(linker::unregister(&source, &dest, false).unwrap());
    assert!(!dest.exists());
    assert!(source.join("references/guide.md").is_file());
}

#[test]
#[serial]
fn read_only_group_copy_can_be_activated_and_withdrawn() {
    let env = Env::new();
    let _cleanup = Cleanup(env.path().to_path_buf());
    let source = env.hub().join("demo");
    read_only_tree(&source, "---\nname: demo\n---\nbody");
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    studio
        .save_group(&mut config, "g".into(), "G", vec![skill_id_for(&source)])
        .unwrap();
    studio
        .activate_agent_groups(&mut config, "codex", &["g".into()])
        .unwrap();
    assert_copy(&source, &env.codex_skills().join("demo"));
    studio
        .activate_agent_groups(&mut config, "codex", &[])
        .unwrap();
    assert!(!env.codex_skills().join("demo").exists());
    assert!(!studio.store().pending_transactions());
    studio.load_config().unwrap();
}

#[test]
#[serial]
fn read_only_project_copy_can_be_applied_and_withdrawn() {
    let env = Env::new();
    let _cleanup = Cleanup(env.path().to_path_buf());
    let source = env.hub().join("demo");
    read_only_tree(&source, "---\nname: demo\n---\nbody");
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let root = env.path().join("project");
    fs::create_dir(&root).unwrap();
    config
        .projects
        .push(ProjectBinding::new("p".into(), "P".into(), root.clone()));
    let selection = ProjectSelection {
        agent_ids: vec!["codex".into()],
        skill_ids: vec![skill_id_for(&source)],
        group_ids: vec![],
        link_mode: LinkMode::Copy,
    };
    studio
        .write_project(&mut config, "p", Some(selection))
        .unwrap();
    assert_copy(&source, &root.join(".agents/skills/demo"));
    studio.set_project_enabled(&mut config, "p", false).unwrap();
    assert!(!root.join(".agents/skills/demo").exists());
    assert!(!studio.store().pending_transactions());
    studio.load_config().unwrap();
}

#[test]
fn recovery_cleans_read_only_trees_without_following_nested_links() {
    for committed in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let _cleanup = Cleanup(temp.path().to_path_buf());
        let store = Store::new(temp.path().join("store"));
        fs::create_dir(store.dir()).unwrap();
        let target = temp.path().join("target");
        let backup = temp.path().join("backup");
        let outside = temp.path().join("outside");
        read_only_tree(&target, "old");
        read_only_tree(&outside, "untouched");
        // Seed the post-reserve journal portably: older macOS cannot rename a 0555 root.
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        fs::rename(&target, &backup).unwrap();
        fs::set_permissions(&backup, fs::Permissions::from_mode(0o555)).unwrap();
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&outside, target.join("outside")).unwrap();
        read_only_tree(&target, "new");
        let journal = store.dir().join("migration.json");
        fs::write(
            &journal,
            serde_json::to_vec(&serde_json::json!({
                "journal": journal, "committed": committed,
                "entries": [{"target": target, "backup": backup, "existed": true}]
            }))
            .unwrap(),
        )
        .unwrap();
        store.load().unwrap();
        assert!(!journal.exists());
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            if committed { "new" } else { "old" }
        );
        assert_eq!(
            fs::read_to_string(outside.join("SKILL.md")).unwrap(),
            "untouched"
        );
        assert_eq!(
            fs::metadata(&outside).unwrap().permissions().mode() & 0o777,
            0o555
        );
        assert_no_journals(temp.path());
    }
}

#[test]
fn rollback_continues_after_an_error_and_keeps_the_journal_until_fully_recovered() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::new(temp.path().join("store"));
    fs::create_dir(store.dir()).unwrap();
    let journal = store.dir().join("migration.json");
    let target = temp.path().join("target");
    let backup = temp.path().join("backup");
    let blocked = temp.path().join("blocked");
    fs::write(&target, "replacement").unwrap();
    fs::write(&backup, "original").unwrap();
    fs::write(&blocked, "not a directory").unwrap();
    fs::write(&journal, serde_json::to_vec(&serde_json::json!({
        "journal": journal, "committed": false,
        "entries": [
            {"target": target, "backup": backup, "existed": true},
            {"target": blocked.join("child"), "backup": temp.path().join("unused"), "existed": false}
        ]
    })).unwrap()).unwrap();
    assert!(store.load().is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "original");
    assert!(
        journal.is_file(),
        "an incomplete rollback must retain its recovery map"
    );
    fs::remove_file(&blocked).unwrap();
    store.load().unwrap();
    assert!(!journal.exists());
}

#[test]
#[serial]
fn config_symlink_survives_transactional_writes_and_interrupted_recovery() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let source = env.write_simple_skill(&env.hub(), "demo");
    studio
        .save_group(&mut config, "g".into(), "G", vec![skill_id_for(&source)])
        .unwrap();
    studio.save_config(&config).unwrap();
    let logical = studio.store().config_path();
    let physical = env.path().join("dotfiles/config.json");
    fs::create_dir_all(physical.parent().unwrap()).unwrap();
    fs::rename(&logical, &physical).unwrap();
    std::os::unix::fs::symlink(&physical, &logical).unwrap();
    config.settings.language = "en".into();
    studio.save_config(&config).unwrap();
    assert!(
        logical.symlink_metadata().unwrap().file_type().is_symlink(),
        "ordinary atomic saves already preserve dotfiles links"
    );
    for groups in [vec!["g".into()], vec![]] {
        studio
            .activate_agent_groups(&mut config, "codex", &groups)
            .unwrap();
        assert!(logical.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read_link(&logical).unwrap(), physical);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&physical).unwrap()).unwrap(),
            serde_json::to_value(&config).unwrap()
        );
        assert!(!studio.store().pending_transactions());
    }
    let before = fs::read(&physical).unwrap();
    let backup = physical.with_extension("backup");
    fs::rename(&physical, &backup).unwrap();
    fs::write(&physical, "incomplete config").unwrap();
    let journal = studio.store().dir().join("migration.json");
    fs::write(
        &journal,
        serde_json::to_vec(&serde_json::json!({
            "journal": journal, "committed": false,
            "entries": [{"target": physical, "backup": backup, "existed": true}]
        }))
        .unwrap(),
    )
    .unwrap();
    studio.load_config().unwrap();
    assert!(logical.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(fs::read(&physical).unwrap(), before);
    assert!(!journal.exists());
}

#[test]
#[serial]
fn dangling_config_symlink_is_rejected_without_replacing_it_or_leaving_deployments() {
    let env = Env::new();
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let source = env.write_simple_skill(&env.hub(), "demo");
    studio
        .save_group(&mut config, "g".into(), "G", vec![skill_id_for(&source)])
        .unwrap();
    let logical = studio.store().config_path();
    let missing = env.path().join("dotfiles/missing.json");
    std::os::unix::fs::symlink(&missing, &logical).unwrap();
    assert!(studio
        .activate_agent_groups(&mut config, "codex", &["g".into()])
        .is_err());
    assert!(logical.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(fs::read_link(&logical).unwrap(), missing);
    assert!(!missing.exists());
    assert!(!env.codex_skills().join("demo").exists());
    assert!(!studio.store().pending_transactions());
}
