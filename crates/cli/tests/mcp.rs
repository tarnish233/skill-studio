//! 命令行的 MCP 命令：在独立的临时主目录里跑，绝不碰真实的 ~/.skill-studio 与各 Agent 配置。

use std::path::PathBuf;

use serde_json::Value;
use serial_test::serial;
use skill_studio_core::fs::paths;

const ENV: &[&str] = &[
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "XDG_CONFIG_HOME",
    "OPENCODE_CONFIG_DIR",
    "OPENCODE_CONFIG",
    "PI_CODING_AGENT_DIR",
    "GROK_HOME",
    "SKILL_STUDIO_LOCK_WAIT_MS",
    "SKILL_STUDIO_APP_EXECUTABLE",
];

struct Home {
    dir: tempfile::TempDir,
}

struct Output {
    code: i32,
    out: String,
    err: String,
}

impl Output {
    fn json(&self) -> Value {
        let text = if self.code == 0 { &self.out } else { &self.err };
        serde_json::from_str(text.trim()).unwrap_or_else(|e| panic!("不是 JSON（{e}）：{text}"))
    }
}

impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, dir.path());
        for name in ENV {
            std::env::remove_var(name);
        }
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        Self { dir }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.path(relative)).unwrap_or_default()
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut out = Vec::new();
        let mut err = Vec::new();
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

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert_eq!(
            output.code, 0,
            "{args:?} 失败：{}{}",
            output.out, output.err
        );
        output.out
    }

    fn json(&self, args: &[&str]) -> Output {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        self.run(&all)
    }

    /// Codex 全局配置里有没有这个 MCP
    fn in_codex(&self, key: &str) -> bool {
        skill_studio_mcp::native::entry(&self.read(".codex/config.toml"), "codex", None, key)
            .unwrap()
            .is_some()
    }

    fn in_claude(&self, key: &str) -> bool {
        skill_studio_mcp::native::entry(&self.read(".claude.json"), "claude", None, key)
            .unwrap()
            .is_some()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        std::env::remove_var(paths::TEST_HOME_ENV);
        for name in ENV {
            std::env::remove_var(name);
        }
    }
}

#[test]
#[serial]
fn adding_mcp_three_ways_keeps_every_secret_out_of_the_output() {
    let home = Home::new();
    home.ok(&[
        "mcp",
        "add",
        "github",
        "--env",
        "GITHUB_TOKEN=ghp_secret",
        "--",
        "npx",
        "-y",
        "@modelcontextprotocol/server-github",
    ]);
    home.ok(&[
        "mcp",
        "add",
        "context7",
        "--url",
        "https://mcp.context7.com/mcp?api_key=sk-query",
        "-H",
        "Authorization: Bearer header-secret",
    ]);
    let added = home.ok(&[
        "mcp",
        "add",
        "--config",
        r#"{"mcpServers":{"fetch":{"command":"uvx","args":["mcp-server-fetch","--api-key","sk-arg"]},"time":{"command":"uvx","args":["mcp-server-time"]}}}"#,
    ]);
    assert!(added.contains("fetch、time"), "{added}");
    // 只放进 Hub，不写进任何 Agent
    assert!(!home.path(".claude.json").exists());
    assert!(!home.in_codex("github"));

    let list = home.ok(&["mcp", "list"]);
    let list_json = home.json(&["mcp", "list"]).out;
    let show = home.json(&["mcp", "show", "github"]).out;
    for output in [&list, &list_json, &show] {
        for secret in ["ghp_secret", "sk-query", "header-secret", "sk-arg"] {
            assert!(!output.contains(secret), "{secret} 出现在输出里：{output}");
        }
    }
    assert!(list.contains("api_key=<已隐藏>"), "{list}");
    assert!(list.contains("--api-key <已隐藏>"), "{list}");
    let shown: Value = serde_json::from_str(show.trim()).unwrap();
    assert_eq!(shown["definition"]["env"]["GITHUB_TOKEN"], "<已隐藏>");
    assert_eq!(shown["hub"], true);

    // 同样的再加一次：没有变化；同名不同配置：拒绝
    let again = home.json(&[
        "mcp",
        "add",
        "github",
        "--env",
        "GITHUB_TOKEN=ghp_secret",
        "--",
        "npx",
        "-y",
        "@modelcontextprotocol/server-github",
    ]);
    assert_eq!(again.code, 0);
    assert_eq!(again.json()["changed"], false);
    let clash = home.json(&["mcp", "add", "GitHub", "--url", "https://example.com/mcp"]);
    assert_eq!(clash.code, 1);
    assert!(clash.json()["error"]["message"]
        .as_str()
        .unwrap()
        .contains("已有同名"));
    // 用法错误：什么都没写；多个配置共用一个名称
    assert_eq!(home.run(&["mcp", "add", "x"]).code, 2);
    let shared = home.run(&[
        "mcp",
        "add",
        "both",
        "--config",
        r#"{"mcpServers":{"a":{"command":"a"},"b":{"command":"b"}}}"#,
    ]);
    assert_eq!(shared.code, 2, "{}", shared.err);
}

#[test]
#[serial]
fn group_switches_stack_per_agent_and_never_drop_each_other() {
    let home = Home::new();
    home.ok(&["mcp", "add", "alpha", "--", "alpha-server"]);
    home.ok(&["mcp", "add", "beta", "--", "beta-server"]);
    home.ok(&["mcp", "groups", "create", "甲", "--mcp", "alpha"]);
    home.ok(&["mcp", "groups", "create", "乙", "--mcp", "beta"]);

    let enabled = home.ok(&["mcp", "groups", "enable", "甲", "-a", "codex,claude"]);
    assert!(enabled.contains("请在 Agent 中重新加载 MCP"), "{enabled}");
    home.ok(&["mcp", "groups", "enable", "乙", "-a", "codex"]);
    assert!(home.in_codex("alpha") && home.in_codex("beta"));
    assert!(home.in_claude("alpha") && !home.in_claude("beta"));

    let again = home.json(&["mcp", "groups", "enable", "甲", "-a", "codex"]);
    assert_eq!(again.json()["changed"], false);

    home.ok(&["mcp", "groups", "disable", "甲", "-a", "codex"]);
    assert!(!home.in_codex("alpha"));
    assert!(home.in_codex("beta"), "停用甲不能把乙一起撤掉");
    let groups = home.json(&["mcp", "groups", "list"]).json();
    assert_eq!(groups[0]["enabled"], serde_json::json!(["claude-code"]));
    assert_eq!(groups[1]["enabled"], serde_json::json!(["codex"]));

    // 正在用的分组不能删
    let refused = home.run(&["mcp", "groups", "delete", "乙", "--yes"]);
    assert_eq!(refused.code, 1);
    assert!(
        refused.err.contains("mcp groups disable 乙 --agent codex"),
        "{}",
        refused.err
    );
}

#[test]
#[serial]
fn editing_a_group_in_use_writes_through_and_emptying_it_needs_stopping_first() {
    let home = Home::new();
    home.ok(&["mcp", "add", "alpha", "--", "alpha-server"]);
    home.ok(&["mcp", "add", "beta", "--", "beta-server"]);
    home.ok(&["mcp", "groups", "create", "甲", "--mcp", "alpha"]);
    home.ok(&["mcp", "groups", "enable", "甲", "-a", "codex"]);

    let added = home.ok(&["mcp", "groups", "add", "甲", "beta"]);
    assert!(added.contains("并写入 Codex"), "{added}");
    assert!(home.in_codex("beta"));
    home.ok(&["mcp", "groups", "remove", "甲", "alpha"]);
    assert!(!home.in_codex("alpha"));

    let refused = home.json(&["mcp", "groups", "remove", "甲", "beta"]);
    assert_eq!(refused.code, 1);
    let error = &refused.json()["error"];
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("清空成员前请先为这些 Agent 停用"),
        "{error}"
    );
    assert!(error["hint"]
        .as_str()
        .unwrap()
        .contains("mcp groups disable 甲"));
    assert!(home.in_codex("beta"), "拒绝时什么都不改");
}

#[test]
#[serial]
fn an_original_config_can_join_a_group_without_being_taken_over() {
    let home = Home::new();
    let original = "model = \"keep\"\n[mcp_servers.manual]\ncommand = \"manual-server\"\n";
    home.write(".codex/config.toml", original);
    let list = home.json(&["mcp", "list"]).json();
    assert_eq!(list[0]["name"], "manual");
    assert_eq!(list[0]["hub"], false);

    home.ok(&["mcp", "groups", "create", "常用", "--mcp", "manual"]);
    home.ok(&["mcp", "groups", "enable", "常用", "-a", "claude"]);
    assert!(home.in_claude("manual"));
    // Codex 里的原件原样留着，也没有放进 Hub
    assert_eq!(home.read(".codex/config.toml"), original);
    let list = home.json(&["mcp", "list"]).json();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["hub"], false);
}

#[test]
#[serial]
fn project_mcp_is_written_per_agent_and_the_project_cannot_be_deleted_while_in_use() {
    let home = Home::new();
    std::fs::create_dir_all(home.path("work")).unwrap();
    home.ok(&["mcp", "add", "alpha", "--", "alpha-server"]);
    home.ok(&["projects", "create", home.path("work").to_str().unwrap()]);
    home.ok(&[
        "mcp", "projects", "add", "work", "--mcp", "alpha", "--claude", "local",
    ]);
    home.ok(&["mcp", "projects", "enable", "work", "-a", "claude,codex"]);

    let root = dunce::canonicalize(home.path("work")).unwrap();
    let claude: Value = serde_json::from_str(&home.read(".claude.json")).unwrap();
    assert!(
        claude["projects"][root.to_str().unwrap()]["mcpServers"]["alpha"].is_object(),
        "{claude}"
    );
    assert!(!home.path("work/.mcp.json").exists(), "选了只给自己用");
    assert!(home.read("work/.codex/config.toml").contains("alpha"));

    let shown = home.json(&["mcp", "projects", "show", "work"]).json();
    assert_eq!(shown["claudeLocal"], true);
    assert_eq!(
        shown["enabled"],
        serde_json::json!(["claude-code", "codex"])
    );

    let refused = home.run(&["projects", "delete", "work", "--yes"]);
    assert_eq!(refused.code, 1);
    assert!(
        refused
            .err
            .contains("mcp projects disable work --agent claude-code,codex"),
        "{}",
        refused.err
    );
    home.ok(&["mcp", "projects", "disable", "work", "-a", "claude,codex"]);
    assert!(!home.read("work/.codex/config.toml").contains("alpha"));
    home.ok(&["projects", "delete", "work", "--yes"]);
    let catalog: Value =
        serde_json::from_str(&home.read(".skill-studio/mcp/catalog.json")).unwrap();
    assert_eq!(catalog["projects"], serde_json::json!({}));
}

#[test]
#[serial]
fn removing_needs_yes_and_puts_back_what_was_there() {
    let home = Home::new();
    home.write(
        ".codex/config.toml",
        "[mcp_servers.manual]\ncommand = \"manual-server\"\n[mcp_servers.other]\ncommand = \"keep\"\n",
    );
    home.ok(&["mcp", "add", "alpha", "--", "alpha-server"]);
    let confirm = home.run(&["mcp", "remove", "alpha"]);
    assert_eq!(confirm.code, 5, "{}", confirm.err);
    home.ok(&["mcp", "remove", "alpha", "--yes"]);
    assert!(home.run(&["mcp", "show", "alpha"]).code == 3);

    // 原有配置：从 Agent 配置里删掉这一项，其他的不动
    assert_eq!(home.run(&["mcp", "remove", "manual"]).code, 5);
    home.ok(&["mcp", "remove", "manual", "--yes"]);
    assert!(!home.in_codex("manual"));
    assert!(home.in_codex("other"));
}

#[test]
#[serial]
fn gateway_mcps_are_written_only_where_the_desktop_app_is() {
    let home = Home::new();
    home.ok(&[
        "mcp",
        "add",
        "remote",
        "--url",
        "https://example.com/mcp",
        "--gateway",
    ]);
    home.ok(&["mcp", "groups", "create", "网关", "--mcp", "remote"]);
    let refused = home.run(&["mcp", "groups", "enable", "网关", "-a", "codex"]);
    assert_eq!(refused.code, 1);
    assert!(
        refused.err.contains("由 Skill Studio 桌面应用转发"),
        "{}",
        refused.err
    );
    assert!(!home.in_codex("remote"));

    std::env::set_var(
        "SKILL_STUDIO_APP_EXECUTABLE",
        "/Applications/Skill Studio.app/Contents/MacOS/skill-studio",
    );
    home.ok(&["mcp", "groups", "enable", "网关", "-a", "codex"]);
    let adapter =
        skill_studio_mcp::native::entry(&home.read(".codex/config.toml"), "codex", None, "remote")
            .unwrap()
            .unwrap();
    assert_eq!(
        adapter["command"],
        "/Applications/Skill Studio.app/Contents/MacOS/skill-studio"
    );
    assert_eq!(adapter["args"][0], "--mcp-client");
}

#[test]
#[serial]
fn waiting_for_another_writer_of_mcp_gives_up_with_exit_4() {
    let home = Home::new();
    home.ok(&["mcp", "add", "alpha", "--", "alpha-server"]);
    let holder = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(home.path(".skill-studio/mcp/management.lock"))
        .unwrap();
    fs2::FileExt::lock_exclusive(&holder).unwrap();
    std::env::set_var("SKILL_STUDIO_LOCK_WAIT_MS", "200");
    let started = std::time::Instant::now();
    let busy = home.json(&["mcp", "groups", "create", "甲", "--mcp", "alpha"]);
    // 按 SKILL_STUDIO_LOCK_WAIT_MS 的上限放弃，而不是干等默认的 30 秒
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert_eq!(busy.code, 4, "{}", busy.err);
    assert_eq!(busy.json()["error"]["code"], "busy");
    fs2::FileExt::unlock(&holder).unwrap();
    home.ok(&["mcp", "groups", "create", "甲", "--mcp", "alpha"]);
}

#[test]
#[serial]
fn mcp_commands_stop_while_mcp_management_is_off_and_writes_are_logged() {
    let home = Home::new();
    home.ok(&["mcp", "add", "alpha", "--", "alpha-server"]);
    home.ok(&[
        "mcp",
        "add",
        "beta",
        "-e",
        "TOKEN=sk-log",
        "--",
        "beta-server",
    ]);
    let log = home.read(".skill-studio/logs/cli.log");
    assert!(
        log.contains("skill-studio mcp add alpha -- alpha-server → 已把 alpha 添加到 MCP Hub"),
        "{log}"
    );
    // 日志里不留密钥
    assert!(!log.contains("sk-log"), "{log}");
    assert!(log.contains("-e TOKEN=<已隐藏> -- beta-server"), "{log}");

    let state = skill_studio_service::state::AppState::bootstrap(
        skill_studio_core::services::store::Store::new(home.path(".skill-studio")),
    )
    .unwrap();
    skill_studio_service::settings::update_settings(
        &state,
        skill_studio_service::settings::SettingsPatch {
            manage_mcp: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    let off = home.json(&["mcp", "list"]);
    assert_eq!(off.code, 1);
    assert!(off.json()["error"]["hint"]
        .as_str()
        .unwrap()
        .contains("启用 MCP 管理"));
    let status = home.ok(&["status"]);
    assert!(status.contains("MCP：管理已关闭"), "{status}");
}

#[cfg(unix)]
#[test]
#[serial]
fn viewing_mcp_in_a_read_only_home_writes_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let home = Home::new();
    std::fs::create_dir_all(home.path("work")).unwrap();
    home.ok(&["mcp", "add", "alpha", "--", "alpha-server"]);
    home.ok(&["mcp", "groups", "create", "甲", "--mcp", "alpha"]);
    home.ok(&["mcp", "groups", "enable", "甲", "-a", "codex"]);
    home.ok(&["projects", "create", home.path("work").to_str().unwrap()]);
    home.ok(&["mcp", "projects", "add", "work", "--mcp", "alpha"]);
    std::fs::remove_file(home.path(".skill-studio/mcp/management.lock")).unwrap();
    std::fs::remove_file(home.path(".skill-studio/config.json.lock")).ok();

    fn tree(root: &std::path::Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                tree(&path, out);
            } else {
                out.push((path.clone(), std::fs::read(&path).unwrap()));
            }
        }
    }
    fn writable(root: &std::path::Path, on: bool) {
        for entry in walk(root) {
            let mode = if entry.is_dir() {
                if on {
                    0o755
                } else {
                    0o555
                }
            } else if on {
                0o644
            } else {
                0o444
            };
            std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(mode)).unwrap();
        }
    }
    fn walk(root: &std::path::Path) -> Vec<PathBuf> {
        let mut all = vec![root.to_path_buf()];
        if root.is_dir() {
            for entry in std::fs::read_dir(root).unwrap() {
                all.extend(walk(&entry.unwrap().path()));
            }
        }
        all
    }
    let mut before = Vec::new();
    tree(home.dir.path(), &mut before);
    writable(home.dir.path(), false);
    let commands: &[&[&str]] = &[
        &["mcp", "list"],
        &["mcp", "show", "alpha"],
        &["mcp", "groups", "list"],
        &["mcp", "groups", "show", "甲"],
        &["mcp", "projects", "list"],
        &["mcp", "projects", "show", "work"],
        &["status"],
        &["doctor"],
    ];
    let results: Vec<(String, Output)> = commands
        .iter()
        .map(|args| (args.join(" "), home.run(args)))
        .collect();
    writable(home.dir.path(), true);
    for (command, output) in &results {
        assert_eq!(output.code, 0, "{command}：{}{}", output.out, output.err);
    }
    let mut after = Vec::new();
    tree(home.dir.path(), &mut after);
    assert_eq!(before.len(), after.len(), "查看命令新建了文件");
    assert!(before == after, "查看命令改了文件");
}

#[test]
#[serial]
fn changing_a_hub_mcp_marks_its_groups_pending_until_applied() {
    let home = Home::new();
    home.write(
        ".codex/config.toml",
        "[mcp_servers.manual]\ncommand = \"manual-server\"\n",
    );
    home.ok(&[
        "mcp",
        "add",
        "github",
        "-e",
        "GITHUB_TOKEN=v1-secret",
        "--",
        "gh-server",
    ]);
    home.ok(&["mcp", "groups", "create", "常用", "--mcp", "github"]);
    home.ok(&["mcp", "groups", "enable", "常用", "-a", "codex"]);
    assert!(home.read(".codex/config.toml").contains("v1-secret"));

    let changed = home.json(&["mcp", "set", "github", "-e", "GITHUB_TOKEN=v2-secret"]);
    assert_eq!(changed.code, 0, "{}", changed.err);
    assert!(!changed.out.contains("v2-secret"), "{}", changed.out);
    let json = changed.json();
    assert_eq!(json["changed"], true);
    assert_eq!(
        json["apply"],
        serde_json::json!(["skill-studio mcp groups apply 常用"])
    );
    // 部署里还是旧的：有待应用修改
    assert!(home.read(".codex/config.toml").contains("v1-secret"));
    let shown = home.json(&["mcp", "groups", "show", "常用"]).json();
    assert_eq!(
        shown["states"][0]["pending"],
        serde_json::json!(["github 的配置已修改"])
    );
    home.ok(&["mcp", "groups", "apply", "常用"]);
    assert!(home.read(".codex/config.toml").contains("v2-secret"));

    let same = home.json(&["mcp", "set", "github", "-e", "GITHUB_TOKEN=v2-secret"]);
    assert_eq!(same.json()["changed"], false);
    assert_eq!(
        home.run(&["mcp", "set", "github", "--url", "https://x.dev"])
            .code,
        2
    );
    assert_eq!(
        home.run(&["mcp", "set", "github", "--unset-env", "NOPE"])
            .code,
        3
    );
    // 原有配置不在 Hub 里，不能这样改
    assert_eq!(home.run(&["mcp", "set", "manual", "-e", "A=1"]).code, 1);
    // 换启动命令、去掉环境变量
    home.ok(&[
        "mcp",
        "set",
        "github",
        "--unset-env",
        "GITHUB_TOKEN",
        "--",
        "gh",
        "--stdio",
    ]);
    let definition = &home.json(&["mcp", "show", "github"]).json()["definition"];
    assert_eq!(definition["command"], "gh");
    assert_eq!(definition["args"], serde_json::json!(["--stdio"]));
    assert!(definition.get("env").is_none(), "{definition}");
}
