#[path = "support.rs"]
mod support;
use serial_test::serial;
use std::fs;
use support::Env;

#[test]
#[serial]
fn full_backups_show_which_one_restores_and_only_unused_ones_can_be_deleted() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let source = env.write_simple_skill(&env.claude_skills(), "demo");
    let view = studio.scan_skills(&c).unwrap().remove(0);
    let adopted = studio.adopt_to_hub(&mut c, &view.skill.id).unwrap();
    let released = studio.release_from_hub(&mut c, &adopted.id).unwrap();
    let root = studio.store().dir().join("skill-backups");
    // 删除 skill 时留下的备份是另一种格式，归「已备份的 skill」管，这里不列也不删
    for stash in [".tmpStash", "legacy-x"] {
        fs::create_dir_all(root.join(stash).join("payload")).unwrap();
        fs::write(root.join(stash).join("record.json"), "{}").unwrap();
    }

    let backups = studio.hub_backups(&c).unwrap();
    assert_eq!(backups.len(), 2, "{backups:?}");
    let (release, adopt) = (&backups[0], &backups[1]);
    assert_eq!(release.operation.as_deref(), Some("release"));
    assert_eq!(adopt.operation.as_deref(), Some("adopt"));
    for backup in &backups {
        assert_eq!(backup.name, "demo");
        assert_eq!(backup.original_path, source);
        assert!(backup.size > 0);
    }
    // 来源记录引用纳入托管时的那份：退出应用管理、再次移出 Hub 时要用它还原
    let record = &c.skill_provenance[&released.id];
    assert_eq!(root.join(&adopt.id), record.backup_path);
    assert!(adopt.in_use);
    assert!(!release.in_use);

    let error = studio
        .delete_hub_backups(&c, &[release.id.clone(), adopt.id.clone()])
        .unwrap_err();
    assert!(error.to_string().contains("仍在使用"), "{error}");
    assert!(root.join(&release.id).is_dir(), "有一份不能删时整批都不删");
    for bad in [
        "",
        ".",
        "..",
        "../skills",
        ".tmpStash",
        "legacy-x",
        "missing",
    ] {
        assert!(
            studio.delete_hub_backups(&c, &[bad.to_string()]).is_err(),
            "{bad:?}"
        );
    }
    assert!(root.join(".tmpStash/record.json").is_file());
    assert!(root.join("legacy-x/record.json").is_file());

    assert_eq!(
        studio
            .delete_hub_backups(&c, std::slice::from_ref(&release.id))
            .unwrap(),
        1
    );
    assert!(!root.join(&release.id).exists());
    assert!(record.backup_path.join("content/SKILL.md").is_file());
    let left = studio.hub_backups(&c).unwrap();
    assert_eq!(left.len(), 1);
    assert!(left[0].in_use);
}

#[test]
#[serial]
fn the_same_full_backup_listed_twice_is_deleted_once() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    env.write_simple_skill(&env.claude_skills(), "demo");
    let view = studio.scan_skills(&c).unwrap().remove(0);
    let adopted = studio.adopt_to_hub(&mut c, &view.skill.id).unwrap();
    studio.release_from_hub(&mut c, &adopted.id).unwrap();
    let unused = studio
        .hub_backups(&c)
        .unwrap()
        .into_iter()
        .find(|b| !b.in_use)
        .unwrap();
    // 重复的 ID 只算一份：第二次删除不该因为「已经不在了」整批报错
    assert_eq!(
        studio
            .delete_hub_backups(&c, &[unused.id.clone(), unused.id.clone()])
            .unwrap(),
        1
    );
    assert!(!studio
        .store()
        .dir()
        .join("skill-backups")
        .join(&unused.id)
        .exists());
}
