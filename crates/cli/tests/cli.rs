//! 命令行的行为：在独立的临时主目录里跑，绝不碰真实的 ~/.skill-studio 与各 Agent 目录。

#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

use serde_json::Value;
use serial_test::serial;
use skill_studio_core::fs::paths;

const AGENT_ENV: &[&str] = &[
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "XDG_CONFIG_HOME",
    "OPENCODE_CONFIG_DIR",
    "OPENCODE_CONFIG",
    "PI_CODING_AGENT_DIR",
    "GROK_HOME",
    "SKILL_STUDIO_LOCK_WAIT_MS",
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
        for name in AGENT_ENV {
            std::env::remove_var(name);
        }
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        Self { dir }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    /// 在 `root`（相对主目录）下造一个 skill
    fn skill(&self, root: &str, name: &str) -> PathBuf {
        let dir = self.path(root).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: 测试用 {name}\n---\n\n正文\n"),
        )
        .unwrap();
        dir
    }

    fn hub(&self, name: &str) -> PathBuf {
        self.skill(".skill-studio/skills", name)
    }

    fn config(&self) -> Value {
        let text = std::fs::read_to_string(self.path(".skill-studio/config.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    /// 直接改配置，模拟另一个窗口（命令行本身只通过命令修改）
    fn save_config(&self, config: &Value) {
        std::fs::write(
            self.path(".skill-studio/config.json"),
            serde_json::to_string_pretty(config).unwrap(),
        )
        .unwrap();
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
}

impl Drop for Home {
    fn drop(&mut self) {
        std::env::remove_var(paths::TEST_HOME_ENV);
    }
}

fn deployed(home: &Home, root: &str, name: &str) -> bool {
    home.path(root).join(name).join("SKILL.md").is_file()
}

#[test]
#[serial]
fn help_is_chinese_and_exit_codes_follow_the_contract() {
    let home = Home::new();
    let help = home.run(&["--help"]);
    assert_eq!(help.code, 0);
    assert!(
        help.out.contains("用法：skill-studio [选项] <命令>"),
        "{}",
        help.out
    );
    assert!(help.out.contains("命令：") && help.out.contains("groups"));
    assert!(help.out.contains("mcp"), "{}", help.out);
    let mcp = home.ok(&["mcp", "add", "--help"]);
    assert!(mcp.contains("例子："), "{mcp}");
    let sub = home.ok(&["groups", "enable", "--help"]);
    assert!(
        sub.contains("用法：skill-studio groups enable [选项] --agent <Agent> <分组>"),
        "{sub}"
    );

    let bare = home.run(&[]);
    assert_eq!(bare.code, 2, "不带命令时显示帮助并按用法错误退出");
    assert!(bare.out.contains("命令："));
    assert!(home.ok(&["--version"]).starts_with("skill-studio "));

    let unknown = home.run(&["nosuch"]);
    assert_eq!(unknown.code, 2);
    assert!(
        unknown.err.contains("没有这个命令：nosuch"),
        "{}",
        unknown.err
    );
    // 服务层的错误前缀（「无效输入: 」）不带到命令行里
    home.ok(&["groups", "create", "笔记"]);
    let conflict = home.run(&["groups", "create", "笔记"]);
    assert_eq!(conflict.code, 1);
    assert_eq!(
        conflict.err.trim(),
        "错误：已存在同名分组",
        "{}",
        conflict.err
    );

    let missing = home.json(&["groups", "enable", "笔记"]);
    assert_eq!(missing.code, 2);
    let error = &missing.json()["error"];
    assert_eq!(error["code"], "usage");
    assert!(error["message"].as_str().unwrap().contains("缺少参数"));
}

#[test]
#[serial]
fn groups_are_created_enabled_per_agent_and_kept_in_sync() {
    let home = Home::new();
    for name in ["alpha", "beta", "gamma"] {
        home.hub(name);
    }
    let created = home.json(&["groups", "create", "笔记", "--skills", "alpha,beta"]);
    assert_eq!(created.json()["group"]["name"], "笔记");
    assert!(created.json()["next"]
        .as_str()
        .unwrap()
        .contains("groups enable 笔记"));

    let enabled = home.json(&[
        "groups", "enable", "笔记", "--agent", "codex", "-a", "claude",
    ]);
    let json = enabled.json();
    assert_eq!(json["changed"], true);
    assert_eq!(json["agents"], serde_json::json!(["codex", "claude-code"]));
    assert!(json["reload"].as_str().unwrap().contains("重新加载"));
    for root in [".codex/skills", ".claude/skills"] {
        assert!(deployed(&home, root, "alpha") && deployed(&home, root, "beta"));
    }

    // 加入成员即写入正在使用这个分组的 Agent，与界面一致
    let added = home.json(&["groups", "add", "笔记", "gamma"]).json();
    assert_eq!(
        added["written"],
        serde_json::json!(["claude-code", "codex"])
    );
    assert!(
        deployed(&home, ".codex/skills", "gamma") && deployed(&home, ".claude/skills", "gamma")
    );

    // 已经是目标状态时什么也不做
    let again = home
        .json(&["groups", "enable", "笔记", "-a", "codex"])
        .json();
    assert_eq!(again["changed"], false);
    assert_eq!(again["unchanged"], serde_json::json!(["codex"]));
    assert!(again["reload"].is_null());

    home.ok(&["groups", "disable", "笔记", "-a", "codex"]);
    assert!(!deployed(&home, ".codex/skills", "alpha"));
    assert!(
        deployed(&home, ".claude/skills", "alpha"),
        "停用一个 Agent 不影响另一个"
    );

    let removed = home.ok(&["groups", "remove", "笔记", "beta"]);
    assert!(
        removed.contains("已从分组「笔记」移出 beta，并写入 Claude Code"),
        "{removed}"
    );
    assert!(!deployed(&home, ".claude/skills", "beta"));

    // 正在使用的分组不能删；停用后还要 --yes 确认
    let busy = home.run(&["groups", "delete", "笔记"]);
    assert_eq!(busy.code, 1);
    assert!(busy.err.contains("请先停用"), "{}", busy.err);
    home.ok(&["groups", "disable", "笔记", "-a", "claude-code"]);
    let unconfirmed = home.json(&["groups", "delete", "笔记"]);
    assert_eq!(unconfirmed.code, 5);
    assert_eq!(unconfirmed.json()["error"]["code"], "needs_confirmation");
    home.ok(&["groups", "delete", "笔记", "--yes"]);
    assert!(home.ok(&["groups", "list"]).contains("共 0 个分组"));
    assert!(
        home.path(".skill-studio/skills/alpha/SKILL.md").is_file(),
        "删除分组不删 Hub 里的 skill"
    );
}

#[test]
#[serial]
fn names_resolve_to_one_thing_or_list_the_candidates() {
    let home = Home::new();
    home.hub("pdf-tools");
    home.skill(".claude/skills", "pdf-tools");
    home.skill(".claude/skills", "twin");
    home.skill(".codex/skills", "twin");
    home.ok(&["groups", "create", "笔记"]);

    let unknown = home.json(&["groups", "show", "笔"]);
    assert_eq!(unknown.code, 3);
    assert_eq!(
        unknown.json()["error"]["candidates"],
        serde_json::json!(["笔记"])
    );

    // 同名时托管在 Hub 里的那份就是「这个 skill」
    let shown = home.json(&["skills", "show", "pdf-tools"]).json();
    assert_eq!(shown["origin"], "hub");

    // 两个 Agent 目录里各有一份原件：报歧义，列出位置，让调用方写目录
    let twin = home.run(&["skills", "show", "twin"]);
    assert_eq!(twin.code, 3);
    assert!(
        twin.err.contains("~/.claude/skills/twin") && twin.err.contains("~/.codex/skills/twin"),
        "{}",
        twin.err
    );
    let by_path = home
        .json(&["skills", "show", "~/.codex/skills/twin"])
        .json();
    assert_eq!(by_path["owner"], "codex");

    let agent = home.run(&["groups", "enable", "笔记", "-a", "cursor"]);
    assert_eq!(agent.code, 3);
    assert!(agent.err.contains("claude-code"));
}

#[test]
#[serial]
fn duplicate_project_names_require_a_path_or_id_before_writing() {
    let home = Home::new();
    let first = home.path("work/one/app");
    let second = home.path("work/two/app");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    // CLI 项目目录用 dunce::canonicalize 去掉 Windows 的 \\?\ 前缀。
    let first_path = dunce::canonicalize(&first)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let second_path = dunce::canonicalize(&second)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let first_id = home.json(&["projects", "create", &first_path]).json()["project"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let second_id = home.json(&["projects", "create", &second_path]).json()["project"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    for args in [
        vec!["projects", "show", "app"],
        vec!["projects", "delete", "app", "--yes"],
        vec!["mcp", "projects", "show", "app"],
    ] {
        let ambiguous = home.json(&args);
        assert_eq!(
            ambiguous.code, 3,
            "{args:?}: {}{}",
            ambiguous.out, ambiguous.err
        );
        let error = &ambiguous.json()["error"];
        assert_eq!(error["code"], "not_found");
        let candidates = error["candidates"].as_array().unwrap();
        assert!(candidates
            .iter()
            .any(|c| c.as_str().unwrap().contains(&first_path)));
        assert!(candidates
            .iter()
            .any(|c| c.as_str().unwrap().contains(&second_path)));
    }
    assert_eq!(
        home.json(&["projects", "list"])
            .json()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        home.json(&["projects", "show", &first_id]).json()["root"],
        first_path
    );
    assert_eq!(
        home.json(&["projects", "show", &second_path]).json()["id"],
        second_id
    );
}

#[test]
#[serial]
fn projects_save_members_and_write_only_the_agents_enabled() {
    let home = Home::new();
    home.hub("alpha");
    home.hub("beta");
    home.ok(&["groups", "create", "笔记", "-s", "beta"]);
    let root = home.path("work/app");
    std::fs::create_dir_all(root.join("src")).unwrap();
    let root_arg = root.to_string_lossy().into_owned();
    let created = home.json(&["projects", "create", &root_arg]).json();
    assert_eq!(created["project"]["name"], "app");

    let saved = home
        .json(&[
            "projects", "add", "app", "--skills", "alpha", "--groups", "笔记",
        ])
        .json();
    assert_eq!(saved["written"], serde_json::json!([]));
    assert!(saved["message"].as_str().unwrap().contains("只保存了成员"));
    assert!(!root.join(".claude/skills/alpha").exists());

    home.ok(&["projects", "enable", "app", "-a", "claude-code"]);
    assert!(root.join(".claude/skills/alpha/SKILL.md").is_file());
    assert!(
        root.join(".claude/skills/beta/SKILL.md").is_file(),
        "绑定分组的成员一起写入"
    );

    // 在项目的子目录里，「.」就是这个项目
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(root.join("src")).unwrap();
    let shown = home.json(&["projects", "show", "."]);
    std::env::set_current_dir(previous).unwrap();
    assert_eq!(shown.json()["name"], "app");
    assert_eq!(shown.json()["enabled"], serde_json::json!(["claude-code"]));

    // 移出成员即写入已启用的 Agent
    home.ok(&["projects", "remove", "app", "--groups", "笔记"]);
    assert!(!root.join(".claude/skills/beta").exists());

    let busy = home.run(&["projects", "delete", "app", "--yes"]);
    assert_eq!(busy.code, 1);
    home.ok(&["projects", "disable", "app", "-a", "claude"]);
    assert!(!root.join(".claude/skills/alpha").exists());
    assert_eq!(home.run(&["projects", "delete", "app"]).code, 5);
    home.ok(&["projects", "delete", "app", "-y"]);
    assert!(root.join("src").is_dir(), "项目目录里的文件不动");
}

#[test]
#[serial]
fn status_and_doctor_report_what_needs_attention_like_the_desktop() {
    let home = Home::new();
    home.hub("alpha");
    home.hub("beta");
    home.ok(&["groups", "create", "笔记", "-s", "alpha,beta"]);
    home.ok(&["groups", "enable", "笔记", "-a", "codex"]);
    std::fs::write(home.path(".codex/skills/alpha/SKILL.md"), "本地改过").unwrap();
    let source = home.path(".skill-studio/skills/beta/SKILL.md");
    let mut text = std::fs::read_to_string(&source).unwrap();
    text.push_str("\n源更新\n");
    std::fs::write(&source, text).unwrap();

    let status = home.json(&["status"]).json();
    let attention = status["attention"].as_array().unwrap();
    assert!(attention.iter().any(|a| a["kind"] == "issue"
        && a["agent"] == "codex"
        && a["detail"]
            .as_str()
            .unwrap()
            .contains("alpha 的副本已被修改")));
    assert!(attention
        .iter()
        .any(|a| a["kind"] == "pending" && a["fix"] == "skill-studio groups apply 笔记"));
    assert_eq!(status["groups"][0]["enabled"], serde_json::json!(["codex"]));
    let text = home.ok(&["status"]);
    assert!(
        text.contains("需要检查（1）") && text.contains("有待应用修改（1）"),
        "{text}"
    );

    let doctor = home.json(&["doctor"]);
    assert_eq!(doctor.code, 0, "只是提醒，不算失败");
    let checks = doctor.json()["checks"].as_array().unwrap().clone();
    assert!(checks
        .iter()
        .any(|c| c["title"] == "分组与项目" && c["level"] == "warn"));

    // 应用修改：被改过的副本原样留下并报告
    let applied = home.json(&["groups", "apply", "笔记"]).json();
    assert_eq!(applied["untouched"][0]["name"], "alpha");
    assert!(home.ok(&["groups", "apply", "笔记"]).contains("原样留下"));
}

#[cfg(unix)]
fn tree(root: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.symlink_metadata().unwrap().is_dir() {
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
fn writable(root: &Path, on: bool) {
    use std::os::unix::fs::PermissionsExt;
    let mut stack = vec![root.to_path_buf()];
    let mut dirs = vec![root.to_path_buf()];
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
                let mode = if on { 0o644 } else { 0o444 };
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            }
        }
    }
    for dir in dirs {
        let mode = if on { 0o755 } else { 0o555 };
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
    }
}

#[test]
#[serial]
#[cfg(unix)]
fn viewing_commands_work_in_a_read_only_home_and_write_nothing() {
    let home = Home::new();
    home.hub("alpha");
    home.ok(&["groups", "create", "笔记", "-s", "alpha"]);
    home.ok(&["groups", "enable", "笔记", "-a", "codex"]);
    let root = home.path("work/app");
    std::fs::create_dir_all(&root).unwrap();
    home.ok(&["projects", "create", &root.to_string_lossy()]);

    let before = tree(home.dir.path());
    writable(home.dir.path(), false);
    let reads: Vec<(Vec<&str>, i32)> = [
        vec!["status"],
        vec!["agents"],
        vec!["skills", "list"],
        vec!["skills", "show", "alpha", "--content"],
        vec!["groups", "list"],
        vec!["groups", "show", "笔记"],
        vec!["projects", "list"],
        vec!["projects", "show", "app"],
        vec!["doctor"],
    ]
    .into_iter()
    .map(|args| {
        let code = home.run(&args).code;
        (args, code)
    })
    .collect();
    let write = home.run(&["groups", "create", "新分组"]);
    writable(home.dir.path(), true);
    for (args, code) in reads {
        assert_eq!(code, 0, "只读时 {args:?} 失败");
    }
    assert_eq!(write.code, 1);
    assert!(write.err.contains("沙箱"), "{}", write.err);
    assert_eq!(tree(home.dir.path()), before, "只读时写了文件");
}

#[test]
#[serial]
fn a_write_waits_for_the_desktop_then_gives_up_with_exit_code_4() {
    let home = Home::new();
    home.ok(&["groups", "create", "已有"]);
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(home.path(".skill-studio/config.json.lock"))
        .unwrap();
    fs2::FileExt::lock_exclusive(&lock).unwrap();
    std::env::set_var("SKILL_STUDIO_LOCK_WAIT_MS", "200");
    let busy = home.json(&["groups", "create", "新的"]);
    std::env::remove_var("SKILL_STUDIO_LOCK_WAIT_MS");
    assert_eq!(busy.code, 4);
    let error = &busy.json()["error"];
    assert_eq!(error["code"], "busy");
    assert!(!error["message"].as_str().unwrap().starts_with("无效输入"));
    drop(lock);
    home.ok(&["groups", "create", "新的"]);
}

#[test]
#[serial]
fn agent_directories_follow_the_desktop_not_the_shell_environment() {
    let home = Home::new();
    home.hub("alpha");
    home.ok(&["groups", "create", "笔记", "-s", "alpha"]);
    let elsewhere = home.path("elsewhere");
    std::env::set_var("CODEX_HOME", &elsewhere);
    home.ok(&["groups", "enable", "笔记", "-a", "codex"]);
    assert!(
        deployed(&home, ".codex/skills", "alpha"),
        "与桌面应用写同一个目录"
    );
    assert!(!elsewhere.join("skills/alpha").exists());

    std::env::set_var("CODEX_HOME", &elsewhere);
    let doctor = home.json(&["doctor"]).json();
    std::env::remove_var("CODEX_HOME");
    assert!(doctor["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["level"] == "warn" && c["detail"].as_str().unwrap().contains("CODEX_HOME")));
}

#[test]
#[serial]
fn local_skills_are_imported_into_the_hub_and_writes_are_logged() {
    let home = Home::new();
    let one = home.skill("downloads", "my-skill");
    home.skill("bundle", "first");
    home.skill("bundle", "second");

    let imported = home
        .json(&["skills", "import", &one.to_string_lossy()])
        .json();
    assert_eq!(imported["skill"]["name"], "my-skill");
    assert!(home
        .path(".skill-studio/skills/my-skill/SKILL.md")
        .is_file());
    let listed = home.json(&["skills", "list", "--hub"]).json();
    assert_eq!(listed[0]["name"], "my-skill");

    let several = home.run(&["skills", "import", &home.path("bundle").to_string_lossy()]);
    assert_eq!(several.code, 2);
    assert!(
        several.err.contains("first") && several.err.contains("second"),
        "{}",
        several.err
    );

    let usage = home.run(&["skills", "install", "owner/repo"]);
    assert_eq!(usage.code, 2, "没写明是仓库里的哪个 skill");

    let log = std::fs::read_to_string(home.path(".skill-studio/logs/cli.log")).unwrap();
    assert!(
        log.contains("skills import") && log.contains("已把「my-skill」导入 Skill Hub"),
        "{log}"
    );
    assert!(
        log.contains("skills import") && log.contains("失败"),
        "失败的修改也记下来：{log}"
    );
    assert!(!log.contains("skills list"), "查看命令不记日志");
}

#[test]
#[serial]
fn reimporting_a_changed_local_skill_updates_the_hub_copy_without_overwriting_edits() {
    let home = Home::new();
    let source = home.skill(".", "mine");
    let first = home.json(&["skills", "import", "~/mine"]);
    assert_eq!(first.code, 0, "{}", first.err);
    assert_eq!(first.json()["changed"], true);

    // 原目录没变：明确回答「没有变化」，不能一律报成功
    let unchanged = home.json(&["skills", "import", "~/mine"]);
    assert_eq!(unchanged.code, 0, "{}", unchanged.err);
    assert_eq!(unchanged.json()["changed"], false, "{}", unchanged.out);

    // 原目录更新了，Hub 副本还没被用户改过：跟着更新
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: mine\ndescription: v2\n---\n\nv2\n",
    )
    .unwrap();
    let updated = home.json(&["skills", "import", "~/mine"]);
    assert_eq!(updated.code, 0, "{}", updated.err);
    assert_eq!(updated.json()["changed"], true);
    assert!(
        std::fs::read_to_string(home.path(".skill-studio/skills/mine/SKILL.md"))
            .unwrap()
            .contains("v2")
    );

    // 用户改过 Hub 副本：不覆盖他的修改
    std::fs::write(
        home.path(".skill-studio/skills/mine/SKILL.md"),
        "---\nname: mine\n---\n\nmine\n",
    )
    .unwrap();
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: mine\ndescription: v3\n---\n\nv3\n",
    )
    .unwrap();
    let refused = home.json(&["skills", "import", "~/mine"]);
    assert_ne!(refused.code, 0, "{}", refused.out);
    assert!(refused.err.contains("已被修改"), "{}", refused.err);
    assert!(
        std::fs::read_to_string(home.path(".skill-studio/skills/mine/SKILL.md"))
            .unwrap()
            .contains("mine")
    );
}

#[test]
#[serial]
fn reimporting_a_hub_copy_already_matching_the_source_refreshes_its_hash() {
    let home = Home::new();
    let source = home.skill(".", "mine");
    home.ok(&["skills", "import", "~/mine"]);
    let hub = home.path(".skill-studio/skills/mine/SKILL.md");
    let v2 = "---\nname: mine\ndescription: v2\n---\n\nv2\n";
    std::fs::write(source.join("SKILL.md"), v2).unwrap();
    std::fs::write(&hub, v2).unwrap();
    let already_equal = home.json(&["skills", "import", "~/mine"]);
    assert_eq!(already_equal.code, 0, "{}", already_equal.err);
    assert_eq!(already_equal.json()["changed"], false);

    // Explicitly importing matching content acknowledges it as the current Hub copy;
    // the next source update must not be mistaken for an unapproved Hub edit.
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: mine\ndescription: v3\n---\n\nv3\n",
    )
    .unwrap();
    let next = home.json(&["skills", "import", "~/mine"]);
    assert_eq!(next.code, 0, "{}", next.err);
    assert_eq!(next.json()["changed"], true);
    assert!(std::fs::read_to_string(hub).unwrap().contains("v3"));
}

#[test]
#[serial]
fn retrying_an_enable_after_a_failed_write_reports_pending_work() {
    let home = Home::new();
    home.hub("alpha");
    home.hub("beta");
    home.ok(&["groups", "create", "笔记", "-s", "alpha"]);
    home.ok(&["groups", "enable", "笔记", "-a", "codex"]);
    // 目标位置已有用户自己的 beta：成员能存下，写入会失败
    home.skill(".codex/skills", "beta");
    let failed = home.run(&["groups", "add", "笔记", "beta"]);
    assert_eq!(failed.code, 1, "{}{}", failed.out, failed.err);

    // 重跑：分组确实没变，但要说清楚 Codex 上还欠着写入
    let retry = home.json(&["groups", "enable", "笔记", "-a", "codex"]);
    assert_eq!(retry.code, 0, "{}", retry.err);
    let value = retry.json();
    assert_eq!(value["changed"], false);
    assert_eq!(value["pending"], true, "{value}");
    let message = value["message"].as_str().unwrap();
    assert!(message.contains("待应用修改"), "{message}");
    assert!(message.contains("groups apply"), "{message}");
}

#[test]
#[serial]
fn retrying_a_project_enable_after_a_group_change_reports_pending_work() {
    let home = Home::new();
    home.hub("alpha");
    home.hub("beta");
    home.ok(&["groups", "create", "笔记", "-s", "alpha"]);
    let root = home.path("work/app");
    std::fs::create_dir_all(&root).unwrap();
    let root_arg = root.to_string_lossy().into_owned();
    home.ok(&["projects", "create", &root_arg]);
    home.ok(&["projects", "add", "app", "--groups", "笔记"]);
    home.ok(&["projects", "enable", "app", "-a", "claude-code"]);
    // 绑定的分组加了成员：项目部署落后，处于「有待应用修改」
    home.ok(&["groups", "add", "笔记", "beta"]);
    let listed = home.json(&["projects", "list"]).json();
    assert_eq!(listed[0]["pending"], true, "{listed}");

    let retry = home.json(&["projects", "enable", "app", "-a", "claude-code"]);
    assert_eq!(retry.code, 0, "{}", retry.err);
    let value = retry.json();
    assert_eq!(value["changed"], false);
    assert_eq!(value["pending"], true, "{value}");
    let message = value["message"].as_str().unwrap();
    assert!(message.contains("待应用修改"), "{message}");
    assert!(message.contains("projects apply"), "{message}");
}

#[test]
#[serial]
fn viewing_commands_never_leave_a_lock_file_behind() {
    let home = Home::new();
    home.hub("alpha");
    home.ok(&["groups", "create", "笔记", "-s", "alpha"]);
    // 写入时创建的锁文件删掉后，查看命令不能重新造一个
    std::fs::remove_file(home.path(".skill-studio/config.json.lock")).unwrap();
    for args in [
        vec!["status"],
        vec!["agents"],
        vec!["skills", "list"],
        vec!["skills", "show", "alpha"],
        vec!["groups", "list"],
        vec!["groups", "show", "笔记"],
        vec!["projects", "list"],
        vec!["doctor"],
    ] {
        home.ok(&args);
        assert!(
            !home.path(".skill-studio/config.json.lock").exists(),
            "{args:?} 创建了锁文件"
        );
    }
}

#[test]
#[serial]
fn a_member_that_left_the_hub_is_reported_with_a_way_out() {
    let home = Home::new();
    home.hub("alpha");
    home.hub("beta");
    home.ok(&["groups", "create", "笔记", "-s", "alpha,beta"]);
    home.ok(&["groups", "enable", "笔记", "-a", "codex"]);
    std::fs::remove_dir_all(home.path(".skill-studio/skills/beta")).unwrap();

    let enable = home.json(&["groups", "enable", "笔记", "-a", "claude-code"]);
    assert_eq!(enable.code, 3);
    let error = &enable.json()["error"];
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("分组成员已不存在"),
        "{error}"
    );
    assert!(
        error["hint"].as_str().unwrap().contains("groups show"),
        "{error}"
    );

    // 提示里的出路确实可用：按 ID 移出缺失的成员
    let shown = home.json(&["groups", "show", "笔记"]).json();
    let missing = shown["missing"][0].as_str().unwrap().to_string();
    home.ok(&["groups", "remove", "笔记", &missing]);
    home.ok(&["groups", "enable", "笔记", "-a", "claude-code"]);
}

#[test]
#[serial]
fn removing_by_name_prefers_the_member_that_left_the_hub_over_a_new_namesake() {
    let home = Home::new();
    let old = home.hub("alpha");
    home.ok(&["groups", "create", "笔记", "-s", "alpha"]);
    // 原来的 alpha 被删掉，另一个目录里又有一个同名的 skill（ID 不同）
    std::fs::remove_dir_all(&old).unwrap();
    home.skill(".claude/skills", "alpha");
    let shown = home.json(&["groups", "show", "笔记"]).json();
    assert_eq!(shown["missing"].as_array().unwrap().len(), 1);

    // 分组里只剩 ID，按名称移不掉：明确告诉调用方改用 ID，而不是悄悄说「不在分组里」
    let by_name = home.json(&["groups", "remove", "笔记", "alpha"]).json();
    assert_eq!(by_name["changed"], false);
    let missing = by_name["missing"][0].as_str().unwrap().to_string();
    assert!(
        by_name["message"]
            .as_str()
            .unwrap()
            .contains("只能按 ID 移出"),
        "{by_name}"
    );
    assert!(by_name["message"].as_str().unwrap().contains(&missing));

    home.ok(&["groups", "remove", "笔记", &missing]);
    let after = home.json(&["groups", "show", "笔记"]).json();
    assert_eq!(after["members"].as_array().unwrap().len(), 0, "{after}");
    assert!(
        home.path(".claude/skills/alpha/SKILL.md").is_file(),
        "同名的另一个 skill 不该被动"
    );
}

#[test]
#[serial]
fn clearing_the_last_member_of_a_group_in_use_says_how_to_stop_it_first() {
    let home = Home::new();
    home.hub("alpha");
    home.ok(&["groups", "create", "笔记", "-s", "alpha"]);
    home.ok(&["groups", "enable", "笔记", "-a", "codex"]);

    let refused = home.json(&["groups", "remove", "笔记", "alpha"]);
    assert_eq!(refused.code, 1);
    let error = &refused.json()["error"];
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("清空成员前请先为这些 Agent 停用"),
        "{error}"
    );
    assert!(
        error["hint"]
            .as_str()
            .unwrap()
            .contains("groups disable 笔记"),
        "{error}"
    );
    // 拒绝时什么都不改
    let shown = home.json(&["groups", "show", "笔记"]).json();
    assert_eq!(shown["members"].as_array().unwrap().len(), 1);

    home.ok(&["groups", "disable", "笔记", "-a", "codex"]);
    home.ok(&["groups", "remove", "笔记", "alpha"]);
    let after = home.json(&["groups", "show", "笔记"]).json();
    assert_eq!(after["members"].as_array().unwrap().len(), 0);
}

#[test]
#[serial]
fn paths_printed_with_a_tilde_can_be_pasted_back() {
    let home = Home::new();
    std::fs::create_dir_all(home.path("proj")).unwrap();

    let created = home.json(&["projects", "create", "~/proj"]);
    assert_eq!(created.code, 0, "{}", created.err);
    let root = created.json()["project"]["root"]
        .as_str()
        .unwrap()
        .to_string();
    // 两边都按当前平台解析：Windows 会带上 \\?\ 前缀，macOS 会把 /var 指到 /private
    assert_eq!(
        std::fs::canonicalize(root).unwrap(),
        home.path("proj").canonicalize().unwrap()
    );

    home.skill(".", "myskill");
    let imported = home.json(&["skills", "import", "~/myskill"]);
    assert_eq!(imported.code, 0, "{}", imported.err);

    // 目录不在时是一句能看懂的「找不到」，不是拼错的绝对路径和 IO 错误
    for args in [
        vec!["projects", "create", "~/nope"],
        vec!["skills", "import", "~/nope"],
    ] {
        let missing = home.json(&args);
        assert_eq!(missing.code, 3, "{args:?}");
        let message = missing.json()["error"]["message"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(message.contains("目录不存在：~/nope"), "{message}");
    }
}

#[test]
#[serial]
fn a_retry_after_a_failed_write_says_the_deployment_is_still_pending() {
    let home = Home::new();
    home.hub("alpha");
    home.hub("beta");
    home.ok(&["groups", "create", "笔记", "-s", "alpha"]);
    home.ok(&["groups", "enable", "笔记", "-a", "codex"]);

    // Codex 退出管理：成员能存下，写入会失败
    let mut config = home.config();
    config["settings"]["disabledAgents"] = serde_json::json!(["codex"]);
    home.save_config(&config);
    let first = home.run(&["groups", "add", "笔记", "beta"]);
    assert_eq!(first.code, 1, "{}{}", first.out, first.err);
    assert!(first.err.contains("已把 beta 加入分组"), "{}", first.err);

    // 重跑：分组确实没变，但要说明 Codex 上还欠着，不能只说「没有变化」
    let again = home.json(&["groups", "add", "笔记", "beta"]);
    assert_eq!(again.code, 0, "{}", again.err);
    let value = again.json();
    assert_eq!(value["changed"], false);
    assert_eq!(value["pending"], true, "{value}");
    let message = value["message"].as_str().unwrap();
    assert!(message.contains("待应用修改"), "{message}");
    assert!(message.contains("groups apply"), "{message}");
}

#[test]
#[serial]
fn removing_a_member_that_left_the_hub_points_at_its_id() {
    let home = Home::new();
    home.hub("alpha");
    home.ok(&["groups", "create", "笔记", "-s", "alpha"]);
    std::fs::remove_dir_all(home.path(".skill-studio/skills/alpha")).unwrap();
    let shown = home.json(&["groups", "show", "笔记"]).json();
    let missing = shown["missing"][0].as_str().unwrap().to_string();

    let output = home.json(&["groups", "remove", "笔记", "alpha"]);
    assert_eq!(output.code, 3, "{}{}", output.out, output.err);
    let error = &output.json()["error"];
    assert!(error["message"]
        .as_str()
        .unwrap()
        .contains("找不到 skill「alpha」"));
    let hint = error["hint"].as_str().unwrap();
    assert!(hint.contains("只能按 ID 移出"), "{error}");
    assert!(hint.contains(&missing), "{error}");
    // 提示里的出路确实可用
    home.ok(&["groups", "remove", "笔记", &missing]);

    // 项目里同样
    let project = home.path("proj");
    std::fs::create_dir_all(&project).unwrap();
    home.ok(&[
        "projects",
        "create",
        project.to_str().unwrap(),
        "--name",
        "示例",
    ]);
    home.hub("gamma");
    home.ok(&["projects", "add", "示例", "--skills", "gamma"]);
    std::fs::remove_dir_all(home.path(".skill-studio/skills/gamma")).unwrap();
    let shown = home.json(&["projects", "show", "示例"]).json();
    let missing = shown["missing"][0].as_str().unwrap().to_string();
    let output = home.json(&["projects", "remove", "示例", "--skills", "gamma"]);
    assert_eq!(output.code, 3, "{}{}", output.out, output.err);
    let hint = output.json()["error"]["hint"].as_str().unwrap().to_string();
    assert!(hint.contains("只能按 ID 移出"), "{hint}");
    assert!(hint.contains(&missing), "{hint}");
    home.ok(&["projects", "remove", "示例", "--skills", &missing]);
}

#[test]
#[serial]
fn a_builtin_skill_is_labelled_as_built_in() {
    let home = Home::new();
    let bundle = home.path("bundle");
    let source = bundle.join("skill-studio");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: skill-studio\ndescription: 内置\n---\n\n正文\n",
    )
    .unwrap();

    let store = skill_studio_core::services::store::Store::new(home.path(".skill-studio"));
    let studio = skill_studio_core::services::studio::Studio::new(store);
    let mut config = studio.load_config().unwrap();
    studio
        .sync_builtin_skills(&mut config, &bundle, "1.0.0")
        .unwrap();

    let listed = home.ok(&["skills", "list"]);
    assert!(listed.contains("Skill Studio 内置"), "{listed}");
    let shown = home.json(&["skills", "show", "skill-studio"]).json();
    assert_eq!(shown["origin"], "builtin", "{shown}");
}
