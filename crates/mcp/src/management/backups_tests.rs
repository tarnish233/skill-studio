use super::*;
use crate::management::{read, remove_sources, restore_backup, BackupFile, Target};
use crate::native;

fn codex(path: &Path, servers: &[&str]) {
    let mut text = String::from("# keep\n");
    for server in servers {
        text.push_str(&format!("[mcp_servers.{server}]\ncommand='{server}'\n"));
    }
    std::fs::write(path, text).unwrap();
}

/// 移除 Codex 配置里的一个原有 MCP：一次会留下备份的写入
fn remove(dir: &Path, path: &Path, key: &str) {
    let text = std::fs::read_to_string(path).unwrap();
    let expected = native::entry(&text, "codex", None, key).unwrap();
    remove_sources(
        dir,
        vec![Target {
            agent: "codex".into(),
            path: path.to_path_buf(),
            project: None,
            key: key.into(),
            expected,
        }],
    )
    .unwrap();
}

fn ids(dir: &Path) -> Vec<String> {
    read(dir)
        .unwrap()
        .backups
        .into_iter()
        .map(|b| b.id)
        .collect()
}

fn status_of(dir: &Path, id: &str) -> (Status, Option<String>) {
    let view = list(dir).unwrap().into_iter().find(|b| b.id == id).unwrap();
    (view.status, view.reason)
}

/// 配置文件所在目录里带 studio-backup 的文件名
fn beside(path: &Path) -> Vec<String> {
    std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("studio-backup"))
        .collect()
}

fn legacy_record(dir: &Path, id: &str, path: &Path, backup: &Path, catalog: &Path) {
    let mut current = read(dir).unwrap_or_default();
    current.backups.push(Backup {
        id: id.into(),
        files: vec![BackupFile {
            path: path.to_path_buf(),
            backup_path: Some(backup.to_path_buf()),
            before_hash: Some("before".into()),
            after_hash: Some("after".into()),
        }],
        created_at: 1,
        catalog_backup_path: catalog.to_path_buf(),
        before_catalog_hash: "before".into(),
        after_catalog_hash: "after".into(),
    });
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("catalog.json"),
        serde_json::to_string_pretty(&current).unwrap(),
    )
    .unwrap();
}

#[test]
fn backups_live_in_the_studio_dir_instead_of_beside_agent_configs() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    codex(&path, &["one", "two"]);
    let original = std::fs::read_to_string(&path).unwrap();
    remove(&dir, &path, "one");

    assert!(
        beside(&path).is_empty(),
        "配置文件旁边不应再出现备份：{:?}",
        beside(&path)
    );
    let backup = read(&dir).unwrap().backups[0].clone();
    let folder = dir.join(DIR).join(&backup.id);
    assert!(backup.catalog_backup_path.starts_with(&folder));
    let copy = backup.files[0].backup_path.clone().unwrap();
    assert!(copy.starts_with(&folder), "{}", copy.display());
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), original);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // 备份可能含凭据（如 headers、env），和原配置一样只给自己读
        let mode = std::fs::metadata(&copy).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    restore_backup(&dir, &backup.id).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn each_backup_says_whether_it_can_still_be_restored_along_the_undo_chain() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    codex(&path, &["one", "two", "three"]);
    remove(&dir, &path, "one");
    remove(&dir, &path, "two");
    let [older, newer] = <[String; 2]>::try_from(ids(&dir)).unwrap();

    assert_eq!(status_of(&dir, &newer), (Status::Restorable, None));
    // 较早的一次要等较新的恢复后才能恢复，不能算作「无法恢复」
    assert_eq!(
        status_of(&dir, &older),
        (Status::AfterNewer, Some("需先恢复更新的备份".into()))
    );
    restore_backup(&dir, &newer).unwrap();
    assert_eq!(status_of(&dir, &older).0, Status::Restorable);

    // Agent 或用户之后又改了这个文件：沿这条链已无法回去
    std::fs::write(&path, "# edited by user\n").unwrap();
    for view in list(&dir).unwrap() {
        assert_eq!(view.status, Status::Unrestorable, "{}", view.id);
        assert_eq!(
            view.reason.as_deref(),
            Some(format!("配置已在备份后修改：{}", path.display()).as_str())
        );
    }
    assert_eq!(delete(&dir, &Selection::Unrestorable).unwrap(), 2);
    assert!(read(&dir).unwrap().backups.is_empty());
    assert!(!dir.join(DIR).join(&older).exists());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# edited by user\n",
        "删除备份不能动配置本身"
    );
}

#[test]
fn a_lost_backup_file_ends_the_chain_and_cleanup_keeps_what_still_restores() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    let other = root.path().join("other.toml");
    codex(&path, &["one", "two", "three"]);
    codex(&other, &["solo"]);
    remove(&dir, &path, "one");
    remove(&dir, &path, "two");
    remove(&dir, &other, "solo");
    let [first, second, third] = <[String; 3]>::try_from(ids(&dir)).unwrap();
    let catalog = read(&dir).unwrap();
    std::fs::remove_file(catalog.backups[1].files[0].backup_path.as_ref().unwrap()).unwrap();

    assert_eq!(
        status_of(&dir, &second),
        (Status::Unrestorable, Some("备份文件已不存在".into()))
    );
    assert_eq!(status_of(&dir, &first).0, Status::Unrestorable);
    assert_eq!(status_of(&dir, &third), (Status::Restorable, None));
    assert_eq!(delete(&dir, &Selection::Unrestorable).unwrap(), 2);
    assert_eq!(ids(&dir), vec![third.clone()]);
    restore_backup(&dir, &third).unwrap();
}

#[test]
fn prune_keeps_only_the_newest_backups() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    codex(&path, &["a", "b", "c", "d"]);
    for key in ["a", "b", "c"] {
        remove(&dir, &path, key);
    }
    let all = ids(&dir);
    assert_eq!(prune(&dir, 1).unwrap(), 2);
    assert_eq!(ids(&dir), vec![all[2].clone()]);
    assert!(!dir.join(DIR).join(&all[0]).exists());
    assert!(!dir.join(DIR).join(&all[1]).exists());
    assert!(dir.join(DIR).join(&all[2]).exists());
    assert_eq!(prune(&dir, 1).unwrap(), 0);
    // 保留 0 份：一份都不留
    assert_eq!(prune(&dir, 0).unwrap(), 1);
    assert!(read(&dir).unwrap().backups.is_empty());
    assert_eq!(std::fs::read_dir(dir.join(DIR)).unwrap().count(), 0);
}

#[test]
fn prune_drops_backups_that_can_no_longer_restore_before_older_ones() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    let other = root.path().join("other.toml");
    codex(&other, &["solo"]);
    codex(&path, &["one", "two", "three"]);
    remove(&dir, &other, "solo");
    remove(&dir, &path, "one");
    remove(&dir, &path, "two");
    let [oldest, ..] = <[String; 3]>::try_from(ids(&dir)).unwrap();
    // 之后的两次备份所在的配置被改过，回不去了；最早那次仍然能撤销
    std::fs::write(&path, "# edited by user\n").unwrap();
    assert_eq!(status_of(&dir, &oldest).0, Status::Restorable);

    assert_eq!(prune(&dir, 1).unwrap(), 2);
    assert_eq!(
        ids(&dir),
        vec![oldest.clone()],
        "留下的应当是还能恢复的那次"
    );
    restore_backup(&dir, &oldest).unwrap();
}

#[test]
fn deleting_chosen_backups_keeps_the_others() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    codex(&path, &["one", "two", "three"]);
    remove(&dir, &path, "one");
    remove(&dir, &path, "two");
    let [older, newer] = <[String; 2]>::try_from(ids(&dir)).unwrap();
    assert_eq!(
        delete(&dir, &Selection::Ids(vec![older.clone(), "missing".into()])).unwrap(),
        1
    );
    assert_eq!(ids(&dir), vec![newer.clone()]);
    assert_eq!(status_of(&dir, &newer), (Status::Restorable, None));
    assert_eq!(delete(&dir, &Selection::All).unwrap(), 1);
    assert!(read(&dir).unwrap().backups.is_empty());
}

#[test]
fn deleting_an_old_record_removes_only_files_studio_wrote_for_it() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    codex(&path, &["one"]);
    // 旧版本把备份写在配置文件旁边，记录里留着这些路径
    let id = "6e643728-7796-456c-8ab0-580202508649";
    let beside_config = path.with_extension(format!("studio-backup-{id}-0"));
    let old_catalog = dir.join(format!("catalog.studio-backup-{id}.json"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&beside_config, "old").unwrap();
    std::fs::write(&old_catalog, "{}").unwrap();
    legacy_record(&dir, id, &path, &beside_config, &old_catalog);
    // 记录被改坏、指向别的文件时不能照着删
    let precious = root.path().join("notes.txt");
    std::fs::write(&precious, "keep").unwrap();
    legacy_record(
        &dir,
        "1f0e0d0c-0000-4000-8000-00000000000a",
        &path,
        &precious,
        &precious,
    );

    assert_eq!(delete(&dir, &Selection::All).unwrap(), 2);
    assert!(!beside_config.exists());
    assert!(!old_catalog.exists());
    assert_eq!(std::fs::read_to_string(&precious).unwrap(), "keep");
    assert!(path.exists());
}

#[test]
fn untracked_backups_beside_configs_are_listed_and_only_deleted_on_request() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    let claude = root.path().join(".claude.json");
    codex(&path, &["one", "two"]);
    std::fs::write(&claude, "{}").unwrap();
    remove(&dir, &path, "one");
    // 旧版网关写入留下的（不带序号）和记录已经丢失的
    let gateway = path.with_extension("studio-backup-6e643728-7796-456c-8ab0-580202508649");
    let orphan = claude.with_extension("studio-backup-cef54bcb-79f4-476a-8a22-ad2e1a1659f3-0");
    // 仍有记录的旧备份跟着记录走，不算在这里
    let tracked = path.with_extension("studio-backup-0229c241-3070-406d-b2da-660327f3674a-0");
    let unrelated = root.path().join("config.toml.bak");
    let named_like_one = path.with_extension("studio-backup-mine.toml");
    let folder = path.with_extension("studio-backup-0be234cc-340c-4f44-a27d-b3fb616703da");
    for file in [&gateway, &orphan, &tracked, &unrelated, &named_like_one] {
        std::fs::write(file, "x").unwrap();
    }
    std::fs::create_dir(&folder).unwrap();
    legacy_record(
        &dir,
        "0229c241-3070-406d-b2da-660327f3674a",
        &path,
        &tracked,
        &dir.join("catalog.studio-backup-0229c241-3070-406d-b2da-660327f3674a.json"),
    );
    let configs = vec![path.clone(), claude.clone(), path.clone()];

    let found: Vec<PathBuf> = legacy(&dir, &configs)
        .unwrap()
        .into_iter()
        .map(|b| b.path)
        .collect();
    let mut expected = vec![gateway.clone(), orphan.clone()];
    expected.sort();
    assert_eq!(found, expected);
    // 只列出，不会自动删
    prune(&dir, 0).unwrap();
    assert!(gateway.exists() && orphan.exists());

    assert_eq!(delete_legacy(&dir, &configs).unwrap(), 2);
    assert!(!gateway.exists() && !orphan.exists());
    assert!(unrelated.exists() && named_like_one.exists() && folder.is_dir());
    assert!(legacy(&dir, &configs).unwrap().is_empty());
}

#[test]
fn cleanup_sweeps_orphaned_backup_folders_inside_the_studio_dir() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    codex(&path, &["one", "two"]);
    remove(&dir, &path, "one");
    let kept = ids(&dir).remove(0);
    // 写入中途退出留下的、旧版本留在 Studio 目录里的
    let orphan = dir.join(DIR).join("1f0e0d0c-0000-4000-8000-00000000000b");
    std::fs::create_dir_all(&orphan).unwrap();
    std::fs::write(orphan.join("0-config.toml"), "x").unwrap();
    let old_catalog = dir.join("catalog.studio-backup-1f0e0d0c-0000-4000-8000-00000000000c.json");
    std::fs::write(&old_catalog, "{}").unwrap();
    let gateway = dir.join(DIR).join(GATEWAY);
    std::fs::create_dir_all(&gateway).unwrap();
    std::fs::write(gateway.join("codex-config.toml"), "x").unwrap();

    assert_eq!(prune(&dir, 10).unwrap(), 0);
    assert!(!orphan.exists());
    assert!(!old_catalog.exists());
    assert!(gateway.join("codex-config.toml").exists());
    assert!(dir.join(DIR).join(&kept).exists());
    assert_eq!(status_of(&dir, &kept), (Status::Restorable, None));
}

#[test]
fn gateway_writes_keep_one_backup_per_file_inside_the_studio_dir() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    let path = root.path().join("config.toml");
    codex(&path, &["manual"]);
    let original = std::fs::read_to_string(&path).unwrap();
    let apply = |remove: bool| {
        crate::integration::apply(
            &path,
            "codex",
            "srv",
            "/app/studio",
            dir.to_str().unwrap(),
            remove,
        )
        .unwrap()
    };
    apply(false);
    assert!(beside(&path).is_empty(), "{:?}", beside(&path));
    let folder = dir.join(DIR).join(GATEWAY);
    let files: Vec<PathBuf> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), original);
    let installed = std::fs::read_to_string(&path).unwrap();
    apply(true);
    // 同一个文件只留最近一次写入前的内容
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
    assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), installed);
    assert!(beside(&path).is_empty());
}
