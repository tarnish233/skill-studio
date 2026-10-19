//! Claude 的「项目本地」写在 ~/.claude.json 里这个项目的配置中。要改家目录，单独一个测试二进制，
//! 不和其他测试并行。
use serde_json::{json, Value};
use skill_studio_mcp::management::{self, projects, Entry};
use std::{collections::HashMap, path::Path};

fn local(claude: &Path, root: &Path, key: &str) -> Option<Value> {
    let text = std::fs::read_to_string(claude).unwrap_or_default();
    skill_studio_mcp::native::entry(&text, "claude", Some(&root.to_string_lossy()), key).unwrap()
}

#[test]
fn claude_local_writes_into_this_projects_section_and_moves_when_switched() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::env::set_var("SKILL_STUDIO_TEST_HOME", &home);
    let dir = tmp.path().join("studio");
    let root = tmp.path().join("work/webapp");
    std::fs::create_dir_all(&root).unwrap();
    management::save(
        &dir,
        Entry {
            id: "alpha".into(),
            name: "alpha".into(),
            mode: "direct".into(),
            definition: json!({"type":"stdio","command":"alpha"}),
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
    let overrides = HashMap::new();
    let save = |claude_local: bool| {
        projects::save(
            &dir,
            "p1",
            projects::Input {
                root: root.clone(),
                entry_ids: vec!["alpha".into()],
                group_ids: vec![],
                claude_local,
            },
            vec![],
            &overrides,
            Path::new("/app"),
        )
        .unwrap()
    };
    let claude = home.join(".claude.json");
    save(true);
    projects::activate(&dir, "p1", "claude", true, &overrides, Path::new("/app")).unwrap();
    assert!(local(&claude, &root, "alpha").is_some());
    assert!(!root.join(".mcp.json").exists());
    // 用户全局的 mcpServers 不受影响
    let text: Value = serde_json::from_str(&std::fs::read_to_string(&claude).unwrap()).unwrap();
    assert!(text["mcpServers"]["alpha"].is_null());
    // 启用期间改成项目配置：从项目本地撤回，写进项目里的 .mcp.json
    save(false);
    assert_eq!(local(&claude, &root, "alpha"), None);
    let shared = std::fs::read_to_string(root.join(".mcp.json")).unwrap();
    assert!(
        skill_studio_mcp::native::entry(&shared, "claude", None, "alpha")
            .unwrap()
            .is_some()
    );
}
