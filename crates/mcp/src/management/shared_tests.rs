//! 桌面应用和命令行同时改 MCP 托管状态：启停只动目标分组、保存前核对、查看不写文件、
//! 等锁有上限。
use super::*;
use crate::native;

fn hub(id: &str) -> Entry {
    Entry {
        id: id.into(),
        name: id.into(),
        mode: "direct".into(),
        definition: json!({"type":"stdio","command":id}),
        oauth: false,
        client_id: None,
        scopes: vec![],
        bindings: vec![],
    }
}

fn group(id: &str, ids: &[&str]) -> groups::Group {
    groups::Group {
        id: id.into(),
        name: id.into(),
        entry_ids: ids.iter().map(|s| s.to_string()).collect(),
        references: vec![],
        sort_order: 0,
    }
}

fn snapshot(name: &str, ids: &[&str]) -> groups::GroupSnapshot {
    groups::GroupSnapshot {
        name: name.into(),
        entry_ids: ids.iter().map(|s| s.to_string()).collect(),
    }
}

struct Env {
    root: tempfile::TempDir,
    dir: PathBuf,
}

impl Env {
    /// Hub 里有 alpha、beta 两个直连的 MCP
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("studio");
        for id in ["alpha", "beta"] {
            save(&dir, hub(id), vec![], Path::new("/app"), None).unwrap();
        }
        Self { root, dir }
    }
    fn config(&self, agent: &str) -> PathBuf {
        self.root.path().join(format!("{agent}.config"))
    }
    fn set(&self, agent: &str, id: &str, enabled: bool) -> Result<Vec<groups::Untouched>> {
        let path = self.config(agent);
        groups::set_active(
            &self.dir,
            agent,
            id,
            enabled,
            || Ok(path),
            Path::new("/app"),
        )
    }
    fn active(&self, agent: &str) -> Vec<String> {
        groups::active_ids(&read(&self.dir).unwrap(), agent)
    }
    fn deployed(&self, agent: &str, key: &str) -> bool {
        let text = std::fs::read_to_string(self.config(agent)).unwrap_or_default();
        native::entry(&text, agent, None, key).unwrap().is_some()
    }
    fn catalog_text(&self) -> String {
        std::fs::read_to_string(self.dir.join("catalog.json")).unwrap()
    }
}

#[test]
fn enabling_a_group_keeps_the_groups_another_process_just_enabled() {
    let env = Env::new();
    groups::save_group(&env.dir, group("a", &["alpha"]), vec![], None).unwrap();
    groups::save_group(&env.dir, group("b", &["beta"]), vec![], None).unwrap();
    // 桌面应用打开页面时 Codex 还没有启用分组；这期间命令行为它启用了 b
    env.set("codex", "b", true).unwrap();
    // 桌面应用再启用 a：只加上 a，不按打开页面时的旧列表把 b 撤掉
    env.set("codex", "a", true).unwrap();
    assert_eq!(env.active("codex"), ["b", "a"]);
    assert!(env.deployed("codex", "alpha"));
    assert!(env.deployed("codex", "beta"));
}

#[test]
fn enabling_twice_or_disabling_an_inactive_group_writes_nothing() {
    let env = Env::new();
    groups::save_group(&env.dir, group("a", &["alpha"]), vec![], None).unwrap();
    groups::save_group(&env.dir, group("b", &["beta"]), vec![], None).unwrap();
    env.set("codex", "a", true).unwrap();
    let catalog = env.catalog_text();
    let native = std::fs::read_to_string(env.config("codex")).unwrap();
    assert!(env.set("codex", "a", true).unwrap().is_empty());
    assert!(env.set("codex", "b", false).unwrap().is_empty());
    assert_eq!(env.catalog_text(), catalog);
    assert_eq!(
        std::fs::read_to_string(env.config("codex")).unwrap(),
        native
    );
}

#[test]
fn disabling_one_group_keeps_the_others_and_the_members_they_share() {
    let env = Env::new();
    groups::save_group(&env.dir, group("a", &["alpha", "beta"]), vec![], None).unwrap();
    groups::save_group(&env.dir, group("b", &["beta"]), vec![], None).unwrap();
    env.set("codex", "a", true).unwrap();
    env.set("codex", "b", true).unwrap();
    env.set("codex", "a", false).unwrap();
    assert_eq!(env.active("codex"), ["b"]);
    assert!(!env.deployed("codex", "alpha"));
    // b 还要用 beta，不能跟着 a 撤掉
    assert!(env.deployed("codex", "beta"));
    env.set("codex", "b", false).unwrap();
    assert!(env.active("codex").is_empty());
    assert!(!env.deployed("codex", "beta"));
}

#[test]
fn reapplying_writes_the_latest_members_and_skips_agents_without_groups() {
    let env = Env::new();
    groups::save_group(&env.dir, group("a", &["alpha"]), vec![], None).unwrap();
    env.set("codex", "a", true).unwrap();
    groups::save_group(
        &env.dir,
        group("a", &["alpha", "beta"]),
        vec![],
        Some(&snapshot("a", &["alpha"])),
    )
    .unwrap();
    assert!(!env.deployed("codex", "beta"));
    let path = env.config("codex");
    groups::reapply(&env.dir, "codex", || Ok(path), Path::new("/app")).unwrap();
    assert!(env.deployed("codex", "beta"));
    // 没有启用分组的 Agent：不解析写入位置，也不写
    groups::reapply(
        &env.dir,
        "claude",
        || bail!("不该解析写入位置"),
        Path::new("/app"),
    )
    .unwrap();
}

#[test]
fn saving_a_group_someone_else_changed_or_deleted_is_refused() {
    let env = Env::new();
    groups::save_group(&env.dir, group("a", &["alpha"]), vec![], None).unwrap();
    // 打开编辑后，命令行给分组加了 beta
    let opened = snapshot("a", &["alpha"]);
    groups::save_group(
        &env.dir,
        group("a", &["alpha", "beta"]),
        vec![],
        Some(&opened),
    )
    .unwrap();
    let error = groups::save_group(&env.dir, group("a", &[]), vec![], Some(&opened))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(skill_studio_core::services::store::CHANGED_ELSEWHERE),
        "{error}"
    );
    assert_eq!(
        read(&env.dir).unwrap().groups[0].entry_ids,
        ["alpha", "beta"]
    );
    // 名称前后的空格和成员顺序不算修改
    groups::save_group(
        &env.dir,
        group("a", &["beta"]),
        vec![],
        Some(&snapshot(" a ", &["beta", "alpha"])),
    )
    .unwrap();
    groups::remove_group(&env.dir, "a").unwrap();
    let error = groups::save_group(
        &env.dir,
        group("a", &["alpha"]),
        vec![],
        Some(&snapshot("a", &["beta"])),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "分组已被别处删除");
    assert!(read(&env.dir).unwrap().groups.is_empty());
}

#[test]
fn a_group_in_use_cannot_be_emptied_until_it_is_stopped() {
    let env = Env::new();
    groups::save_group(&env.dir, group("a", &["alpha"]), vec![], None).unwrap();
    env.set("codex", "a", true).unwrap();
    env.set("claude", "a", true).unwrap();
    let error = groups::save_group(&env.dir, group("a", &[]), vec![], None)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("正在 Claude Code、Codex 上使用，清空成员前请先为这些 Agent 停用"),
        "{error}"
    );
    assert_eq!(read(&env.dir).unwrap().groups[0].entry_ids, ["alpha"]);
    env.set("codex", "a", false).unwrap();
    env.set("claude", "a", false).unwrap();
    groups::save_group(&env.dir, group("a", &[]), vec![], None).unwrap();
}

#[test]
fn saving_a_project_someone_else_changed_is_refused() {
    let env = Env::new();
    let root = env.root.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let input = |ids: &[&str], claude_local| projects::Input {
        root: root.clone(),
        entry_ids: ids.iter().map(|s| s.to_string()).collect(),
        group_ids: vec![],
        claude_local,
    };
    let overrides = std::collections::HashMap::new();
    let exe = Path::new("/app");
    // 还没有记录的项目，打开时就是什么都没选
    let empty = projects::Snapshot::default();
    projects::save(
        &env.dir,
        "p",
        input(&["alpha"], false),
        vec![],
        &overrides,
        exe,
        Some(&empty),
    )
    .unwrap();
    let error = projects::save(
        &env.dir,
        "p",
        input(&["beta"], false),
        vec![],
        &overrides,
        exe,
        Some(&empty),
    )
    .unwrap_err()
    .to_string();
    assert!(error.starts_with("项目已被别处修改"), "{error}");
    assert_eq!(read(&env.dir).unwrap().projects["p"].entry_ids, ["alpha"]);
    let opened = projects::Snapshot {
        entry_ids: vec!["alpha".into()],
        group_ids: vec![],
        claude_local: true,
    };
    // 「Claude Code 写到项目本地」也算项目的设置
    assert!(projects::save(
        &env.dir,
        "p",
        input(&["beta"], false),
        vec![],
        &overrides,
        exe,
        Some(&opened)
    )
    .is_err());
    let opened = projects::Snapshot {
        claude_local: false,
        ..opened
    };
    projects::save(
        &env.dir,
        "p",
        input(&["beta"], false),
        vec![],
        &overrides,
        exe,
        Some(&opened),
    )
    .unwrap();
    assert_eq!(read(&env.dir).unwrap().projects["p"].entry_ids, ["beta"]);
}

#[test]
fn viewing_creates_nothing_and_refuses_an_unfinished_write() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    // 还没有托管状态：读出空的，不建目录、不建锁文件
    assert!(read_only(&dir).unwrap().entries.is_empty());
    assert!(!dir.exists());
    save(&dir, hub("alpha"), vec![], Path::new("/app"), None).unwrap();
    assert_eq!(read_only(&dir).unwrap().entries.len(), 1);
    // 写到一半退出留下的记录：不替它恢复，也不读半截的状态
    let journal = dir.join("management-transaction.json");
    std::fs::write(
        &journal,
        r#"{"changes":[],"catalog":{"path":"x","before":null,"after":null}}"#,
    )
    .unwrap();
    assert_eq!(read_only(&dir).err().unwrap().to_string(), PENDING);
    assert!(journal.exists());
}

#[cfg(unix)]
#[test]
fn viewing_works_where_the_studio_directory_is_read_only() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("studio");
    save(&dir, hub("alpha"), vec![], Path::new("/app"), None).unwrap();
    std::fs::remove_file(dir.join("management.lock")).unwrap();
    let mode = |path: &Path, bits| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits)).unwrap()
    };
    mode(&dir, 0o500);
    let result = read_only(&dir);
    mode(&dir, 0o700);
    assert_eq!(result.unwrap().entries.len(), 1);
    assert!(!dir.join("management.lock").exists());
}

#[test]
fn waiting_for_another_process_gives_up_at_the_limit() {
    let env = Env::new();
    let holder = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(env.dir.join("management.lock"))
        .unwrap();
    fs2::FileExt::lock_exclusive(&holder).unwrap();
    set_lock_wait(Duration::from_millis(200));
    let started = std::time::Instant::now();
    let write = groups::save_group(&env.dir, group("a", &["alpha"]), vec![], None);
    let view = read_only(&env.dir);
    set_lock_wait(Duration::from_secs(30));
    assert_eq!(write.unwrap_err().to_string(), CONFIG_BUSY);
    assert_eq!(view.err().unwrap().to_string(), CONFIG_BUSY);
    assert!(started.elapsed() < Duration::from_secs(5));
    fs2::FileExt::unlock(&holder).unwrap();
    groups::save_group(&env.dir, group("a", &["alpha"]), vec![], None).unwrap();
}

#[test]
fn gateway_connections_need_the_desktop_app() {
    let env = Env::new();
    let mut gateway = hub("remote");
    gateway.mode = "gateway".into();
    gateway.definition = json!({"type":"http","url":"https://example.com/mcp"});
    // 只放进 Hub 不用写接入，找不到桌面应用也可以
    save(&env.dir, gateway.clone(), vec![], Path::new(""), None).unwrap();
    let target = Target {
        agent: "codex".into(),
        path: env.config("codex"),
        project: None,
        key: "remote".into(),
        expected: None,
    };
    let catalog = read(&env.dir).unwrap();
    let stored = catalog.entries.iter().find(|e| e.id == "remote").cloned();
    let error = save(
        &env.dir,
        gateway,
        vec![target],
        Path::new(""),
        stored.as_ref(),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, GATEWAY_NEEDS_APP);
    groups::save_group(&env.dir, group("g", &["remote"]), vec![], None).unwrap();
    let path = env.config("codex");
    let error = groups::set_active(&env.dir, "codex", "g", true, || Ok(path), Path::new(""))
        .unwrap_err()
        .to_string();
    assert_eq!(error, GATEWAY_NEEDS_APP);
    assert!(env.active("codex").is_empty());
    // 直连的照常写
    groups::save_group(&env.dir, group("d", &["alpha"]), vec![], None).unwrap();
    let path = env.config("codex");
    groups::set_active(&env.dir, "codex", "d", true, || Ok(path), Path::new("")).unwrap();
    assert!(env.deployed("codex", "alpha"));
}
