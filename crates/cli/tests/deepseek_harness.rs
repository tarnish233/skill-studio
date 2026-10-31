//! Public Skill CLI routes Harness through the same locked service as the desktop.
use serde_json::{json, Value};
use serial_test::serial;
use skill_studio_core::fs::paths;
use std::{fs, path::PathBuf};

struct Home(tempfile::TempDir);
impl Home {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, home.path());
        for key in [
            "DSH_HOME",
            "DSH_AGENTS_HOME",
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "XDG_CONFIG_HOME",
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_CONFIG",
            "PI_CODING_AGENT_DIR",
            "GROK_HOME",
            "SKILL_STUDIO_APP_EXECUTABLE",
            "SKILL_STUDIO_LOCK_WAIT_MS",
        ] {
            std::env::remove_var(key);
        }
        Self(home)
    }
    fn path(&self, path: &str) -> PathBuf {
        self.0.path().join(path)
    }
    fn skill(&self, path: &str, name: &str) -> PathBuf {
        let dir = self.path(path);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test\n---\nbody\n"),
        )
        .unwrap();
        dir
    }
    fn run(&self, args: &[&str]) -> (i32, Value) {
        let mut out = vec![];
        let mut err = vec![];
        let code = skill_studio_cli::run(
            std::iter::once("skill-studio")
                .chain(std::iter::once("--json"))
                .chain(args.iter().copied()),
            &mut out,
            &mut err,
        );
        let value = serde_json::from_slice(if code == 0 { &out } else { &err })
            .unwrap_or_else(|e| panic!("{e}: {out:?} {err:?}"));
        (code, value)
    }
    fn ok(&self, args: &[&str]) -> Value {
        let (code, value) = self.run(args);
        assert_eq!(code, 0, "{args:?}: {value}");
        value
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        std::env::remove_var(paths::TEST_HOME_ENV);
    }
}

#[test]
#[serial]
fn harness_alias_deploys_groups_and_projects_to_private_roots() {
    let home = Home::new();
    let source = home.skill("incoming/demo", "demo");
    home.ok(&["skills", "import", source.to_str().unwrap()]);
    home.ok(&["groups", "create", "Dev", "--skills", "demo"]);
    home.ok(&["groups", "enable", "Dev", "--agent", "dsh"]);
    assert!(home.path(".dsh/skills/demo/SKILL.md").is_file());
    assert!(!home.path(".agents/skills/demo").exists());
    home.ok(&["groups", "disable", "Dev", "--agent", "DeepSeek Harness"]);
    assert!(!home.path(".dsh/skills/demo").exists());
    let project = home.path("project");
    fs::create_dir(&project).unwrap();
    home.ok(&[
        "projects",
        "create",
        project.to_str().unwrap(),
        "--name",
        "App",
    ]);
    home.ok(&["projects", "add", "App", "--skills", "demo"]);
    home.ok(&["projects", "enable", "App", "--agent", "deepseek-harness"]);
    assert!(project.join(".dsh/skills/demo/SKILL.md").is_file());
    home.ok(&["projects", "disable", "App", "--agent", "dsh"]);
    assert!(!project.join(".dsh/skills/demo").exists());
}

#[test]
#[serial]
fn harness_settings_alias_and_agent_inventory_use_the_canonical_id() {
    let home = Home::new();
    let custom = home.path("custom-home");
    home.ok(&[
        "settings",
        "set",
        "--agent-dir",
        &format!("dsh={}", custom.display()),
    ]);
    let list = home.ok(&["agents"]);
    let harness = list
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "deepseek-harness")
        .unwrap();
    assert_eq!(harness["skillDirs"][0], json!(custom.join("skills")));
    assert_eq!(harness["projectSkillDir"], ".dsh/skills");
    home.ok(&["settings", "set", "--unmanage-agent", "dsh", "--yes"]);
    assert_eq!(
        home.ok(&["settings", "show"])["disabledAgents"],
        json!(["deepseek-harness"])
    );
    home.ok(&[
        "settings",
        "set",
        "--manage-agent",
        "DeepSeek Harness",
        "--yes",
    ]);
}

#[test]
#[serial]
fn harness_skill_adoption_and_restore_do_not_touch_profile_configuration() {
    let home = Home::new();
    let original = home.skill(".dsh/skills/manual", "manual");
    let profile = home.path(".dsh/cordis.patch.yml");
    let original_config = b"# user-owned dynamic YAML\n- config: !!js userExpression\n";
    fs::write(&profile, original_config).unwrap();
    home.ok(&["skills", "adopt", original.to_str().unwrap(), "--yes"]);
    assert!(home.path(".skill-studio/skills/manual/SKILL.md").is_file());
    home.ok(&["skills", "release", "manual", "--yes"]);
    assert!(original.join("SKILL.md").is_file());
    assert_eq!(fs::read(profile).unwrap(), original_config);
}

#[test]
#[serial]
fn harness_is_not_exposed_for_mcp_writes() {
    let home = Home::new();
    home.ok(&["mcp", "add", "one", "--", "echo"]);
    home.ok(&["mcp", "groups", "create", "MCP", "--mcp", "one"]);
    let (code, value) = home.run(&["mcp", "groups", "enable", "MCP", "--agent", "dsh"]);
    assert_ne!(code, 0);
    assert!(value.to_string().contains("不支持"), "{value}");
    assert!(!home.path(".dsh").exists());
}

#[test]
#[serial]
fn cli_isolates_and_reports_both_harness_home_variables() {
    let home = Home::new();
    let outside_private = home.path("outside-dsh");
    let outside_shared = home.path("outside-shared");
    std::env::set_var("DSH_HOME", &outside_private);
    std::env::set_var("DSH_AGENTS_HOME", &outside_shared);
    let agents = home.ok(&["agents"]);
    std::env::set_var("DSH_HOME", &outside_private);
    std::env::set_var("DSH_AGENTS_HOME", &outside_shared);
    let doctor = home.ok(&["doctor"]);
    std::env::remove_var("DSH_HOME");
    std::env::remove_var("DSH_AGENTS_HOME");
    let harness = agents
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "deepseek-harness")
        .unwrap();
    let roots: Vec<PathBuf> = serde_json::from_value(harness["skillDirs"].clone()).unwrap();
    assert_eq!(
        roots,
        vec![home.path(".dsh/skills"), home.path(".agents/skills")],
        "interactive CLI must isolate both Harness roots"
    );
    let details: Vec<&str> = doctor["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["detail"].as_str())
        .collect();
    for name in ["DSH_HOME", "DSH_AGENTS_HOME"] {
        assert!(
            details.iter().any(|detail| detail.contains(name)),
            "doctor must explain ignored {name}: {doctor}"
        );
    }
}
