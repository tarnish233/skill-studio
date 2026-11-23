use serde_json::{json, Value};
use serial_test::serial;
use skill_studio_core::{fs::paths, services::store::Store};
use skill_studio_service::{
    plugins::{self, PluginAction, PluginOperation, PluginView},
    state::AppState,
};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

fn put(path: impl AsRef<Path>, text: impl AsRef<[u8]>) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
struct Fixture {
    _temp: tempfile::TempDir,
    saved: Vec<(&'static str, Option<OsString>)>,
    home: PathBuf,
    state: AppState,
}
impl Drop for Fixture {
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
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let bin = home.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let keys = [
            paths::TEST_HOME_ENV,
            "PATH",
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_CONFIG",
            "XDG_CONFIG_HOME",
            "PI_CODING_AGENT_DIR",
            "GROK_HOME",
            "DSH_HOME",
            "DSH_AGENTS_HOME",
        ];
        let saved: Vec<_> = keys
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect();
        for (key, _) in &saved {
            std::env::remove_var(key);
        }
        std::env::set_var(paths::TEST_HOME_ENV, &home);
        let old_path = saved
            .iter()
            .find(|(key, _)| *key == "PATH")
            .unwrap()
            .1
            .clone()
            .unwrap_or_default();
        let mut path = vec![bin.clone()];
        path.extend(std::env::split_paths(&old_path));
        std::env::set_var("PATH", std::env::join_paths(path).unwrap());
        #[cfg(unix)]
        for (name, version) in [
            ("pi", "1.6.0"),
            ("opencode", "2.3.2"),
            ("claude", "2.1.268"),
        ] {
            let executable = bin.join(name);
            put(&executable, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo {version}; exit 0; fi\nif [ \"$1\" = --help ] || [ \"$2\" = --help ]; then echo 'install enable disable update uninstall add remove'; exit 0; fi\nprintf '%s\\n' \"$@\" > '{}'\nexit 0\n", home.join("native-args").display()));
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let state = AppState::bootstrap(Store::new(home.join("studio"))).unwrap();
        Self {
            _temp: temp,
            saved,
            home,
            state,
        }
    }
    fn manage(&self) {
        self.state
            .mutate(|_, config| {
                config.settings.manage_plugins = true;
                Ok(())
            })
            .unwrap();
    }
    fn pi(&self, entries: Value) -> PathBuf {
        let path = self.home.join(".pi/agent/settings.json");
        put(&path, json!({"packages":entries,"mcpServers":{"secret":{"command":"node","env":{"API_KEY":"MCP_SECRET"}}}}).to_string());
        let root = self.home.join(".pi/agent/npm/node_modules/pi-tools");
        put(root.join("package.json"), json!({"name":"pi-tools","version":"1.0.0","pi":{"extensions":["./extensions/tool.ts"],"skills":["./skills"]}}).to_string());
        put(
            root.join("extensions/tool.ts"),
            "throw new Error('MUST NOT EXECUTE');",
        );
        put(
            root.join("skills/review/SKILL.md"),
            "---\nname: review\ndescription: Review\n---\nReview.",
        );
        path
    }
    fn view(&self, agent: &str) -> PluginView {
        plugins::scan_plugins(&self.state)
            .unwrap()
            .plugins
            .into_iter()
            .find(|plugin| plugin.agent_id == agent)
            .unwrap()
    }
    fn run(
        &self,
        action: PluginAction,
        plugin: &PluginView,
    ) -> Result<plugins::PluginOperationResult, String> {
        plugins::plugin_operation(
            &self.state,
            PluginOperation {
                action,
                agent_id: plugin.agent_id.clone(),
                plugin_id: Some(plugin.id.clone()),
                expected_revision: Some(plugin.revision.clone()),
                source: None,
                marketplace: None,
                confirmed: true,
            },
        )
    }
}

#[test]
#[serial]
fn scanning_is_read_only_and_never_returns_native_options_or_credentials() {
    let fixture = Fixture::new();
    fixture.pi(json!([{"source":"npm:pi-tools","options":{"api_key":"PACKAGE_SECRET"}}]));
    let before = fs::read_dir(fixture.home.join("studio")).unwrap().count();
    let inventory = plugins::scan_plugins(&fixture.state).unwrap();
    assert_eq!(
        fs::read_dir(fixture.home.join("studio")).unwrap().count(),
        before
    );
    assert!(!fixture.home.join("native-args").exists());
    assert!(!fixture.home.join("studio/plugins").exists());
    let text = serde_json::to_string(&inventory).unwrap();
    assert!(!text.contains("PACKAGE_SECRET"));
    assert!(!text.contains("MCP_SECRET"));
    assert_eq!(inventory.plugins[0].package.components.len(), 2);
}

#[test]
#[serial]
fn plugin_writes_are_opt_in_and_confirmation_is_enforced() {
    let fixture = Fixture::new();
    let path = fixture.pi(json!(["npm:pi-tools"]));
    let before = fs::read_to_string(&path).unwrap();
    let plugin = fixture.view("pi");
    assert!(fixture
        .run(PluginAction::Disable, &plugin)
        .unwrap_err()
        .contains("未开启"));
    fixture.manage();
    let input = PluginOperation {
        action: PluginAction::Disable,
        agent_id: "pi".into(),
        plugin_id: Some(plugin.id),
        expected_revision: Some(plugin.revision),
        source: None,
        marketplace: None,
        confirmed: false,
    };
    assert!(plugins::plugin_operation(&fixture.state, input.clone())
        .unwrap_err()
        .contains("确认"));
    #[cfg(unix)]
    {
        let confirmed = PluginOperation {
            confirmed: true,
            ..input
        };
        assert!(plugins::plugin_operation(&fixture.state, confirmed).is_ok());
        assert_ne!(fs::read_to_string(&path).unwrap(), before);
    }
    #[cfg(not(unix))]
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

#[cfg(unix)]
#[test]
#[serial]
fn pi_pause_restores_original_resource_filters_and_preserves_mcp_and_package_files() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    fixture.manage();
    let entry = json!({"source":"npm:pi-tools","extensions":["extensions/tool.ts"],"skills":[],"custom":{"secret":"PACKAGE_SECRET"}});
    let path = fixture.pi(json!([entry.clone()]));
    let content = fs::read_to_string(&path).unwrap();
    put(&path, format!("// keep root comment\n{content}"));
    let plugin = fixture.view("pi");
    let root = plugin.package.root.clone();
    let code = fs::read(root.join("extensions/tool.ts")).unwrap();
    fixture.run(PluginAction::Disable, &plugin).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("keep root comment"));
    let paused: Value = jsonc_parser::parse_to_serde_value(&text, &Default::default()).unwrap();
    for field in ["extensions", "skills", "prompts", "themes"] {
        assert_eq!(paused["packages"][0][field], json!([]));
    }
    assert_eq!(
        paused["mcpServers"]["secret"]["env"]["API_KEY"],
        "MCP_SECRET"
    );
    assert_eq!(
        fs::metadata(fixture.home.join("studio/plugins/catalog.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    fixture
        .run(PluginAction::Enable, &fixture.view("pi"))
        .unwrap();
    let restored: Value =
        jsonc_parser::parse_to_serde_value(&fs::read_to_string(path).unwrap(), &Default::default())
            .unwrap();
    assert_eq!(restored["packages"][0], entry);
    assert_eq!(fs::read(root.join("extensions/tool.ts")).unwrap(), code);
    assert!(!fixture.home.join("native-args").exists());
}

#[test]
#[serial]
fn stale_confirmation_does_not_overwrite_external_edits() {
    let fixture = Fixture::new();
    fixture.manage();
    let path = fixture.pi(json!(["npm:pi-tools"]));
    let before = fixture.view("pi");
    let text = json!({"packages":[{"source":"npm:pi-tools","skills":["custom"]}]}).to_string();
    put(&path, &text);
    assert!(fixture
        .run(PluginAction::Disable, &before)
        .unwrap_err()
        .contains("外部修改"));
    assert_eq!(fs::read_to_string(path).unwrap(), text);
    assert!(!fixture.home.join("native-args").exists());
}

#[cfg(unix)]
#[test]
#[serial]
fn externally_edited_paused_entry_can_only_be_forgotten_without_changing_native_config() {
    let fixture = Fixture::new();
    fixture.manage();
    let path = fixture.pi(json!(["npm:pi-tools"]));
    fixture
        .run(PluginAction::Disable, &fixture.view("pi"))
        .unwrap();
    let external = json!({"packages":[{"source":"npm:pi-tools","skills":["custom"]}]}).to_string();
    put(&path, &external);
    let plugin = fixture.view("pi");
    assert_eq!(plugin.actions, [PluginAction::Forget]);
    assert!(fixture.run(PluginAction::Enable, &plugin).is_err());
    fixture.run(PluginAction::Forget, &plugin).unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), external);
}

#[cfg(unix)]
#[test]
#[serial]
fn opencode_pause_removes_only_the_package_declaration_without_guessing_plugin_id() {
    let fixture = Fixture::new();
    fixture.manage();
    let root = fixture.home.join(".config/opencode");
    let package = root.join("node_modules/opencode-package");
    put(
        package.join("package.json"),
        "{\"name\":\"opencode-package\",\"version\":\"1.0.0\"}",
    );
    put(
        package.join("index.ts"),
        "throw new Error('do not infer id by executing me');",
    );
    let entry = json!({"package":"opencode-package","options":{"integer":9223372036854775811u64,"api_key":"SECRET"}});
    let path = root.join("opencode.jsonc");
    put(
        &path,
        format!(
            "// preserve comment\n{}",
            json!({"plugins":[entry.clone(),"-dynamic.plugin.id"],"mcp":{"servers":{"docs":{"type":"remote","url":"https://example.com"}}}})
        ),
    );
    fixture
        .run(PluginAction::Disable, &fixture.view("opencode"))
        .unwrap();
    let paused: Value = jsonc_parser::parse_to_serde_value(
        &fs::read_to_string(&path).unwrap(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(paused["plugins"], json!(["-dynamic.plugin.id"]));
    assert!(package.exists());
    let plugin = fixture.view("opencode");
    assert!(!plugin.configured);
    assert_eq!(plugin.enabled, Some(false));
    fixture.run(PluginAction::Enable, &plugin).unwrap();
    let restored: Value = jsonc_parser::parse_to_serde_value(
        &fs::read_to_string(&path).unwrap(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(restored["plugins"][0], entry);
    assert!(fs::read_to_string(path)
        .unwrap()
        .contains("preserve comment"));
    assert!(!fixture.home.join("native-args").exists());
}

#[cfg(unix)]
#[test]
#[serial]
fn opencode_local_install_writes_the_resolved_path_and_keeps_the_package_in_place() {
    let fixture = Fixture::new();
    fixture.manage();
    let package = fixture.home.join("external-plugin");
    put(
        package.join("package.json"),
        "{\"name\":\"external-plugin\"}",
    );
    put(package.join("index.ts"), "export default () => {};");
    let result = plugins::plugin_operation(
        &fixture.state,
        PluginOperation {
            action: PluginAction::Install,
            agent_id: "opencode".into(),
            plugin_id: None,
            expected_revision: None,
            source: Some(package.to_string_lossy().into_owned()),
            marketplace: None,
            confirmed: true,
        },
    )
    .unwrap();
    assert!(!result.pending);
    let document: Value = jsonc_parser::parse_to_serde_value(
        &fs::read_to_string(fixture.home.join(".config/opencode/opencode.jsonc")).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let expected = package.canonicalize().unwrap();
    assert_eq!(document["plugins"], json!([expected.to_string_lossy()]));
    assert!(package.join("index.ts").exists());
    assert!(!fixture.home.join("native-args").exists());
}

#[cfg(unix)]
#[test]
#[serial]
fn native_install_receipt_without_config_change_is_pending_not_a_claim_of_installation() {
    let fixture = Fixture::new();
    fixture.manage();
    let result = plugins::plugin_operation(
        &fixture.state,
        PluginOperation {
            action: PluginAction::Install,
            agent_id: "pi".into(),
            plugin_id: None,
            expected_revision: None,
            source: Some("npm:new-package".into()),
            marketplace: None,
            confirmed: true,
        },
    )
    .unwrap();
    assert!(result.pending);
    assert!(!result.changed);
    assert!(fs::read_to_string(fixture.home.join("native-args"))
        .unwrap()
        .contains("--no-approve"));
}

#[cfg(unix)]
#[test]
#[serial]
fn credentials_and_option_injection_are_rejected_without_running_native_install() {
    let fixture = Fixture::new();
    fixture.manage();
    for source in [
        "https://token:SECRET@example.com/repo.git",
        "https://example.com/repo?token=SECRET",
        " --all",
        "--yes",
        "npm:bad\n--all",
    ] {
        let result = plugins::plugin_operation(
            &fixture.state,
            PluginOperation {
                action: PluginAction::Install,
                agent_id: "pi".into(),
                plugin_id: None,
                expected_revision: None,
                source: Some(source.into()),
                marketplace: None,
                confirmed: true,
            },
        );
        assert!(result.is_err());
        assert!(!result.unwrap_err().contains("SECRET"));
    }
    assert!(!fixture.home.join("native-args").exists());
}

#[cfg(unix)]
#[test]
#[serial]
fn opencode_legacy_and_current_plugin_fields_are_both_scanned_and_managed() {
    let fixture = Fixture::new();
    fixture.manage();
    let root = fixture.home.join(".config/opencode");
    let package = root.join("legacy-plugin");
    put(package.join("package.json"), "{\"name\":\"legacy-plugin\"}");
    put(
        root.join("opencode.jsonc"),
        json!({
            "plugin": [format!("file://{}", package.display())],
            "plugins": ["npm-history-pkg"]
        })
        .to_string(),
    );
    let inventory = plugins::scan_plugins(&fixture.state).unwrap();
    let ids: Vec<&str> = inventory
        .plugins
        .iter()
        .filter(|plugin| plugin.agent_id == "opencode")
        .map(|plugin| plugin.source.as_str())
        .collect();
    assert!(ids.contains(&"npm-history-pkg"));
    assert!(ids.iter().any(|source| source.ends_with("legacy-plugin")));
    // 新旧键并存不再是整页只读：旧键条目可精确启停。
    let legacy = inventory
        .plugins
        .iter()
        .find(|plugin| plugin.source.ends_with("legacy-plugin"))
        .unwrap()
        .clone();
    assert!(legacy.actions.contains(&PluginAction::Disable));
    fixture.run(PluginAction::Disable, &legacy).unwrap();
    let paused: Value = jsonc_parser::parse_to_serde_value(
        &fs::read_to_string(root.join("opencode.jsonc")).unwrap(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(paused["plugin"], json!([]));
    assert_eq!(paused["plugins"], json!(["npm-history-pkg"]));
    // 停用后配置已变，旧视图的 revision 过期，需按新清单重新确认。
    let paused_view = plugins::scan_plugins(&fixture.state)
        .unwrap()
        .plugins
        .into_iter()
        .find(|plugin| plugin.source.ends_with("legacy-plugin"))
        .unwrap();
    fixture.run(PluginAction::Enable, &paused_view).unwrap();
    let restored: Value = jsonc_parser::parse_to_serde_value(
        &fs::read_to_string(root.join("opencode.jsonc")).unwrap(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        restored["plugin"],
        json!([format!("file://{}", package.display())])
    );
    assert_eq!(restored["plugins"], json!(["npm-history-pkg"]));
    assert!(!fixture.home.join("native-args").exists());
}

#[test]
#[serial]
fn opencode_npm_cache_packages_are_resolved_not_treated_as_id_rules() {
    let fixture = Fixture::new();
    let root = fixture.home.join(".config/opencode");
    put(
        root.join("opencode.jsonc"),
        json!({"plugins": ["@scope/scoped-pkg"]}).to_string(),
    );
    let package = fixture.home.join(
        ".cache/opencode/npm/@scope/scoped-pkg@latest/1795425314978/node_modules/@scope/scoped-pkg",
    );
    put(
        package.join("package.json"),
        "{\"name\":\"@scope/scoped-pkg\",\"version\":\"0.0.6\"}",
    );
    let inventory = plugins::scan_plugins(&fixture.state).unwrap();
    let plugin = inventory
        .plugins
        .iter()
        .find(|plugin| plugin.source == "@scope/scoped-pkg")
        .unwrap();
    assert_eq!(plugin.package.version.as_deref(), Some("0.0.6"));
    // 缓存里能定位包文件，就不再是“无法确定包声明还是 ID 规则”的只读条目。
    assert!(plugin.actions.contains(&PluginAction::Disable));
    assert!(plugin
        .read_only_reason
        .as_deref()
        .is_none_or(|reason| !reason.contains("无法确定")));
}

#[test]
#[serial]
fn auto_discovered_opencode_plugin_and_unresolved_id_rules_are_read_only() {
    let fixture = Fixture::new();
    let root = fixture.home.join(".config/opencode");
    let package = root.join("plugins/local");
    put(package.join("package.json"), "{\"name\":\"local\"}");
    put(
        root.join("opencode.jsonc"),
        json!({"plugins":[package.to_string_lossy(),"dynamic.plugin.id"]}).to_string(),
    );
    let inventory = plugins::scan_plugins(&fixture.state).unwrap();
    assert!(inventory
        .plugins
        .iter()
        .filter(|plugin| plugin.agent_id == "opencode")
        .all(|plugin| plugin.actions.is_empty()));
}

#[test]
#[serial]
fn pi_local_source_aliases_are_not_treated_as_npm_packages_or_self_updates() {
    let fixture = Fixture::new();
    let root = fixture.home.join(".pi/agent");
    put(
        root.join("local/extensions/local.ts"),
        "export default () => {};",
    );
    put(
        root.join("pi/extensions/other.ts"),
        "export default () => {};",
    );
    put(
        root.join("settings.json"),
        json!({"packages":["local","./local","pi"]}).to_string(),
    );
    let inventory = plugins::scan_plugins(&fixture.state).unwrap();
    let local: Vec<_> = inventory
        .plugins
        .iter()
        .filter(|plugin| matches!(plugin.source.as_str(), "local" | "./local"))
        .collect();
    assert_eq!(local.len(), 2);
    let canonical = root.join("local").canonicalize().unwrap();
    assert!(local.iter().all(|plugin| plugin.package.root == canonical));
    assert!(local.iter().all(|plugin| plugin.actions.is_empty()));
    let pi = inventory
        .plugins
        .iter()
        .find(|plugin| plugin.source == "pi")
        .unwrap();
    assert!(!pi.actions.contains(&PluginAction::Update));
}

#[test]
#[serial]
fn codex_grok_and_dsh_are_only_observed_without_runtime_or_profile_mounting() {
    let fixture = Fixture::new();
    put(
        fixture.home.join(".codex/config.toml"),
        "[plugins.\"review@local\"]\nenabled = true\n",
    );
    put(
        fixture
            .home
            .join(".codex/plugins/cache/local/review/1/plugin.json"),
        json!({"$schema":skill_studio_core::services::plugins::PLUGIN_SCHEMA,"name":"review"})
            .to_string(),
    );
    put(
        fixture.home.join(".grok/config.toml"),
        "[plugins]\nenabled = [\"grok-tools\"]\n",
    );
    put(
        fixture
            .home
            .join(".grok/plugins/grok-tools/.claude-plugin/plugin.json"),
        "{\"name\":\"grok-tools\"}",
    );
    put(
        fixture.home.join(".dsh/profiles/web/package.json"),
        json!({"dsh":{"profile":{"bundles":["@deepseek-ai/example"]}}}).to_string(),
    );
    let inventory = plugins::scan_plugins(&fixture.state).unwrap();
    for agent in ["codex", "grok", "deepseek-harness"] {
        let plugin = inventory
            .plugins
            .iter()
            .find(|plugin| plugin.agent_id == agent)
            .unwrap();
        assert!(plugin.actions.is_empty());
        assert!(plugin.read_only_reason.is_some());
    }
    let dsh = inventory
        .plugins
        .iter()
        .find(|plugin| plugin.agent_id == "deepseek-harness")
        .unwrap();
    assert_eq!(dsh.scope, "profile");
    assert_eq!(dsh.profile.as_deref(), Some("web"));
    assert!(!fixture.home.join("native-args").exists());
}

#[test]
#[serial]
fn dsh_bundle_selected_by_multiple_profiles_is_one_read_only_row() {
    let fixture = Fixture::new();
    for profile in ["desktop", "web"] {
        put(
            fixture
                .home
                .join(format!(".dsh/profiles/{profile}/package.json")),
            json!({"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base"]}}}).to_string(),
        );
    }
    let inventory = plugins::scan_plugins(&fixture.state).unwrap();
    let rows: Vec<_> = inventory
        .plugins
        .iter()
        .filter(|plugin| plugin.agent_id == "deepseek-harness")
        .collect();
    assert_eq!(rows.len(), 1);
    let row = rows[0];
    assert_eq!(row.package.name, "@deepseek-ai/dsh-base");
    assert_eq!(row.profile, None);
    assert!(row
        .package
        .warnings
        .iter()
        .any(|warning| warning.contains("desktop、web")));
    assert!(row.actions.is_empty());
}

#[cfg(unix)]
#[test]
#[serial]
fn claude_marketplace_entry_supplies_name_and_components_and_commit_is_not_a_version() {
    let fixture = Fixture::new();
    let marketplace = fixture.home.join(".claude/plugins/marketplaces/example");
    let cache = fixture
        .home
        .join(".claude/plugins/cache/example/document-skills/575462609294");
    for name in ["one", "two"] {
        put(
            cache.join(format!("skills/{name}/SKILL.md")),
            format!("---\nname: {name}\ndescription: {name}\n---"),
        );
    }
    put(
        marketplace.join(".claude-plugin/marketplace.json"),
        json!({"name":"example","plugins":[{"name":"document-skills","skills":["./skills/one"],"lspServers":{"sourcekit-lsp":{"command":"sourcekit-lsp"}}}]})
            .to_string(),
    );
    put(
        fixture.home.join(".claude/plugins/known_marketplaces.json"),
        json!({"example":{"installLocation":marketplace}}).to_string(),
    );
    put(
        fixture.home.join(".claude/plugins/installed_plugins.json"),
        json!({"plugins":{"document-skills@example":[{"scope":"user","installPath":cache,"version":"575462609294","gitCommitSha":"57546260929473d4e0d1c1bb75297be2fdfa1949"}]}})
            .to_string(),
    );
    put(fixture.home.join(".claude/settings.json"), "{}");
    let plugin = fixture.view("claude-code");
    assert_eq!(plugin.package.name, "document-skills");
    assert!(plugin.package.version.is_none());
    assert_eq!(plugin.package.components.len(), 2);
    assert!(plugin.package.components.iter().any(|c| c.name == "one"));
    assert!(plugin.package.components.iter().any(|c| c.kind == "lsp"));
    assert!(!plugin.package.components.iter().any(|c| c.name == "two"));
}

#[test]
#[serial]
fn disabling_management_does_not_change_any_native_plugin_configuration() {
    let fixture = Fixture::new();
    let path = fixture.pi(json!(["npm:pi-tools"]));
    let before = fs::read(&path).unwrap();
    fixture.manage();
    fixture
        .state
        .mutate(|_, config| {
            config.settings.manage_plugins = false;
            Ok(())
        })
        .unwrap();
    assert_eq!(fs::read(path).unwrap(), before);
    assert!(!fixture.home.join("native-args").exists());
}
