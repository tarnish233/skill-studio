//! 桌面应用与命令行等多个进程同时读写 `~/.skill-studio/config.json`。
//!
//! 每个 `AppState` 代表一个进程：桌面应用启动时把配置读进内存，命令行每次调用各自读一次。
//! 跨进程的文件锁按「打开的文件」计，同一进程里两次打开同样互斥，所以这里不必真的起进程；
//! 真正跨进程的用例见 remote-helper 的协议测试。

use serial_test::serial;
use skill_studio_core::fs::paths;
use skill_studio_core::services::store::Store;
use skill_studio_service::{groups, state::AppState};
use std::path::{Path, PathBuf};
use std::time::Duration;

struct Env {
    home: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, home.path());
        for var in [
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "XDG_CONFIG_HOME",
            "OPENCODE_CONFIG_DIR",
            "PI_CODING_AGENT_DIR",
            "GROK_HOME",
        ] {
            std::env::remove_var(var);
        }
        Self { home }
    }

    fn dir(&self) -> PathBuf {
        self.home.path().join(".skill-studio")
    }

    /// 起一个「进程」：和真实启动一样，从磁盘读配置
    fn process(&self) -> AppState {
        AppState::bootstrap(Store::new(self.dir())).unwrap()
    }

    fn lock_path(&self) -> PathBuf {
        self.dir().join("config.json.lock")
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        std::env::remove_var(paths::TEST_HOME_ENV);
    }
}

fn names(state: &AppState) -> Vec<String> {
    let mut names: Vec<_> = groups::list_groups(state)
        .unwrap()
        .into_iter()
        .map(|g| g.name)
        .collect();
    names.sort();
    names
}

fn on_disk(dir: &Path) -> Vec<String> {
    let config = Store::new(dir.to_path_buf()).load().unwrap();
    let mut names: Vec<_> = config.groups.into_iter().map(|g| g.name).collect();
    names.sort();
    names
}

/// 另一个进程正拿着配置锁：像它正在写入一样
fn hold_lock(path: &Path) -> std::fs::File {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    fs2::FileExt::lock_exclusive(&file).unwrap();
    file
}

#[test]
#[serial]
fn a_second_process_keeps_the_group_the_first_one_saved() {
    let env = Env::new();
    let desktop = env.process();
    let cli = env.process();
    groups::save_group(&cli, None, "命令行建的".into(), vec![], None).unwrap();
    groups::save_group(&desktop, None, "桌面建的".into(), vec![], None).unwrap();
    assert_eq!(on_disk(&env.dir()), ["命令行建的", "桌面建的"]);
}

#[test]
#[serial]
fn reads_show_what_another_process_just_saved() {
    let env = Env::new();
    let desktop = env.process();
    let cli = env.process();
    groups::save_group(&cli, None, "命令行建的".into(), vec![], None).unwrap();
    assert_eq!(names(&desktop), ["命令行建的"]);
    assert_eq!(desktop.config().groups.len(), 1);
}

#[test]
#[serial]
fn a_write_waits_until_the_other_process_has_finished() {
    let env = Env::new();
    let desktop = env.process();
    let held = hold_lock(&env.lock_path());
    let (tx, rx) = std::sync::mpsc::channel();
    let writer = desktop.clone();
    let thread = std::thread::spawn(move || {
        let result = groups::save_group(&writer, None, "桌面建的".into(), vec![], None);
        tx.send(()).unwrap();
        result
    });
    assert!(
        rx.recv_timeout(Duration::from_millis(400)).is_err(),
        "另一个进程还没写完，这次写入就落盘了"
    );
    drop(held);
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    thread.join().unwrap().unwrap();
    assert_eq!(on_disk(&env.dir()), ["桌面建的"]);
}

#[test]
#[serial]
fn a_reader_never_falls_back_to_an_empty_config_while_another_process_is_writing() {
    let env = Env::new();
    let desktop = env.process();
    groups::save_group(&desktop, None, "已有分组".into(), vec![], None).unwrap();
    let config = env.dir().join("config.json");
    let aside = env.dir().join("config.json.moving");
    // 另一个进程的事务：拿着锁，先把 config.json 挪开，稍后写回
    let held = hold_lock(&env.lock_path());
    std::fs::rename(&config, &aside).unwrap();
    let reader = desktop.clone();
    let thread = std::thread::spawn(move || names(&reader));
    std::thread::sleep(Duration::from_millis(300));
    std::fs::rename(&aside, &config).unwrap();
    drop(held);
    assert_eq!(thread.join().unwrap(), ["已有分组"]);
}

#[test]
#[serial]
fn a_write_gives_up_with_a_clear_message_when_the_other_process_does_not_finish() {
    let env = Env::new();
    let desktop = env.process().with_lock_wait(Duration::from_millis(200));
    let _held = hold_lock(&env.lock_path());
    let error = groups::save_group(&desktop, None, "桌面建的".into(), vec![], None).unwrap_err();
    assert!(error.contains("正在修改配置"), "{error}");
    assert!(on_disk(&env.dir()).is_empty());
}

#[test]
#[serial]
fn reads_do_not_hang_when_the_other_process_holds_the_lock_too_long() {
    let env = Env::new();
    let desktop = env.process().with_lock_wait(Duration::from_millis(200));
    groups::save_group(&desktop, None, "已有分组".into(), vec![], None).unwrap();
    let _held = hold_lock(&env.lock_path());
    let started = std::time::Instant::now();
    assert_eq!(names(&desktop), ["已有分组"]);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
#[serial]
fn the_desktop_is_told_once_about_changes_made_by_another_process() {
    let env = Env::new();
    let desktop = env.process();
    groups::save_group(&desktop, None, "自己建的".into(), vec![], None).unwrap();
    assert!(!desktop.take_external_change(), "自己的写入不算外部修改");
    let cli = env.process();
    groups::save_group(&cli, None, "命令行建的".into(), vec![], None).unwrap();
    assert!(desktop.take_external_change());
    assert!(!desktop.take_external_change(), "同一次修改只报一次");
    assert_eq!(names(&desktop), ["命令行建的", "自己建的"]);
}

// ───────────── 按最新状态启停：不撤掉别处刚做的启用 ─────────────

use serde_json::json;
use skill_studio_service::{dispatch, projects, skills};
use std::collections::HashMap;

fn skill(env: &Env, name: &str) {
    let dir = env.dir().join("skills").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: 测试用 {name}\n---\n\n正文\n"),
    )
    .unwrap();
}

fn ids(state: &AppState) -> HashMap<String, String> {
    skills::scan_skills(state)
        .unwrap()
        .into_iter()
        .map(|v| (v.skill.name, v.skill.id))
        .collect()
}

fn active(env: &Env, agent: &str) -> Vec<String> {
    let config = Store::new(env.dir()).load().unwrap();
    config
        .active_groups_of(agent)
        .iter()
        .map(|g| g.group_id.clone())
        .collect()
}

fn deployed(env: &Env, name: &str) -> bool {
    env.home
        .path()
        .join(".codex/skills")
        .join(name)
        .join("SKILL.md")
        .is_file()
}

/// 两个分组「甲」(one)、「乙」(two)
fn two_groups(env: &Env) -> (AppState, String, String, HashMap<String, String>) {
    skill(env, "one");
    skill(env, "two");
    let desktop = env.process();
    let ids = ids(&desktop);
    let a = groups::save_group(&desktop, None, "甲".into(), vec![ids["one"].clone()], None)
        .unwrap()
        .id;
    let b = groups::save_group(&desktop, None, "乙".into(), vec![ids["two"].clone()], None)
        .unwrap()
        .id;
    (desktop, a, b, ids)
}

#[test]
#[serial]
fn enabling_a_group_keeps_the_one_another_process_just_enabled() {
    let env = Env::new();
    let (desktop, a, b, _) = two_groups(&env);
    let cli = env.process();
    groups::set_agent_group(&cli, "codex".into(), b.clone(), true).unwrap();
    // 桌面这边还没刷新，只知道自己要启用「甲」
    groups::set_agent_group(&desktop, "codex".into(), a.clone(), true).unwrap();
    assert_eq!(active(&env, "codex"), [b.clone(), a.clone()]);
    assert!(deployed(&env, "one") && deployed(&env, "two"));

    groups::set_agent_group(&cli, "codex".into(), a.clone(), false).unwrap();
    assert_eq!(active(&env, "codex"), [b]);
    assert!(!deployed(&env, "one") && deployed(&env, "two"));
}

#[test]
#[serial]
fn enabling_an_enabled_group_changes_nothing() {
    let env = Env::new();
    let (desktop, a, _, _) = two_groups(&env);
    groups::set_agent_group(&desktop, "codex".into(), a.clone(), true).unwrap();
    let before = std::fs::read(env.dir().join("config.json")).unwrap();
    assert!(groups::set_agent_group(&desktop, "codex".into(), a, true)
        .unwrap()
        .is_empty());
    assert_eq!(
        std::fs::read(env.dir().join("config.json")).unwrap(),
        before
    );
    assert!(groups::set_agent_group(&desktop, "claude-code".into(), "乙".into(), false).is_ok());
}

#[test]
#[serial]
fn reapplying_uses_the_groups_enabled_at_that_moment() {
    let env = Env::new();
    let (desktop, a, b, _) = two_groups(&env);
    groups::set_agent_group(&desktop, "codex".into(), a.clone(), true).unwrap();
    let cli = env.process();
    groups::set_agent_group(&cli, "codex".into(), b.clone(), true).unwrap();
    groups::reapply_agent_groups(&desktop, "codex".into()).unwrap();
    assert_eq!(active(&env, "codex"), [a, b]);
    assert!(deployed(&env, "one") && deployed(&env, "two"));
}

#[test]
#[serial]
fn saving_a_group_that_changed_elsewhere_is_refused() {
    let env = Env::new();
    let (desktop, a, _, ids) = two_groups(&env);
    let opened = groups::GroupSnapshot {
        name: "甲".into(),
        skill_ids: vec![ids["one"].clone()],
    };
    let cli = env.process();
    groups::save_group(
        &cli,
        Some(a.clone()),
        "甲".into(),
        vec![ids["one"].clone(), ids["two"].clone()],
        Some(opened.clone()),
    )
    .unwrap();
    let error = groups::save_group(
        &desktop,
        Some(a.clone()),
        "甲（改）".into(),
        vec![ids["one"].clone()],
        Some(opened),
    )
    .unwrap_err();
    assert!(error.contains("已被别处修改"), "{error}");
    let config = Store::new(env.dir()).load().unwrap();
    let group = config.group(&a).unwrap();
    assert_eq!((group.name.as_str(), group.skill_ids.len()), ("甲", 2));

    // 名称前后的空格和成员顺序不算修改
    let latest = groups::GroupSnapshot {
        name: " 甲 ".into(),
        skill_ids: vec![ids["two"].clone(), ids["one"].clone()],
    };
    groups::save_group(
        &desktop,
        Some(a.clone()),
        "甲".into(),
        vec![ids["two"].clone()],
        Some(latest),
    )
    .unwrap();

    groups::delete_group(&cli, a.clone()).unwrap();
    let gone = groups::GroupSnapshot {
        name: "甲".into(),
        skill_ids: vec![ids["two"].clone()],
    };
    let error = groups::save_group(&desktop, Some(a), "甲".into(), vec![], Some(gone)).unwrap_err();
    assert!(error.contains("已被别处删除"), "{error}");
    assert_eq!(on_disk(&env.dir()), ["乙"]);
}

fn members(skill_ids: &[&str]) -> skill_studio_service::state::ProjectMembers {
    serde_json::from_value(json!({"skillIds": skill_ids, "groupIds": [], "linkMode": "copy"}))
        .unwrap()
}

/// 项目在各 Agent 上的部署：Agent → 写入的 skill
fn deployments(state: &AppState, project: &str) -> HashMap<String, usize> {
    let list = dispatch(state, "list_projects", json!({})).unwrap();
    let project = list
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == project)
        .unwrap();
    project["deployments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["agentId"].as_str().unwrap().to_string(),
                d["skillIds"].as_array().map(Vec::len).unwrap_or(0),
            )
        })
        .collect()
}

#[test]
#[serial]
fn project_switches_and_saves_follow_what_other_processes_did() {
    let env = Env::new();
    let (desktop, _, _, ids) = two_groups(&env);
    let root = env.home.path().join("work/app");
    std::fs::create_dir_all(&root).unwrap();
    let project = projects::create_project(&desktop, "app".into(), root)
        .unwrap()
        .id;
    let (one, two) = (ids["one"].as_str(), ids["two"].as_str());
    projects::save_project(
        &desktop,
        project.clone(),
        members(&[one]),
        Some(members(&[])),
    )
    .unwrap();
    assert!(
        deployments(&desktop, &project).is_empty(),
        "还没有启用 Agent 时只保存"
    );
    projects::set_project_agent(&desktop, project.clone(), "claude-code".into(), true).unwrap();

    // 命令行加了成员、又为 Codex 启用
    let cli = env.process();
    projects::save_project(
        &cli,
        project.clone(),
        members(&[one, two]),
        Some(members(&[one])),
    )
    .unwrap();
    projects::set_project_agent(&cli, project.clone(), "codex".into(), true).unwrap();
    let both: HashMap<String, usize> = [("claude-code".into(), 2), ("codex".into(), 2)].into();
    assert_eq!(deployments(&cli, &project), both);

    // 桌面拿着打开时的成员保存：别处改过，不覆盖
    let error = projects::save_project(
        &desktop,
        project.clone(),
        members(&[one]),
        Some(members(&[one])),
    )
    .unwrap_err();
    assert!(error.contains("已被别处修改"), "{error}");
    assert_eq!(deployments(&desktop, &project), both);

    // 按最新成员保存：写入两边已启用的 Agent，不撤掉命令行启用的 Codex
    projects::save_project(
        &desktop,
        project.clone(),
        members(&[two]),
        Some(members(&[two, one])),
    )
    .unwrap();
    let after: HashMap<String, usize> = [("claude-code".into(), 1), ("codex".into(), 1)].into();
    assert_eq!(deployments(&desktop, &project), after);

    // 停用一个 Agent 不影响另一个；重复停用什么也不做
    projects::set_project_agent(&desktop, project.clone(), "claude-code".into(), false).unwrap();
    projects::set_project_agent(&cli, project.clone(), "claude-code".into(), false).unwrap();
    let codex: HashMap<String, usize> = [("codex".into(), 1)].into();
    assert_eq!(deployments(&cli, &project), codex);
    projects::reapply_project(&desktop, project.clone()).unwrap();
    assert_eq!(deployments(&desktop, &project), codex);
}

// ───────────── 只读：不恢复、不校正、不写任何文件 ─────────────

#[cfg(unix)]
fn tree(root: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let meta = path.symlink_metadata().unwrap();
            if meta.is_dir() {
                stack.push(path.clone());
                out.push((path, None));
            } else {
                out.push((path.clone(), std::fs::read(&path).ok()));
            }
        }
    }
    out.sort();
    out
}

#[cfg(unix)]
fn set_writable(root: &Path, writable: bool) {
    use std::os::unix::fs::PermissionsExt;
    let mut stack = vec![root.to_path_buf()];
    let mut dirs = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let meta = path.symlink_metadata().unwrap();
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(path.clone());
                dirs.push(path);
            } else {
                let mode = if writable { 0o644 } else { 0o444 };
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            }
        }
    }
    dirs.push(root.to_path_buf());
    for dir in dirs {
        let mode = if writable { 0o755 } else { 0o555 };
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
    }
}

#[test]
#[serial]
#[cfg(unix)]
fn a_read_only_state_reads_everything_without_writing_a_byte() {
    let env = Env::new();
    let (desktop, a, _, ids) = two_groups(&env);
    groups::set_agent_group(&desktop, "codex".into(), a, true).unwrap();
    let root = env.home.path().join("work/app");
    std::fs::create_dir_all(&root).unwrap();
    let project = projects::create_project(&desktop, "app".into(), root)
        .unwrap()
        .id;
    projects::save_project(&desktop, project.clone(), members(&[&ids["two"]]), None).unwrap();
    projects::set_project_agent(&desktop, project.clone(), "claude-code".into(), true).unwrap();
    drop(desktop);

    let before = tree(env.home.path());
    set_writable(env.home.path(), false);
    let viewer = AppState::read_only(Store::new(env.dir()));
    let outcome = viewer.map(|viewer| {
        let reads = [
            ("get_config", json!({})),
            ("get_settings", json!({})),
            ("list_agents", json!({"skipCliProbe": true})),
            ("scan_skills", json!({})),
            ("list_groups", json!({})),
            ("list_projects", json!({})),
            ("list_project_skills", json!({"projectId": project})),
            ("list_skill_backups", json!({})),
            ("list_hub_backups", json!({})),
            ("list_backups", json!({})),
        ];
        let failures: Vec<String> = reads
            .iter()
            .filter_map(|(method, params)| {
                dispatch(&viewer, method, params.clone())
                    .err()
                    .map(|e| format!("{method}: {e}"))
            })
            .collect();
        let refused = dispatch(
            &viewer,
            "save_group",
            json!({"groupId": null, "name": "新分组", "skillIds": []}),
        )
        .unwrap_err();
        (failures, refused)
    });
    set_writable(env.home.path(), true);
    let (failures, refused) = outcome.unwrap();
    assert!(failures.is_empty(), "只读时读取失败：{failures:?}");
    assert!(refused.contains("只读"), "{refused}");
    assert_eq!(tree(env.home.path()), before, "只读时写了文件");
}

#[test]
#[serial]
fn an_agent_chosen_long_ago_but_never_written_is_not_written_along() {
    let env = Env::new();
    let (desktop, _, _, ids) = two_groups(&env);
    let root = env.home.path().join("work/note");
    std::fs::create_dir_all(&root).unwrap();
    let project = projects::create_project(&desktop, "note".into(), root)
        .unwrap()
        .id;
    // 旧版本留下的数据：给 Codex 选了成员，但没有写入
    projects::update_project(
        &desktop,
        project.clone(),
        None,
        Some(vec!["codex".into()]),
        Some(vec![ids["one"].clone()]),
        None,
        None,
    )
    .unwrap();
    projects::set_project_agent(&desktop, project.clone(), "claude-code".into(), true).unwrap();
    let claude: HashMap<String, usize> = [("claude-code".into(), 1)].into();
    assert_eq!(deployments(&desktop, &project), claude);
}

#[test]
#[serial]
fn a_reader_that_cannot_wait_any_longer_keeps_the_last_good_config() {
    let env = Env::new();
    let desktop = env.process().with_lock_wait(Duration::from_millis(100));
    groups::save_group(&desktop, None, "已有分组".into(), vec![], None).unwrap();
    let config = env.dir().join("config.json");
    let aside = env.dir().join("config.json.moving");
    let held = hold_lock(&env.lock_path());
    std::fs::rename(&config, &aside).unwrap();
    // 等锁超时、文件又不在：用内存里上一份完整的，不能当成首次启动的空配置
    assert_eq!(names(&desktop), ["已有分组"]);
    std::fs::rename(&aside, &config).unwrap();
    drop(held);
    assert_eq!(names(&desktop), ["已有分组"]);
}

#[test]
#[serial]
fn a_read_guard_holds_no_lock_and_sees_the_next_write() {
    let env = Env::new();
    let desktop = env.process();
    let cli = env.process();
    // 桌面正拿着配置做判断（比如列出全部项目）：命令行照常能写，不必等它
    let reading = desktop.config();
    assert!(names(&desktop).is_empty());
    groups::save_group(&cli, None, "命令行建的".into(), vec![], None).unwrap();
    drop(reading);
    assert_eq!(names(&desktop), ["命令行建的"]);
}

#[test]
#[serial]
fn reading_does_not_wait_for_another_process_that_is_writing() {
    let env = Env::new();
    let desktop = env.process();
    groups::save_group(&desktop, None, "已有分组".into(), vec![], None).unwrap();
    let _held = hold_lock(&env.lock_path());
    // 别的进程正写着：读取立刻返回上一次读到的完整配置，不跟着等
    let started = std::time::Instant::now();
    assert_eq!(names(&desktop), ["已有分组"]);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "读取等了 {:?}",
        started.elapsed()
    );
}

#[test]
#[serial]
fn a_long_read_does_not_block_another_process_writing() {
    let env = Env::new();
    let desktop = env.process();
    let cli = env.process();
    // 桌面正拿着配置做判断（比如列出全部项目）：命令行照常能写
    let reading = desktop.config();
    let started = std::time::Instant::now();
    groups::save_group(&cli, None, "命令行建的".into(), vec![], None).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "写入等了 {:?}",
        started.elapsed()
    );
    drop(reading);
}

#[test]
#[serial]
fn reads_keep_working_while_this_process_waits_for_another_writer() {
    let env = Env::new();
    let desktop = env.process();
    groups::save_group(&desktop, None, "已有分组".into(), vec![], None).unwrap();
    let held = hold_lock(&env.lock_path());
    let writer = desktop.clone();
    let thread = std::thread::spawn(move || {
        groups::save_group(&writer, None, "桌面建的".into(), vec![], None)
    });
    std::thread::sleep(Duration::from_millis(300));
    let started = std::time::Instant::now();
    assert_eq!(names(&desktop), ["已有分组"]);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "写入排队时读取被挡住了 {:?}",
        started.elapsed()
    );
    drop(held);
    thread.join().unwrap().unwrap();
    assert_eq!(names(&desktop), ["已有分组", "桌面建的"]);
}

#[test]
#[serial]
fn a_scan_while_another_process_holds_the_lock_falls_back_to_reading() {
    let env = Env::new();
    skill(&env, "one");
    let desktop = env.process().with_lock_wait(Duration::from_millis(200));
    let _held = hold_lock(&env.lock_path());
    // 界面刷新触发的那次扫描不该冒出错误
    let views = skills::scan_skills(&desktop).unwrap();
    assert!(views.iter().any(|v| v.skill.name == "one"), "{views:?}");
}
