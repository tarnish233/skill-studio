use serde_json::{json, Value};
use serial_test::serial;
use skill_studio_core::fs::paths;
use std::{ffi::OsString, fs, path::PathBuf};

struct Home {
    temp: tempfile::TempDir,
    saved: Vec<(&'static str, Option<OsString>)>,
}
impl Home {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let keys = [
            paths::TEST_HOME_ENV,
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "XDG_CONFIG_HOME",
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_CONFIG",
            "PI_CODING_AGENT_DIR",
            "GROK_HOME",
            "DSH_HOME",
            "DSH_AGENTS_HOME",
        ];
        let saved = keys
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect::<Vec<_>>();
        for (key, _) in &saved {
            std::env::remove_var(key);
        }
        std::env::set_var(paths::TEST_HOME_ENV, temp.path());
        Self { temp, saved }
    }
    fn path(&self, relative: &str) -> PathBuf {
        self.temp.path().join(relative)
    }
    fn put(&self, relative: &str, value: impl AsRef<[u8]>) {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = skill_studio_cli::run(
            std::iter::once("skill-studio").chain(args.iter().copied()),
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

#[test]
#[serial]
fn plugin_list_show_and_inspect_do_not_initialize_studio_or_echo_native_secrets() {
    let home = Home::new();
    home.put(
        ".pi/agent/settings.json",
        json!({"packages":[{"source":"npm:example","options":{"api_key":"SECRET_VALUE"}}]})
            .to_string(),
    );
    home.put(
        ".pi/agent/npm/node_modules/example/package.json",
        "{\"name\":\"example\",\"pi\":{\"extensions\":[\"./index.ts\"]}}",
    );
    home.put(
        ".pi/agent/npm/node_modules/example/index.ts",
        "throw new Error('must not execute');",
    );
    let (code, out, err) = home.run(&["plugins", "list", "--agent", "pi", "--json"]);
    assert_eq!(code, 0, "{err}");
    assert!(!out.contains("SECRET_VALUE"));
    let inventory: Value = serde_json::from_str(&out).unwrap();
    let id = inventory["plugins"][0]["id"].as_str().unwrap();
    assert_eq!(home.run(&["plugins", "show", id, "--json"]).0, 0);
    assert_eq!(
        home.run(&[
            "plugins",
            "inspect",
            home.path(".pi/agent/npm/node_modules/example")
                .to_str()
                .unwrap(),
            "--agent",
            "pi",
            "--json"
        ])
        .0,
        0
    );
    assert!(!home.path(".skill-studio").exists());
}

#[test]
#[serial]
fn plugin_install_confirmation_precedes_any_configuration_write() {
    let home = Home::new();
    let (code, _, err) = home.run(&[
        "plugins",
        "install",
        "npm:example",
        "--agent",
        "pi",
        "--json",
    ]);
    assert_eq!(code, 5);
    assert!(err.contains("needs_confirmation"));
    // Failed mutation attempts are audited, but never initialize configuration or plugins.
    assert!(!home.path(".skill-studio/config.json").exists());
    assert!(!home.path(".skill-studio/plugins").exists());
}

#[test]
#[serial]
fn invalid_standard_inspection_keeps_diagnostic_json_in_stdout_and_exits_nonzero() {
    let home = Home::new();
    home.put("bad/plugin.json", json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"INVALID"}).to_string());
    let (code, out, err) = home.run(&[
        "plugins",
        "inspect",
        home.path("bad").to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(code, 1);
    assert!(err.is_empty());
    let result: Value = serde_json::from_str(&out).unwrap();
    assert!(result["error"].is_string());
    assert!(!home.path(".skill-studio").exists());
}

#[test]
#[serial]
fn same_plugin_name_across_agents_requires_concrete_id() {
    let home = Home::new();
    home.put(
        ".pi/agent/settings.json",
        "{\"packages\":[\"npm:example\"]}",
    );
    home.put(
        ".pi/agent/npm/node_modules/example/package.json",
        "{\"name\":\"example\",\"pi\":{}}",
    );
    home.put(
        ".codex/config.toml",
        "[plugins.\"example@local\"]\nenabled=true\n",
    );
    let (code, _, err) = home.run(&["plugins", "show", "example", "--json"]);
    assert_eq!(code, 3);
    let error: Value = serde_json::from_str(&err).unwrap();
    assert_eq!(error["error"]["candidates"].as_array().unwrap().len(), 2);
    assert!(!home.path(".skill-studio").exists());
}

#[test]
#[serial]
fn plugin_management_switch_is_independent_and_not_an_install_operation() {
    let home = Home::new();
    let (code, _, err) = home.run(&["settings", "set", "--manage-plugins", "true", "--yes"]);
    assert_eq!(code, 0, "{err}");
    let path = home.path(".skill-studio/config.json");
    let config: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(config["settings"]["managePlugins"], true);
    assert_eq!(config["settings"]["manageMcp"], true);
    assert!(!home.path(".pi/agent/settings.json").exists());
    assert!(!home.path(".skill-studio/plugins").exists());
    assert_eq!(
        home.run(&[
            "settings",
            "set",
            "--manage-plugins",
            "false",
            "--theme",
            "dark",
            "--yes"
        ])
        .0,
        2
    );
}

#[test]
#[serial]
fn no_new_agent_or_unsupported_plugin_scope_is_accepted() {
    let home = Home::new();
    assert_eq!(
        home.run(&["plugins", "install", "source", "--agent", "gemini", "--yes"])
            .0,
        3
    );
    assert_eq!(
        home.run(&["plugins", "install", "source", "--agent", "pi", "--scope", "project", "--yes"])
            .0,
        2
    );
    assert!(!home.path(".skill-studio/config.json").exists());
    assert!(!home.path(".skill-studio/plugins").exists());
}
