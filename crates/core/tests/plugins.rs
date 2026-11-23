use serde_json::json;
use skill_studio_core::{
    models::agent::require_agent,
    services::{
        plugins::{self, MCP_SCHEMA, PLUGIN_SCHEMA},
        scanner,
    },
};
use std::{fs, path::Path};

fn put(path: impl AsRef<Path>, text: impl AsRef<[u8]>) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn manifest(root: &Path, value: serde_json::Value) {
    put(root.join("plugin.json"), value.to_string());
}
fn standard(root: &Path) {
    manifest(root, json!({"$schema":PLUGIN_SCHEMA,"name":"review-kit"}));
}
fn skill(root: &Path) {
    put(
        root.join("skills/review/SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nReview.",
    );
}

#[test]
fn standard_package_discovers_fixed_components_without_requiring_a_version() {
    let temp = tempfile::tempdir().unwrap();
    standard(temp.path());
    skill(temp.path());
    put(temp.path().join("mcp.json"), json!({"$schema":MCP_SCHEMA,"mcpServers":{"docs":{"type":"streamable-http","url":"https://example.com/mcp"}}}).to_string());
    let package = plugins::inspect(temp.path(), None);
    assert!(package.error.is_none(), "{:?}", package.error);
    assert_eq!(package.format, "agent-plugins-v1");
    assert!(package.version.is_none());
    assert_eq!(package.components.len(), 2);
    assert!(package.components.iter().all(|c| c.issue.is_none()));
}

#[test]
fn standard_ignores_unknown_fields_and_unimplemented_extensions() {
    let temp = tempfile::tempdir().unwrap();
    manifest(
        temp.path(),
        json!({"$schema":PLUGIN_SCHEMA,"name":"acme.tools","skills":"./extra","version":"not-semver","extensions":{"com.other":false}}),
    );
    put(
        temp.path().join("extra/ignored/SKILL.md"),
        "---\nname: ignored\ndescription: ignored\n---",
    );
    let package = plugins::inspect(temp.path(), None);
    assert!(package.error.is_none());
    assert_eq!(package.version.as_deref(), Some("not-semver"));
    assert_eq!(package.extensions, ["com.other"]);
    assert!(package.warnings.iter().any(|w| w.contains("skills")));
    assert!(package.components.is_empty());
}

#[test]
fn non_object_extensions_are_nonfatal() {
    let temp = tempfile::tempdir().unwrap();
    manifest(
        temp.path(),
        json!({"$schema":PLUGIN_SCHEMA,"name":"a","extensions":[] }),
    );
    let package = plugins::inspect(temp.path(), None);
    assert!(package.error.is_none());
    assert!(!package.warnings.is_empty());
}

#[test]
fn invalid_or_unsupported_manifests_do_not_fall_back_to_native_layouts() {
    for value in [
        json!({"$schema":PLUGIN_SCHEMA}),
        json!({"$schema":PLUGIN_SCHEMA,"name":"BAD"}),
        json!({"$schema":PLUGIN_SCHEMA,"name":"has--double"}),
        json!({"$schema":PLUGIN_SCHEMA,"name":"a","version":1}),
        json!({"$schema":PLUGIN_SCHEMA,"name":"a","author":{"extra":1}}),
        json!({"$schema":"https://agent-plugins.org/schemas/9.0.0/plugin.schema.json","name":"a"}),
    ] {
        let temp = tempfile::tempdir().unwrap();
        manifest(temp.path(), value);
        skill(temp.path());
        put(
            temp.path().join(".claude-plugin/plugin.json"),
            "{\"name\":\"native\"}",
        );
        let package = plugins::inspect(temp.path(), Some("claude-code"));
        assert!(package.error.is_some());
        assert!(package.components.is_empty());
    }
}

#[test]
fn invalid_mcp_entry_does_not_disable_valid_servers_or_skills() {
    let temp = tempfile::tempdir().unwrap();
    standard(temp.path());
    skill(temp.path());
    put(temp.path().join("mcp.json"), json!({"$schema":MCP_SCHEMA,"mcpServers":{"good":{"type":"stdio","command":"node","args":["${PLUGIN_ROOT}/server.js"]},"bad":{"type":"stdio","command":"node server.js"},"reserved":{"type":"stdio","command":"node","env":{"PLUGIN_ROOT":"elsewhere"}}}}).to_string());
    let package = plugins::inspect(temp.path(), None);
    assert!(package.error.is_none());
    assert_eq!(package.components.len(), 4);
    assert!(package
        .components
        .iter()
        .find(|c| c.name == "good")
        .unwrap()
        .issue
        .is_none());
    assert!(package
        .components
        .iter()
        .find(|c| c.name == "bad")
        .unwrap()
        .issue
        .is_some());
    assert!(package
        .components
        .iter()
        .find(|c| c.name == "reserved")
        .unwrap()
        .issue
        .is_some());
}

#[test]
fn mcp_schema_mismatch_and_invalid_json_are_component_level_failures() {
    for content in [
        "not-json".to_string(),
        json!({"$schema":PLUGIN_SCHEMA,"mcpServers":{}}).to_string(),
    ] {
        let temp = tempfile::tempdir().unwrap();
        standard(temp.path());
        skill(temp.path());
        put(temp.path().join("mcp.json"), content);
        let package = plugins::inspect(temp.path(), None);
        assert!(package.error.is_none());
        assert_eq!(package.components.len(), 1);
        assert!(!package.warnings.is_empty());
    }
}

#[test]
fn remote_server_validation_enforces_https_and_header_rules() {
    let temp = tempfile::tempdir().unwrap();
    standard(temp.path());
    put(temp.path().join("mcp.json"), json!({"$schema":MCP_SCHEMA,"mcpServers":{
        "loopback":{"type":"streamable-http","url":"http://[::1]:8080/mcp"},
        "http":{"type":"streamable-http","url":"http://example.com/mcp"},
        "credentials":{"type":"sse","url":"https://user:SECRET@example.com/mcp"},
        "duplicate":{"type":"streamable-http","url":"https://example.com","headers":{"X-KEY":"a","x-key":"b"}},
        "newline":{"type":"streamable-http","url":"https://example.com","headers":{"X-KEY":"a\r\nb"}}
    }}).to_string());
    let package = plugins::inspect(temp.path(), None);
    assert!(package
        .components
        .iter()
        .find(|c| c.name == "loopback")
        .unwrap()
        .issue
        .is_none());
    assert_eq!(
        package
            .components
            .iter()
            .filter(|c| c.issue.is_some())
            .count(),
        4
    );
    assert!(!serde_json::to_string(&package).unwrap().contains("SECRET"));
}

#[test]
fn skill_discovery_is_not_recursive_and_does_not_load_skills_root_itself() {
    let temp = tempfile::tempdir().unwrap();
    standard(temp.path());
    skill(temp.path());
    put(
        temp.path().join("skills/SKILL.md"),
        "---\nname: root\ndescription: root\n---",
    );
    put(
        temp.path().join("skills/nested/deeper/SKILL.md"),
        "---\nname: deeper\ndescription: deeper\n---",
    );
    let package = plugins::inspect(temp.path(), None);
    assert_eq!(package.components.len(), 1);
    assert_eq!(package.components[0].name, "review");
}

#[test]
fn standard_skills_require_frontmatter_name_and_description() {
    let temp = tempfile::tempdir().unwrap();
    standard(temp.path());
    put(temp.path().join("skills/bad/SKILL.md"), "No frontmatter");
    let package = plugins::inspect(temp.path(), None);
    assert!(package.error.is_none());
    assert!(package.components[0].issue.is_some());
}

#[cfg(unix)]
#[test]
fn symlinked_manifests_and_components_cannot_escape_package_root() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    standard(outside.path());
    symlink(
        outside.path().join("plugin.json"),
        temp.path().join("plugin.json"),
    )
    .unwrap();
    assert!(plugins::inspect(temp.path(), None).error.is_some());
    fs::remove_file(temp.path().join("plugin.json")).unwrap();
    standard(temp.path());
    put(
        outside.path().join("SKILL.md"),
        "---\nname: outside\ndescription: secret\n---",
    );
    fs::create_dir_all(temp.path().join("skills")).unwrap();
    symlink(outside.path(), temp.path().join("skills/outside")).unwrap();
    let package = plugins::inspect(temp.path(), None);
    assert!(package.components.is_empty());
    assert!(!package.warnings.is_empty());
}

#[test]
fn bundled_commands_and_working_directories_cannot_escape() {
    let temp = tempfile::tempdir().unwrap();
    standard(temp.path());
    put(
        temp.path().join("mcp.json"),
        json!({"$schema":MCP_SCHEMA,"mcpServers":{
            "cmd":{"type":"stdio","command":"./../escape"},
            "cwd":{"type":"stdio","command":"node","cwd":"${PLUGIN_ROOT}/../escape"},
            "data":{"type":"stdio","command":"node","cwd":"${PLUGIN_DATA}/../escape"}
        }})
        .to_string(),
    );
    assert!(plugins::inspect(temp.path(), None)
        .components
        .iter()
        .all(|c| c.issue.is_some()));
}

#[test]
fn marketplace_entry_is_the_manifest_when_the_cache_has_none() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["one", "two"] {
        put(
            temp.path().join(format!("skills/{name}/SKILL.md")),
            format!("---\nname: {name}\ndescription: {name}\n---"),
        );
    }
    let entry = json!({
        "name": "document-skills",
        "version": "1.0.0",
        "skills": ["./skills/one"],
        "lspServers": {"sourcekit-lsp": {"command": "sourcekit-lsp"}}
    });
    let package = plugins::inspect_with_manifest(temp.path(), Some("claude-code"), Some(&entry));
    assert!(package.error.is_none(), "{:?}", package.error);
    assert_eq!(package.name, "document-skills");
    assert_eq!(package.version.as_deref(), Some("1.0.0"));
    assert_eq!(package.components.len(), 2);
    assert!(package.components.iter().any(|c| c.name == "one"));
    assert!(!package.components.iter().any(|c| c.name == "two"));
    assert!(package.components.iter().any(|c| c.kind == "lsp"));
    assert_eq!(
        plugins::inspect(temp.path(), Some("claude-code"))
            .components
            .len(),
        2
    );
}

#[test]
fn native_claude_and_pi_packages_remain_native() {
    let temp = tempfile::tempdir().unwrap();
    skill(temp.path());
    put(
        temp.path().join(".claude-plugin/plugin.json"),
        "{\"name\":\"native\",\"mcpServers\":{\"docs\":{\"command\":\"node\"}}}",
    );
    let package = plugins::inspect(temp.path(), None);
    assert_eq!(package.format, "claude");
    assert_eq!(package.components.len(), 2);
    fs::remove_dir_all(temp.path().join(".claude-plugin")).unwrap();
    put(temp.path().join("package.json"), "{\"name\":\"pi-tools\",\"pi\":{\"skills\":[\"./skills\"],\"extensions\":[\"./src/tool.ts\"]}}");
    put(
        temp.path().join("src/tool.ts"),
        "throw new Error('MUST NOT EXECUTE');",
    );
    let package = plugins::inspect(temp.path(), None);
    assert_eq!(package.format, "pi-package");
    assert_eq!(package.components.len(), 2);
}

#[test]
fn plugin_in_skill_directory_is_not_adopted_or_deployed_as_a_skill() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("skills");
    let plugin = root.join("plugin");
    put(
        plugin.join("SKILL.md"),
        "---\nname: plugin\ndescription: plugin\n---",
    );
    put(
        plugin.join(".claude-plugin/plugin.json"),
        "{\"name\":\"plugin\"}",
    );
    assert!(
        scanner::scan_root(&root, require_agent("claude-code").unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(scanner::validate_sync_source(&plugin)
        .unwrap_err()
        .to_string()
        .contains("完整插件"));
}

#[test]
fn oversized_or_malformed_manifest_errors_do_not_echo_content() {
    let temp = tempfile::tempdir().unwrap();
    put(
        temp.path().join(".claude-plugin/plugin.json"),
        "SECRET bad json",
    );
    let package = plugins::inspect(temp.path(), None);
    assert!(package.error.is_some());
    assert!(!serde_json::to_string(&package).unwrap().contains("SECRET"));
    put(
        temp.path().join(".claude-plugin/plugin.json"),
        "x".repeat(1024 * 1024 + 1),
    );
    assert!(plugins::inspect(temp.path(), None)
        .error
        .unwrap()
        .contains("1 MiB"));
}
