//! 新增管理命令的回归：所有文件与 Agent 配置都在临时主目录里。
use std::path::PathBuf;

use serde_json::{json, Value};
use serial_test::serial;
use skill_studio_core::{fs::paths, services::store::Store};

struct Home(tempfile::TempDir);
struct Output {
    code: i32,
    out: String,
    err: String,
}
impl Output {
    fn json(&self) -> Value {
        serde_json::from_str(if self.code == 0 { &self.out } else { &self.err })
            .unwrap_or_else(|e| panic!("{e}: {}{}", self.out, self.err))
    }
}
impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, dir.path());
        for name in [
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "XDG_CONFIG_HOME",
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_CONFIG",
            "PI_CODING_AGENT_DIR",
            "GROK_HOME",
            "DSH_HOME",
            "DSH_AGENTS_HOME",
            "SKILL_STUDIO_LOCK_WAIT_MS",
            "SKILL_STUDIO_APP_EXECUTABLE",
        ] {
            std::env::remove_var(name);
        }
        for dir_name in [".claude", ".codex", ".pi/agent"] {
            std::fs::create_dir_all(dir.path().join(dir_name)).unwrap();
        }
        Self(dir)
    }
    fn path(&self, path: &str) -> PathBuf {
        self.0.path().join(path)
    }
    fn write(&self, path: &str, content: &str) {
        let path = self.path(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    fn skill(&self, path: &str, name: &str) -> PathBuf {
        self.write(
            &format!("{path}/SKILL.md"),
            &format!("---\nname: {name}\ndescription: test\n---\nbody\n"),
        );
        self.path(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        let mut out = vec![];
        let mut err = vec![];
        let code = skill_studio_cli::run(
            std::iter::once("skill-studio").chain(args.iter().copied()),
            &mut out,
            &mut err,
        );
        Output {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }
    fn json(&self, args: &[&str]) -> Value {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let result = self.run(&all);
        assert_eq!(result.code, 0, "{args:?}: {}{}", result.out, result.err);
        result.json()
    }
    fn config(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.path(".skill-studio/config.json")).unwrap())
            .unwrap()
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        std::env::remove_var(paths::TEST_HOME_ENV);
        std::env::remove_var("SKILL_STUDIO_APP_EXECUTABLE");
        std::env::remove_var("SKILL_STUDIO_LOCK_WAIT_MS");
    }
}

#[test]
#[serial]
fn skill_lifecycle_selects_one_source_and_requires_confirmation() {
    let home = Home::new();
    let claude = home.skill(".claude/skills/rex", "rex");
    let shared = home.skill(".agents/skills/rex", "rex");
    let ambiguous = home.run(&["--json", "skills", "adopt", "rex", "--yes"]);
    assert_eq!(ambiguous.code, 3, "{}", ambiguous.err);
    assert_eq!(
        ambiguous.json()["error"]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        home.run(&["skills", "adopt", claude.to_str().unwrap()])
            .code,
        5
    );
    assert!(claude.join("SKILL.md").is_file() && shared.join("SKILL.md").is_file());
    let adopted = home.json(&["skills", "adopt", claude.to_str().unwrap(), "--yes"]);
    assert_eq!(adopted["changed"], true);
    let hub = home.path(".skill-studio/skills/rex");
    assert!(hub.join("SKILL.md").is_file());
    assert!(shared.join("SKILL.md").is_file());
    #[cfg(unix)]
    assert!(claude.symlink_metadata().unwrap().file_type().is_symlink());
    // Even when one same-named source is in Hub, destructive operations must not guess.
    assert_eq!(home.run(&["skills", "delete", "rex", "--yes"]).code, 3);
    let backups = home.json(&["backups", "list", "hub"]);
    let id = backups["backups"][0]["id"].as_str().unwrap();
    assert_eq!(backups["backups"][0]["inUse"], true);
    assert_eq!(home.run(&["backups", "delete", "hub", id, "--yes"]).code, 1);
    assert_eq!(
        home.run(&["skills", "release", hub.to_str().unwrap()]).code,
        5
    );
    home.json(&["skills", "release", hub.to_str().unwrap(), "--yes"]);
    assert!(claude.join("SKILL.md").is_file());
    assert!(shared.join("SKILL.md").is_file());
    assert!(!hub.exists());
    let unused = home.json(&["backups", "delete", "hub", "--unused", "--yes"]);
    assert!(unused["removed"].as_u64().unwrap() > 0);
}

#[test]
#[serial]
fn skill_deletion_and_restore_never_overwrite_an_existing_directory() {
    let home = Home::new();
    let original = home.skill(".skill-studio/skills/alpha", "alpha");
    assert_eq!(home.run(&["skills", "delete", "alpha"]).code, 5);
    home.json(&["skills", "delete", "alpha", "--yes"]);
    assert!(!original.exists());
    let backups = home.json(&["backups", "list", "skill"]);
    let id = backups["backups"][0]["id"].as_str().unwrap();
    assert_eq!(home.run(&["backups", "restore", "skill", id]).code, 5);
    home.skill(".skill-studio/skills/alpha", "replacement");
    let before = std::fs::read(original.join("SKILL.md")).unwrap();
    assert_eq!(
        home.run(&["backups", "restore", "skill", id, "--yes"]).code,
        1
    );
    assert_eq!(std::fs::read(original.join("SKILL.md")).unwrap(), before);
    std::fs::remove_dir_all(&original).unwrap();
    home.json(&["backups", "restore", "skill", id, "--yes"]);
    home.json(&["skills", "delete", "alpha", "--yes"]);
    let backups = home.json(&["backups", "list", "skill"]);
    let id = backups["backups"][0]["id"].as_str().unwrap();
    assert_eq!(home.run(&["backups", "delete", "skill", id]).code, 5);
    home.json(&["backups", "delete", "skill", id, "--yes"]);
    assert!(home.json(&["backups", "list", "skill"])["backups"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
#[serial]
fn deleting_an_active_or_builtin_skill_is_rejected() {
    let home = Home::new();
    home.skill(".skill-studio/skills/alpha", "alpha");
    home.json(&["groups", "create", "work", "--skills", "alpha"]);
    home.json(&["groups", "enable", "work", "--agent", "claude"]);
    assert_eq!(home.run(&["skills", "delete", "alpha", "--yes"]).code, 1);
    assert!(home.path(".skill-studio/skills/alpha/SKILL.md").is_file());
    let state =
        skill_studio_service::state::AppState::bootstrap(Store::new(home.path(".skill-studio")))
            .unwrap();
    let resources =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources/builtin-skills");
    skill_studio_service::skills::sync_builtin_skills(&state, &resources, "test").unwrap();
    assert_eq!(
        home.run(&["skills", "delete", "skill-studio", "--yes"])
            .code,
        1
    );
    assert!(home
        .path(".skill-studio/skills/skill-studio/SKILL.md")
        .is_file());
}

#[test]
#[serial]
fn viewing_settings_and_backups_is_read_only() {
    let home = Home::new();
    assert_eq!(
        home.json(&["settings", "show"])["preserveManualSkills"],
        true
    );
    for kind in ["config", "skill", "hub", "mcp"] {
        assert!(home.json(&["backups", "list", kind])["backups"]
            .as_array()
            .unwrap()
            .is_empty());
    }
    assert!(!home.path(".skill-studio").exists());
}

#[test]
#[serial]
fn settings_reuse_policy_directory_and_mcp_management_checks() {
    let home = Home::new();
    assert_eq!(
        home.json(&["settings", "set", "--default-link-mode", "copy"])["settings"]
            ["defaultLinkMode"],
        "copy"
    );
    assert_eq!(
        home.json(&["settings", "set", "--default-link-mode", "copy"])["changed"],
        false
    );
    assert_eq!(
        home.run(&["settings", "set", "--preserve-manual-skills", "false"])
            .code,
        5
    );
    home.json(&[
        "settings",
        "set",
        "--preserve-manual-skills",
        "false",
        "--yes",
    ]);
    home.json(&[
        "settings",
        "set",
        "--preserve-manual-skills",
        "true",
        "--yes",
    ]);
    home.json(&["settings", "set", "--unmanage-agent", "pi", "--yes"]);
    assert!(home.json(&["settings", "show"])["disabledAgents"]
        .as_array()
        .unwrap()
        .contains(&json!("pi")));
    home.json(&["settings", "set", "--manage-agent", "pi", "--yes"]);
    let override_dir = home.path("custom-claude");
    home.json(&[
        "settings",
        "set",
        "--agent-dir",
        &format!("claude={}", override_dir.display()),
    ]);
    assert_eq!(
        home.json(&["settings", "show"])["agentDirOverrides"]["claude-code"],
        override_dir.to_str().unwrap()
    );
    home.json(&["settings", "set", "--clear-agent-dir", "claude"]);
    let new_hub = home.path("empty-hub");
    home.json(&["settings", "set", "--hub-dir", new_hub.to_str().unwrap()]);
    home.json(&["settings", "set", "--clear-hub-dir"]);
    home.json(&["settings", "set", "--manage-mcp", "false", "--yes"]);
    assert_eq!(home.run(&["mcp", "list"]).code, 1);
    home.json(&["settings", "set", "--manage-mcp", "true", "--yes"]);
    assert_eq!(
        home.run(&["settings", "set", "--backup-keep", "101"]).code,
        2
    );
    // Stateful management toggles cannot be mixed into a partially-applied patch.
    let before = home.config();
    assert_ne!(
        home.run(&[
            "settings",
            "set",
            "--manage-mcp",
            "false",
            "--theme",
            "dark",
            "--yes"
        ])
        .code,
        0
    );
    assert_eq!(home.config(), before);
}

#[test]
#[serial]
fn settings_cannot_bypass_the_cli_off_switch() {
    let home = Home::new();
    home.json(&["settings", "set", "--cli-enabled", "false", "--yes"]);
    let log = std::fs::read(home.path(".skill-studio/logs/cli.log")).unwrap();
    assert_eq!(
        home.run(&["settings", "set", "--cli-enabled", "true", "--yes"])
            .code,
        1
    );
    assert_eq!(
        std::fs::read(home.path(".skill-studio/logs/cli.log")).unwrap(),
        log
    );
}

#[test]
#[serial]
fn config_backup_restore_validates_ids_and_requires_confirmation() {
    let home = Home::new();
    home.json(&["groups", "create", "before"]);
    home.json(&["groups", "create", "after"]);
    let backups = home.json(&["backups", "list", "config"]);
    let id = backups["backups"][0]["id"].as_str().unwrap();
    assert_eq!(home.run(&["backups", "restore", "config", id]).code, 5);
    assert_eq!(
        home.run(&["backups", "restore", "config", "../config.json", "--yes"])
            .code,
        3
    );
    home.json(&["backups", "restore", "config", id, "--yes"]);
    assert_eq!(home.config()["groups"].as_array().unwrap().len(), 1);
    assert_eq!(home.run(&["backups", "delete", "config", id]).code, 5);
    home.json(&["backups", "delete", "config", id, "--yes"]);
}

#[test]
#[serial]
fn config_backups_remain_accessible_when_config_is_corrupt() {
    let home = Home::new();
    home.json(&["groups", "create", "one"]);
    home.json(&["groups", "create", "two"]);
    let backups = home.json(&["backups", "list", "config"]);
    let id = backups["backups"][0]["id"].as_str().unwrap();
    home.write(".skill-studio/config.json", "not json");
    assert_eq!(home.run(&["status"]).code, 1);
    assert!(!home.json(&["backups", "list", "config"])["backups"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_ne!(
        home.run(&["backups", "delete", "config", id, "--yes"]).code,
        0
    );
    home.json(&["backups", "restore", "config", id, "--yes"]);
    assert_eq!(home.run(&["status"]).code, 0);
}

#[test]
#[serial]
fn project_originals_have_separate_list_suspend_collect_and_delete_commands() {
    let home = Home::new();
    home.write("work/.keep", "");
    let root = home.path("work");
    home.json(&[
        "projects",
        "create",
        root.to_str().unwrap(),
        "--name",
        "app",
    ]);
    let original = home.skill("work/.claude/skills/manual", "manual");
    let listed = home.json(&["projects", "skills", "list", "app"]);
    assert_eq!(listed[0]["name"], "manual");
    home.json(&["projects", "skills", "disable", "app", "manual"]);
    assert!(!original.exists());
    assert_eq!(
        home.json(&["projects", "skills", "disable", "app", "manual"])["changed"],
        false
    );
    assert_eq!(
        home.json(&["projects", "skills", "list", "app"])[0]["disabled"],
        true
    );
    home.json(&["projects", "skills", "adopt", "app", "manual"]);
    assert!(home.path(".skill-studio/skills/manual/SKILL.md").is_file());
    home.json(&["projects", "skills", "enable", "app", "manual"]);
    assert!(original.join("SKILL.md").is_file());
    assert_eq!(
        home.run(&["projects", "skills", "delete", "app", "manual"])
            .code,
        5
    );
    home.json(&["projects", "skills", "delete", "app", "manual", "--yes"]);
    assert!(!original.exists());
    let before = home.config();
    let wrong = home.skill("elsewhere/.claude/skills/unrelated", "unrelated");
    assert_eq!(
        home.run(&[
            "projects",
            "skills",
            "delete",
            "app",
            wrong.to_str().unwrap(),
            "--yes"
        ])
        .code,
        3
    );
    assert!(wrong.join("SKILL.md").exists());
    assert_eq!(home.config(), before);
}

#[test]
#[serial]
fn project_settings_apply_link_modes_and_gitignore_without_duplicate_lines() {
    let home = Home::new();
    home.write("work/.gitignore", "node_modules/\n");
    let root = home.path("work");
    home.json(&[
        "projects",
        "create",
        root.to_str().unwrap(),
        "--name",
        "app",
    ]);
    home.json(&["projects", "set", "app", "--link-mode", "symlink"]);
    assert_eq!(
        home.json(&["projects", "show", "app"])["linkMode"],
        "symlink"
    );
    assert_eq!(
        home.json(&["projects", "set", "app", "--link-mode", "symlink"])["changed"],
        false
    );
    assert_eq!(
        home.json(&["projects", "gitignore", "app"])["changed"],
        true
    );
    assert_eq!(
        home.json(&["projects", "gitignore", "app"])["changed"],
        false
    );
    assert!(std::fs::read_to_string(root.join(".gitignore"))
        .unwrap()
        .starts_with("node_modules/"));
}

#[test]
#[serial]
fn mcp_full_edit_preserves_secrets_and_supports_modes_transport_and_metadata() {
    let home = Home::new();
    home.json(&["mcp", "add", "demo", "--", "tool"]);
    let raw = r#"{"type":"stdio","command":"tool","env":{"TOKEN":"sk-full-config"},"timeout":7,"tools":["one"]}"#;
    let cwd = home.path("working-directory");
    home.json(&[
        "mcp",
        "set",
        "demo",
        "--config",
        raw,
        "--cwd",
        cwd.to_str().unwrap(),
    ]);
    let shown = home.json(&["mcp", "show", "demo"]);
    assert_eq!(shown["definition"]["cwd"], cwd.to_str().unwrap());
    assert_eq!(shown["definition"]["env"]["TOKEN"], "<已隐藏>");
    assert_eq!(shown["definition"]["timeout"], 7);
    home.json(&["mcp", "set", "demo", "--clear-cwd"]);
    home.json(&[
        "mcp",
        "set",
        "demo",
        "--transport",
        "http",
        "--url",
        "https://example.com/mcp?token=sk-query-secret",
    ]);
    std::env::set_var(
        "SKILL_STUDIO_APP_EXECUTABLE",
        std::env::current_exe().unwrap(),
    );
    assert_eq!(
        home.run(&["mcp", "set", "demo", "--mode", "gateway"]).code,
        1,
        "网关不应静默丢弃不支持的扩展字段"
    );
    home.json(&[
        "mcp",
        "set",
        "demo",
        "--config",
        r#"{"type":"http","url":"https://example.com/mcp?token=sk-query-secret"}"#,
        "--mode",
        "gateway",
        "--client-id",
        "client-one",
        "--scopes",
        "read,write",
    ]);
    let entry = home.json(&["mcp", "show", "demo"]);
    assert_eq!(entry["mode"], "gateway");
    assert_eq!(entry["clientId"], "client-one");
    assert_eq!(entry["scopes"], json!(["read", "write"]));
    assert_eq!(
        home.json(&["mcp", "set", "demo", "--scopes", "read,write"])["changed"],
        false
    );
    let log = std::fs::read_to_string(home.path(".skill-studio/logs/cli.log")).unwrap();
    assert!(
        !log.contains("sk-full-config") && !log.contains("sk-query-secret"),
        "{log}"
    );
    let before = home.json(&["mcp", "show", "demo"]);
    let failed = home.run(&[
        "mcp",
        "set",
        "demo",
        "--config",
        "not-json sk-malformed-secret",
    ]);
    assert_eq!(failed.code, 2);
    assert!(!failed.err.contains("sk-malformed-secret"));
    assert_eq!(home.json(&["mcp", "show", "demo"]), before);
}

#[test]
#[serial]
fn mcp_test_is_bounded_read_only_and_does_not_leak_credentials_on_failure() {
    let home = Home::new();
    home.json(&[
        "mcp",
        "add",
        "bad",
        "--env",
        "TOKEN=sk-test-secret",
        "--",
        "nonexistent-cli-mcp-test-command",
    ]);
    let config = std::fs::read(home.path(".skill-studio/config.json")).unwrap_or_default();
    let catalog = std::fs::read(home.path(".skill-studio/mcp/catalog.json")).unwrap();
    let failed = home.run(&["--json", "mcp", "test", "bad", "--timeout", "1"]);
    assert_eq!(failed.code, 1, "{}", failed.err);
    assert!(!failed.err.contains("sk-test-secret"));
    assert_eq!(
        std::fs::read(home.path(".skill-studio/config.json")).unwrap_or_default(),
        config
    );
    assert_eq!(
        std::fs::read(home.path(".skill-studio/mcp/catalog.json")).unwrap(),
        catalog
    );
    assert_eq!(home.run(&["mcp", "test", "bad", "--timeout", "0"]).code, 2);
}

#[test]
#[serial]
fn mcp_search_rejects_bad_queries_without_writes_or_network() {
    let home = Home::new();
    assert_eq!(home.run(&["mcp", "search", "a"]).code, 2);
    assert_eq!(home.run(&["mcp", "search", &"x".repeat(101)]).code, 2);
    assert!(!home.path(".skill-studio").exists());
}

#[test]
#[serial]
fn mcp_backups_restore_native_files_and_refuse_foreign_changes() {
    let home = Home::new();
    home.json(&["mcp", "add", "demo", "--", "tool"]);
    home.json(&["mcp", "groups", "create", "work", "--mcp", "demo"]);
    home.json(&["mcp", "groups", "enable", "work", "--agent", "claude"]);
    home.json(&["mcp", "groups", "disable", "work", "--agent", "claude"]);
    let backups = home.json(&["backups", "list", "mcp"]);
    assert!(!backups["backups"].as_array().unwrap().is_empty());
    let id = backups["backups"][0]["id"].as_str().unwrap();
    assert_eq!(home.run(&["backups", "restore", "mcp", id]).code, 5);
    let before = std::fs::read(home.path(".claude.json")).unwrap();
    home.write(
        ".claude.json",
        "{\"mcpServers\":{\"foreign\":{\"command\":\"mine\"}}}",
    );
    assert_eq!(
        home.run(&["backups", "restore", "mcp", id, "--yes"]).code,
        1
    );
    std::fs::write(home.path(".claude.json"), before).unwrap();
    home.json(&["backups", "restore", "mcp", id, "--yes"]);
    assert_eq!(home.run(&["backups", "delete", "mcp", "--all"]).code, 5);
    home.json(&["backups", "delete", "mcp", "--all", "--yes"]);
}
