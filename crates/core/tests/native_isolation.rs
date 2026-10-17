//! 让分组真正管得住 Pi 和 Grok：
//! - Pi 有原生开关（settings.json 的 `-<路径>`），「不保留组外 skill」对它生效，
//!   连共享根 `~/.agents/skills` 里别的工具装进来的也能停用；
//! - Grok 默认还读 `~/.claude/skills`，管理期间关掉这项兼容读取，退出时撤回。
#[path = "support.rs"]
mod support;
use serde_json::Value;
use serial_test::serial;
use std::fs;
use support::Env;

fn pi_settings(env: &Env) -> std::path::PathBuf {
    env.path().join(".pi/agent/settings.json")
}

fn pi_skills(env: &Env) -> Value {
    let text = fs::read_to_string(pi_settings(env)).unwrap();
    serde_json::from_str::<Value>(&text).unwrap()["skills"].clone()
}

#[test]
#[serial]
fn pi_policy_excludes_unmanaged_skills_by_path_and_releases_them_on_leave() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    fs::create_dir_all(env.path().join(".pi/agent")).unwrap();
    let original = r#"{
  "defaultModel": "opus",
  "skills": ["../team-skills"]
}
"#;
    fs::write(pi_settings(&env), original).unwrap();
    let manual = env.write_simple_skill(&env.path().join(".pi/agent/skills"), "manual");
    let shared = env.write_simple_skill(&env.agents_skills(), "shared");

    c.settings.preserve_manual_skills = false;
    studio.reconcile_manual_policy(&mut c, true).unwrap();

    // 组外的手动 skill 都停用：Pi 自己目录里的，和共享根里别的工具装进来的。
    // 按路径分量比较（Pi 也是这样匹配的）：Windows 上同一路径可能混用 `/` 和 `\`。
    let skills = pi_skills(&env);
    let entries: Vec<&str> = skills
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(entries[0], "../team-skills", "用户自己的资源路径保留在原位");
    let excluded: Vec<std::path::PathBuf> = entries[1..]
        .iter()
        .map(|e| std::path::PathBuf::from(e.strip_prefix('-').expect("精确排除以 - 开头")))
        .collect();
    assert_eq!(excluded, vec![manual.clone(), shared.clone()]);
    let views = studio.scan_skills(&c).unwrap();
    for view in &views {
        assert!(view.agents["pi"].disabled, "{}", view.skill.name);
    }

    // 分组部署进来的照常启用
    env.write_simple_skill(&env.hub(), "grouped");
    let id = studio
        .scan_skills(&c)
        .unwrap()
        .into_iter()
        .find(|v| v.skill.name == "grouped")
        .unwrap()
        .skill
        .id;
    studio
        .save_group(&mut c, "g".into(), "g", vec![id.clone()])
        .unwrap();
    studio
        .activate_agent_groups(&mut c, "pi", &["g".to_string()])
        .unwrap();
    let grouped = studio
        .scan_skills(&c)
        .unwrap()
        .into_iter()
        .find(|v| v.skill.id == id)
        .unwrap();
    assert!(!grouped.agents["pi"].disabled);
    assert!(env
        .path()
        .join(".pi/agent/skills/grouped/SKILL.md")
        .is_file());

    // 退出 Pi 管理：分组副本撤回，停用的手动 skill 恢复，用户设置原样还原
    studio.set_agent_management(&mut c, "pi", false).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&fs::read_to_string(pi_settings(&env)).unwrap()).unwrap(),
        serde_json::from_str::<Value>(original).unwrap()
    );
    assert!(manual.join("SKILL.md").is_file());
    assert!(shared.join("SKILL.md").is_file());
}

fn grok_config(env: &Env) -> std::path::PathBuf {
    env.path().join(".grok/config.toml")
}

#[test]
#[serial]
fn grok_stops_reading_claude_skills_while_managed_and_restores_on_leave() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    fs::create_dir_all(env.path().join(".grok")).unwrap();
    let original = "# 用户自己的注释\n[ui]\ntheme = \"dark\"\n";
    fs::write(grok_config(&env), original).unwrap();

    studio.reconcile_manual_policy(&mut c, false).unwrap();
    let text = fs::read_to_string(grok_config(&env)).unwrap();
    assert!(text.starts_with(original), "用户内容原样保留：{text}");
    let doc: toml_edit::DocumentMut = text.parse().unwrap();
    assert_eq!(doc["compat"]["claude"]["skills"].as_bool(), Some(false));
    assert!(
        !text.contains("[compat]\n"),
        "不单独输出空的 [compat] 表头：{text}"
    );
    assert_eq!(c.native_isolation["grok"], vec!["compat.claude.skills"]);
    assert_eq!(
        studio.load_config().unwrap().native_isolation["grok"],
        vec!["compat.claude.skills"],
        "记录要落盘"
    );

    // 再扫一次不重复写
    studio.reconcile_manual_policy(&mut c, false).unwrap();
    assert_eq!(fs::read_to_string(grok_config(&env)).unwrap(), text);

    studio.set_agent_management(&mut c, "grok", false).unwrap();
    assert_eq!(fs::read_to_string(grok_config(&env)).unwrap(), original);
    assert!(c.native_isolation.is_empty());

    studio.set_agent_management(&mut c, "grok", true).unwrap();
    let doc: toml_edit::DocumentMut = fs::read_to_string(grok_config(&env))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(doc["compat"]["claude"]["skills"].as_bool(), Some(false));
}

#[test]
#[serial]
fn grok_isolation_respects_the_users_own_choice() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    fs::create_dir_all(env.path().join(".grok")).unwrap();

    // 用户本来就关着：不记录，退出管理时也不动
    let off = "[compat.claude]\nskills = false\nmcps = true\n";
    fs::write(grok_config(&env), off).unwrap();
    studio.reconcile_manual_policy(&mut c, false).unwrap();
    assert!(c.native_isolation.is_empty());
    studio.set_agent_management(&mut c, "grok", false).unwrap();
    assert_eq!(fs::read_to_string(grok_config(&env)).unwrap(), off);
    studio.set_agent_management(&mut c, "grok", true).unwrap();
    assert_eq!(fs::read_to_string(grok_config(&env)).unwrap(), off);

    // Studio 关掉之后用户又自己打开：不再覆盖，退出时也保持用户的值
    fs::write(grok_config(&env), "").unwrap();
    c.native_isolation.clear();
    studio.reconcile_manual_policy(&mut c, true).unwrap();
    assert!(c.native_isolation.contains_key("grok"));
    let user_on = "[compat.claude]\nskills = true\n";
    fs::write(grok_config(&env), user_on).unwrap();
    studio.reconcile_manual_policy(&mut c, false).unwrap();
    assert_eq!(fs::read_to_string(grok_config(&env)).unwrap(), user_on);
    studio.set_agent_management(&mut c, "grok", false).unwrap();
    assert_eq!(fs::read_to_string(grok_config(&env)).unwrap(), user_on);
    assert!(c.native_isolation.is_empty());
}

#[test]
#[serial]
fn grok_isolation_never_creates_config_for_an_uninstalled_grok() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    studio.reconcile_manual_policy(&mut c, true).unwrap();
    assert!(!env.path().join(".grok").exists());
    assert!(c.native_isolation.is_empty());
}
