use super::*;
use serde_json::json;

fn hub(dir: &Path, id: &str, command: &str) {
    crate::management::save(
        dir,
        Entry {
            id: id.into(),
            name: id.into(),
            mode: "direct".into(),
            definition: json!({"type":"stdio","command":command}),
            oauth: false,
            client_id: None,
            scopes: vec![],
            bindings: vec![],
        },
        vec![],
        Path::new("/app"),
        None,
    )
    .unwrap();
}
fn input(root: &Path, ids: &[&str], groups: &[&str]) -> Input {
    Input {
        root: root.into(),
        entry_ids: ids.iter().map(|s| s.to_string()).collect(),
        group_ids: groups.iter().map(|s| s.to_string()).collect(),
        claude_local: false,
    }
}
/// 各 Agent 的全局配置都指到临时目录，测试不碰真实的 ~/.codex 等
fn overrides(tmp: &Path) -> HashMap<String, PathBuf> {
    ["codex", "pi", "grok", "opencode"]
        .iter()
        .map(|a| (a.to_string(), tmp.join("home").join(a)))
        .collect()
}
fn native_entry(path: &Path, agent: &str, key: &str) -> Option<Value> {
    native::entry(
        &std::fs::read_to_string(path).unwrap_or_default(),
        agent,
        None,
        key,
    )
    .unwrap()
}
struct Env {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    root: PathBuf,
    overrides: HashMap<String, PathBuf>,
}
impl Env {
    fn new(ids: &[&str]) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("studio");
        let root = tmp.path().join("work/webapp");
        std::fs::create_dir_all(&root).unwrap();
        for id in ids {
            hub(&dir, id, id);
        }
        let overrides = overrides(tmp.path());
        Self {
            _tmp: tmp,
            dir,
            root,
            overrides,
        }
    }
    fn save(&self, ids: &[&str], groups: &[&str]) -> Result<Vec<Untouched>> {
        save(
            &self.dir,
            "p1",
            input(&self.root, ids, groups),
            vec![],
            &self.overrides,
            Path::new("/app"),
            None,
        )
    }
    fn activate(&self, agent: &str, enabled: bool) -> Result<Vec<Untouched>> {
        activate(
            &self.dir,
            "p1",
            agent,
            enabled,
            &self.overrides,
            Path::new("/app"),
        )
    }
    fn codex(&self) -> PathBuf {
        self.root.join(".codex/config.toml")
    }
    fn claude(&self) -> PathBuf {
        self.root.join(".mcp.json")
    }
}

#[test]
fn members_are_written_to_each_agent_separately() {
    let env = Env::new(&["alpha", "beta"]);
    // 只保存成员：还没有 Agent 启用，一个文件都不写
    assert!(env.save(&["alpha", "beta"], &[]).unwrap().is_empty());
    assert!(!env.codex().exists() && !env.claude().exists());
    env.activate("codex", true).unwrap();
    assert!(native_entry(&env.codex(), "codex", "alpha").is_some());
    assert!(native_entry(&env.codex(), "codex", "beta").is_some());
    assert!(!env.claude().exists());
    env.activate("claude", true).unwrap();
    assert!(native_entry(&env.claude(), "claude", "alpha").is_some());
    // 停用 Codex 只撤回 Codex 的，Claude 的不受影响
    env.activate("codex", false).unwrap();
    assert_eq!(native_entry(&env.codex(), "codex", "alpha"), None);
    assert!(native_entry(&env.claude(), "claude", "beta").is_some());
    let catalog = read(&env.dir).unwrap();
    assert_eq!(
        catalog.projects["p1"].active.keys().collect::<Vec<_>>(),
        ["claude"]
    );
}

#[test]
fn saving_rewrites_enabled_agents_and_withdraws_them_when_emptied() {
    let env = Env::new(&["alpha", "beta"]);
    env.save(&["alpha", "beta"], &[]).unwrap();
    env.activate("codex", true).unwrap();
    // 保存即写入已启用的 Agent
    env.save(&["alpha"], &[]).unwrap();
    assert!(native_entry(&env.codex(), "codex", "alpha").is_some());
    assert_eq!(native_entry(&env.codex(), "codex", "beta"), None);
    env.save(&[], &[]).unwrap();
    assert_eq!(native_entry(&env.codex(), "codex", "alpha"), None);
    assert!(read(&env.dir).unwrap().projects["p1"].active.is_empty());
}

#[test]
fn bound_groups_expand_into_members() {
    let env = Env::new(&["alpha", "beta", "gamma"]);
    groups::save_group(
        &env.dir,
        groups::Group {
            id: "g".into(),
            name: "g".into(),
            entry_ids: vec!["alpha".into(), "beta".into()],
            references: vec![],
            sort_order: 0,
        },
        vec![],
        None,
    )
    .unwrap();
    env.save(&["gamma", "alpha"], &["g"]).unwrap();
    env.activate("codex", true).unwrap();
    for key in ["alpha", "beta", "gamma"] {
        assert!(native_entry(&env.codex(), "codex", key).is_some(), "{key}");
    }
    // 只写进项目，不碰 Codex 的全局配置
    assert!(!env.overrides["codex"].join("config.toml").exists());
}

#[test]
fn a_rewritten_connection_is_left_alone_when_stopping() {
    let env = Env::new(&["alpha", "beta"]);
    env.save(&["alpha", "beta"], &[]).unwrap();
    env.activate("codex", true).unwrap();
    let mut doc: toml_edit::DocumentMut = std::fs::read_to_string(env.codex())
        .unwrap()
        .parse()
        .unwrap();
    doc["mcp_servers"]["alpha"]["command"] = toml_edit::value("rewritten");
    std::fs::write(env.codex(), doc.to_string()).unwrap();
    let untouched = env.activate("codex", false).unwrap();
    assert_eq!(
        untouched
            .iter()
            .map(|u| (u.key.as_str(), u.tracked))
            .collect::<Vec<_>>(),
        [("alpha", false)]
    );
    assert_eq!(
        native_entry(&env.codex(), "codex", "alpha").unwrap()["command"],
        "rewritten"
    );
    assert_eq!(native_entry(&env.codex(), "codex", "beta"), None);
}

#[test]
fn issues_name_the_project_and_agent_whose_connection_changed() {
    let env = Env::new(&["alpha"]);
    env.save(&["alpha"], &[]).unwrap();
    env.activate("codex", true).unwrap();
    env.activate("claude", true).unwrap();
    assert!(issues(&read(&env.dir).unwrap()).is_empty());
    std::fs::write(env.codex(), "[mcp_servers.alpha]\ncommand='rewritten'\n").unwrap();
    let issues = issues(&read(&env.dir).unwrap());
    assert_eq!(issues["p1"].keys().collect::<Vec<_>>(), ["codex"]);
    assert!(issues["p1"]["codex"].contains("alpha"));
}

#[test]
fn removing_a_project_needs_every_agent_stopped() {
    let env = Env::new(&["alpha"]);
    env.save(&["alpha"], &[]).unwrap();
    env.activate("codex", true).unwrap();
    assert!(remove(&env.dir, "p1").is_err());
    env.activate("codex", false).unwrap();
    remove(&env.dir, "p1").unwrap();
    assert!(read(&env.dir).unwrap().projects.is_empty());
}

#[test]
fn a_hub_entry_an_active_project_uses_cannot_be_deleted() {
    let env = Env::new(&["alpha"]);
    env.save(&["alpha"], &[]).unwrap();
    env.activate("codex", true).unwrap();
    assert!(crate::management::remove(&env.dir, "alpha", false).is_err());
    env.activate("codex", false).unwrap();
    crate::management::remove(&env.dir, "alpha", false).unwrap();
    // 成员已缺失：不能再启用
    let error = env.activate("codex", true).unwrap_err().to_string();
    assert!(error.contains("已缺失"), "{error}");
}

#[test]
fn turning_management_off_and_on_restores_project_connections() {
    let env = Env::new(&["alpha"]);
    env.save(&["alpha"], &[]).unwrap();
    env.activate("codex", true).unwrap();
    crate::management::set_enabled(&env.dir, false).unwrap();
    assert_eq!(native_entry(&env.codex(), "codex", "alpha"), None);
    crate::management::set_enabled(&env.dir, true).unwrap();
    assert!(native_entry(&env.codex(), "codex", "alpha").is_some());
}

#[test]
fn a_project_at_the_agents_global_config_is_rejected() {
    let mut env = Env::new(&["alpha"]);
    // Codex 的全局配置目录正好是这个项目里的 .codex：写进项目就等于写进全局
    env.overrides
        .insert("codex".into(), env.root.join(".codex"));
    env.save(&["alpha"], &[]).unwrap();
    let error = env.activate("codex", true).unwrap_err().to_string();
    assert!(error.contains("重叠"), "{error}");
    assert!(!env.codex().exists());
}

#[test]
fn enabling_needs_members() {
    let env = Env::new(&["alpha"]);
    assert!(env.activate("codex", true).is_err());
    env.save(&[], &[]).unwrap();
    let error = env.activate("codex", true).unwrap_err().to_string();
    assert!(error.contains("还没有选择 MCP"), "{error}");
}
