//! 三态启停：已注册 / 已注册但停用 / 未注册。
//!
//! 停用**不删文件**，而是写各 agent 自己的原生配置开关：
//! - Claude Code → `settings.json` 的 `skillOverrides: { "<name>": "off" }`
//! - Codex → `config.toml` 的 `[[skills.config]] path / enabled`（读取兼容名称规则）
//! - Pi → `settings.json` 的 `skills: ["-<skill 目录>"]`（精确排除自动发现的条目）
//!
//! 这样瞬时、无损、可逆，而且用户在 agent 里 `/skills` 看到的状态与 Studio 一致。
//!
//! 两条硬要求：
//! 1. **必须保住用户其他配置** —— JSON 深合并、TOML 用 `toml_edit` 保留注释与格式
//! 2. **重复写不产生重复条目** —— 幂等

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

use crate::error::{Error, Result};
use crate::fs::atomic;
use crate::models::agent::{AgentDescriptor, ToggleMechanism};

/// Claude Code `skillOverrides` 的取值
const CLAUDE_OFF: &str = "off";
const CLAUDE_OVERRIDES_KEY: &str = "skillOverrides";

/// 读 JSON，文件不存在或为空时返回空对象。
fn read_json_object(path: &Path) -> Result<Map<String, Value>> {
    match std::fs::read_to_string(path) {
        Ok(raw) => {
            let text = raw.trim_start_matches('\u{feff}').trim();
            if text.is_empty() {
                return Ok(Map::new());
            }
            match serde_json::from_str::<Value>(text).map_err(|e| Error::json(path, e))? {
                Value::Object(map) => Ok(map),
                other => Err(Error::config(format!(
                    "{} 的顶层不是 JSON 对象，实际是 {}",
                    path.display(),
                    kind_of(&other)
                ))),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(Error::io(path, e)),
    }
}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "布尔",
        Value::Number(_) => "数字",
        Value::String(_) => "字符串",
        Value::Array(_) => "数组",
        Value::Object(_) => "对象",
    }
}

/// 写回 JSON，**保留原有键序**。
///
/// 不能用 `atomic::write_json_file` —— 它会递归排序 key，那是我们自己配置文件的
/// 需求（确定性输出便于 diff），但用户的 settings.json 不该被我们重排。
/// serde_json 开了 `preserve_order` feature，Value 内部是 IndexMap，顺序天然保留。
fn write_json_preserving_order(path: &Path, map: &Map<String, Value>) -> Result<()> {
    let mut text = serde_json::to_string_pretty(&Value::Object(map.clone()))
        .map_err(|e| Error::JsonSerialize { source: e })?;
    text.push('\n');
    atomic::atomic_write(path, text.as_bytes())
}

/// 设置 Claude Code 里某个 skill 的启用状态。
///
/// 停用写 `"off"`；启用则**移除该键**（不存在即默认启用），保持用户配置最小。
pub fn set_claude_skill_enabled(
    settings_path: &Path,
    skill_name: &str,
    enabled: bool,
) -> Result<()> {
    let mut root = read_json_object(settings_path)?;

    let overrides_entry = root
        .entry(CLAUDE_OVERRIDES_KEY.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(overrides) = overrides_entry.as_object_mut() else {
        return Err(Error::config(format!(
            "{} 里的 {CLAUDE_OVERRIDES_KEY} 不是对象",
            settings_path.display()
        )));
    };

    if enabled {
        overrides.remove(skill_name);
    } else {
        overrides.insert(skill_name.to_string(), Value::String(CLAUDE_OFF.into()));
    }

    // 空的 skillOverrides 就别留了，免得在用户配置里堆垃圾
    if root
        .get(CLAUDE_OVERRIDES_KEY)
        .and_then(|v| v.as_object())
        .is_some_and(|m| m.is_empty())
    {
        root.remove(CLAUDE_OVERRIDES_KEY);
    }

    write_json_preserving_order(settings_path, &root)
}

/// 读 Claude Code 里某个 skill 是否被停用
pub fn is_claude_skill_disabled(settings_path: &Path, skill_name: &str) -> bool {
    read_json_object(settings_path)
        .ok()
        .and_then(|root| {
            root.get(CLAUDE_OVERRIDES_KEY)?
                .as_object()?
                .get(skill_name)?
                .as_str()
                .map(|v| v == CLAUDE_OFF)
        })
        .unwrap_or(false)
}

fn codex_config_array(doc: &mut DocumentMut) -> Result<&mut ArrayOfTables> {
    if !doc.contains_key("skills") {
        let mut table = Table::new();
        // implicit：只有 [[skills.config]] 时不额外输出一个空的 [skills] 头
        table.set_implicit(true);
        doc.insert("skills", Item::Table(table));
    }
    let skills = doc
        .get_mut("skills")
        .and_then(|i| i.as_table_mut())
        .ok_or_else(|| Error::config("config.toml 里的 [skills] 不是表"))?;

    if !skills.contains_key("config") {
        skills.insert("config", Item::ArrayOfTables(ArrayOfTables::new()));
    }
    skills
        .get_mut("config")
        .and_then(|i| i.as_array_of_tables_mut())
        .ok_or_else(|| Error::config("config.toml 里的 skills.config 不是表数组"))
}

/// Read ordered user-level Codex rules. Session flags belong to the running
/// Codex process and are not available to this desktop application.
pub fn is_codex_skill_disabled_at(config_path: &Path, skill_name: &str, document: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(config_path) else {
        return false;
    };
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return false;
    };
    let Some(config) = doc.get("skills").and_then(|s| s.get("config")) else {
        return false;
    };
    let mut disabled = false;
    let mut apply = |name: Option<&str>, path: Option<&str>, enabled: Option<bool>| {
        let matches = match (name, path) {
            (Some(name), None) => !name.trim().is_empty() && name.trim() == skill_name,
            (None, Some(path)) => {
                Path::new(path).is_absolute()
                    && crate::fs::paths::paths_alias(Path::new(path), document)
            }
            _ => false,
        };
        if matches {
            if let Some(enabled) = enabled {
                disabled = !enabled;
            }
        }
    };
    if let Some(arr) = config.as_array_of_tables() {
        for t in arr {
            apply(
                t.get("name").and_then(Item::as_str),
                t.get("path").and_then(Item::as_str),
                t.get("enabled").and_then(Item::as_bool),
            );
        }
    } else if let Some(arr) = config.as_array() {
        for value in arr {
            if let Some(t) = value.as_inline_table() {
                apply(
                    t.get("name").and_then(toml_edit::Value::as_str),
                    t.get("path").and_then(toml_edit::Value::as_str),
                    t.get("enabled").and_then(toml_edit::Value::as_bool),
                );
            }
        }
    }
    disabled
}

/// Write an exact document selector after existing rules, preserving name rules
/// for other same-name skills. Explicit true overrides an earlier name disable.
pub fn set_codex_skill_enabled_at(
    config_path: &Path,
    document: &Path,
    enabled: bool,
) -> Result<()> {
    let document = document
        .canonicalize()
        .map_err(|e| Error::io(document, e))?;
    let text = match std::fs::read_to_string(config_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Error::io(config_path, e)),
    };
    let mut doc: DocumentMut = text.parse().map_err(|source| Error::Toml {
        path: config_path.display().to_string(),
        source,
    })?;
    // TOML also permits skills.config = [{ path = "...", enabled = false }].
    if let Some(item) = doc.get_mut("skills").and_then(|s| s.get_mut("config")) {
        if let Some(array) = item.as_array() {
            let mut tables = ArrayOfTables::new();
            for value in array {
                let table = value
                    .as_inline_table()
                    .ok_or_else(|| Error::config("skills.config 必须包含表"))?;
                tables.push(table.clone().into_table());
            }
            *item = Item::ArrayOfTables(tables);
        }
    }
    let array = codex_config_array(&mut doc)?;
    let matches = |table: &Table| {
        table.get("name").is_none()
            && table
                .get("path")
                .and_then(Item::as_str)
                .is_some_and(|p| crate::fs::paths::paths_alias(Path::new(p), &document))
    };
    let next_position = array.iter().filter_map(Table::position).max().unwrap_or(0) + 1;
    let mut entry = array
        .iter()
        .filter(|t| matches(t))
        .last()
        .cloned()
        .unwrap_or_default();
    array.retain(|t| !matches(t));
    // toml_edit retains document positions even after an array entry is moved.
    // Move its serialized position too, otherwise a later name rule wins again.
    entry.set_position(next_position);
    entry["path"] = toml_edit::value(document.to_string_lossy().as_ref());
    entry["enabled"] = toml_edit::value(enabled);
    array.push(entry);
    atomic::write_text_file(config_path, &doc.to_string())
}

const PI_SKILLS_KEY: &str = "skills";

/// 这条 pi 资源覆盖是否精确排除了 `dir`（pi 也接受写成 `<dir>/SKILL.md`、以及 `~/` 开头）。
fn pi_excludes(entry: &Value, dir: &Path) -> bool {
    let Some(pattern) = entry.as_str().and_then(|s| s.trim().strip_prefix('-')) else {
        return false;
    };
    let pattern = match pattern.strip_prefix("~/") {
        Some(rest) => crate::fs::paths::home_dir().join(rest),
        None => PathBuf::from(pattern),
    };
    let pattern = match pattern.file_name() {
        Some(name) if name == "SKILL.md" => pattern.parent().map(Path::to_path_buf),
        _ => Some(pattern),
    };
    pattern.is_some_and(|p| p.components().eq(dir.components()))
}

/// Pi 里这个 skill 是否被停用。Pi 按**未解析软链**的路径匹配，这里同样不跟随链接。
pub fn is_pi_skill_disabled_at(settings_path: &Path, document: &Path) -> bool {
    let Some(dir) = document.parent() else {
        return false;
    };
    read_json_object(settings_path)
        .ok()
        .and_then(|root| {
            root.get(PI_SKILLS_KEY)?
                .as_array()
                .map(|list| list.iter().any(|entry| pi_excludes(entry, dir)))
        })
        .unwrap_or(false)
}

/// 停用写一条 `-<skill 目录绝对路径>`；启用则移除指向它的精确排除。
///
/// 写绝对路径：相对路径在 pi 里按各自的 baseDir 解析，`~/.pi/agent/skills` 和
/// `~/.agents/skills` 的 baseDir 不同，同一个 `skills/<name>` 会同时命中两边。
pub fn set_pi_skill_enabled_at(settings_path: &Path, document: &Path, enabled: bool) -> Result<()> {
    let dir = document
        .parent()
        .ok_or_else(|| Error::invalid("skill 路径无效"))?;
    let mut root = read_json_object(settings_path)?;
    let entry = root
        .entry(PI_SKILLS_KEY.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(list) = entry.as_array_mut() else {
        return Err(Error::config(format!(
            "{} 里的 {PI_SKILLS_KEY} 不是数组",
            settings_path.display()
        )));
    };
    list.retain(|entry| !pi_excludes(entry, dir));
    if !enabled {
        list.push(Value::String(format!("-{}", dir.display())));
    }
    if root
        .get(PI_SKILLS_KEY)
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        root.remove(PI_SKILLS_KEY);
    }
    write_json_preserving_order(settings_path, &root)
}

pub fn is_skill_disabled_at(
    agent: &AgentDescriptor,
    overrides: &HashMap<String, PathBuf>,
    name: &str,
    document: &Path,
) -> bool {
    let Some(path) = agent.toggle_config_path(overrides) else {
        return false;
    };
    match agent.toggle {
        ToggleMechanism::ClaudeSettingsJson => is_claude_skill_disabled(&path, name),
        ToggleMechanism::CodexConfigToml => is_codex_skill_disabled_at(&path, name, document),
        ToggleMechanism::PiSettingsJson => is_pi_skill_disabled_at(&path, document),
        ToggleMechanism::None => false,
    }
}

pub fn set_skill_enabled_at(
    agent: &AgentDescriptor,
    overrides: &HashMap<String, PathBuf>,
    name: &str,
    document: &Path,
    enabled: bool,
) -> Result<()> {
    let path = agent
        .toggle_config_path(overrides)
        .ok_or_else(|| Error::invalid("该 agent 不支持原生启停"))?;
    match agent.toggle {
        ToggleMechanism::ClaudeSettingsJson => set_claude_skill_enabled(&path, name, enabled),
        ToggleMechanism::CodexConfigToml => set_codex_skill_enabled_at(&path, document, enabled),
        ToggleMechanism::PiSettingsJson => set_pi_skill_enabled_at(&path, document, enabled),
        ToggleMechanism::None => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pi 的停用是 `skills` 数组里的一条 `-<skill 目录>`：用户自己的资源路径、其他设置和键序
    /// 都要保住；重复停用不堆重复条目，启用时连用户写成 `/SKILL.md` 的等价条目一起去掉。
    #[test]
    fn pi_exclusion_round_trip_keeps_user_resources_and_order() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        let skill = d.path().join("agents/skills/demo");
        // 用 serde_json 生成：Windows 路径里的反斜杠必须转义
        let settings = serde_json::json!({
            "defaultModel": "opus",
            "skills": ["../team-skills", format!("-{}", skill.join("SKILL.md").display())],
            "theme": "dark",
        });
        std::fs::write(&p, serde_json::to_string_pretty(&settings).unwrap()).unwrap();
        let document = skill.join("SKILL.md");
        assert!(
            is_pi_skill_disabled_at(&p, &document),
            "用户写的 SKILL.md 形式也算"
        );

        set_pi_skill_enabled_at(&p, &document, false).unwrap();
        set_pi_skill_enabled_at(&p, &document, false).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            v["skills"],
            serde_json::json!(["../team-skills", format!("-{}", skill.display())])
        );
        assert!(
            text.find("defaultModel").unwrap() < text.find("theme").unwrap(),
            "键序不变：{text}"
        );

        set_pi_skill_enabled_at(&p, &document, true).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["skills"], serde_json::json!(["../team-skills"]));
        assert_eq!(v["defaultModel"], "opus");
        assert!(!is_pi_skill_disabled_at(&p, &document));
    }

    #[test]
    #[serial_test::serial]
    fn pi_exclusion_understands_home_shorthand_and_removes_empty_list() {
        let d = tempfile::tempdir().unwrap();
        std::env::set_var(crate::fs::paths::TEST_HOME_ENV, d.path());
        let p = d.path().join("settings.json");
        std::fs::write(&p, r#"{"skills": ["-~/.agents/skills/demo"]}"#).unwrap();
        let document = d.path().join(".agents/skills/demo/SKILL.md");
        assert!(is_pi_skill_disabled_at(&p, &document));
        set_pi_skill_enabled_at(&p, &document, true).unwrap();
        std::env::remove_var(crate::fs::paths::TEST_HOME_ENV);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(v.get("skills").is_none(), "空列表不留：{v}");
    }

    #[test]
    fn claude_disable_then_enable_leaves_other_settings_intact() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(
            &p,
            r#"{
  "model": "opus",
  "permissions": { "allow": ["Bash(git *)"] },
  "env": { "FOO": "bar" }
}"#,
        )
        .unwrap();

        set_claude_skill_enabled(&p, "deploy", false).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["skillOverrides"]["deploy"], "off");
        // 用户其他配置必须完好
        assert_eq!(v["model"], "opus");
        assert_eq!(v["permissions"]["allow"][0], "Bash(git *)");
        assert_eq!(v["env"]["FOO"], "bar");
        assert!(is_claude_skill_disabled(&p, "deploy"));

        set_claude_skill_enabled(&p, "deploy", true).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        // 启用后移除键，且空的 skillOverrides 不留
        assert!(v.get("skillOverrides").is_none(), "{v}");
        assert_eq!(v["model"], "opus");
        assert!(!is_claude_skill_disabled(&p, "deploy"));
    }

    #[test]
    fn claude_preserves_key_order() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, r#"{"zeta":1,"alpha":2,"middle":3}"#).unwrap();
        set_claude_skill_enabled(&p, "x", false).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        // 不能被重排成字母序 —— 那是我们自己配置的需求，不是用户文件的
        assert!(
            text.find("zeta").unwrap() < text.find("alpha").unwrap(),
            "键序被改动了:\n{text}"
        );
    }

    #[test]
    fn claude_creates_file_when_missing() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("nested/settings.json");
        set_claude_skill_enabled(&p, "deploy", false).unwrap();
        assert!(is_claude_skill_disabled(&p, "deploy"));
    }

    #[test]
    fn claude_keeps_other_skill_overrides() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, r#"{"skillOverrides":{"other":"name-only"}}"#).unwrap();
        set_claude_skill_enabled(&p, "deploy", false).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["skillOverrides"]["other"], "name-only");
        assert_eq!(v["skillOverrides"]["deploy"], "off");
    }

    #[test]
    fn claude_rejects_non_object_root() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, "[1,2,3]").unwrap();
        let err = set_claude_skill_enabled(&p, "x", false).unwrap_err();
        assert!(err.to_string().contains("不是 JSON 对象"), "{err}");
    }

    fn document(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name).join("SKILL.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "test skill").unwrap();
        path
    }

    #[test]
    fn codex_disable_preserves_comments_and_other_tables() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        let skill = document(d.path(), "deploy");
        std::fs::write(&p, "# user config\nmodel = \"gpt-5\"\n[tui]\ntheme = \"dark\" # inline comment\n[skills]\ninclude_instructions = true\n").unwrap();
        set_codex_skill_enabled_at(&p, &skill, false).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        for kept in [
            "# user config",
            "# inline comment",
            "model = \"gpt-5\"",
            "[tui]",
            "include_instructions = true",
        ] {
            assert!(text.contains(kept), "{text}");
        }
        assert!(is_codex_skill_disabled_at(&p, "deploy", &skill));
    }

    #[test]
    fn codex_repeated_disable_does_not_duplicate_entries() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        let skill = document(d.path(), "deploy");
        for _ in 0..3 {
            set_codex_skill_enabled_at(&p, &skill, false).unwrap();
        }
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text.matches("[[skills.config]]").count(), 1, "{text}");
        assert!(is_codex_skill_disabled_at(&p, "deploy", &skill));
    }

    #[test]
    fn codex_enable_overrides_name_rule_only_for_selected_document() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        let first = document(d.path(), "first");
        let second = document(d.path(), "second");
        std::fs::write(
            &p,
            "[[skills.config]]\nname = \"deploy\"\nenabled = false\n",
        )
        .unwrap();
        set_codex_skill_enabled_at(&p, &first, true).unwrap();
        assert!(!is_codex_skill_disabled_at(&p, "deploy", &first));
        assert!(is_codex_skill_disabled_at(&p, "deploy", &second));
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("name = \"deploy\""));
    }

    #[test]
    fn codex_handles_multiple_skills_independently() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        let a = document(d.path(), "a");
        let b = document(d.path(), "b");
        set_codex_skill_enabled_at(&p, &a, false).unwrap();
        set_codex_skill_enabled_at(&p, &b, false).unwrap();
        assert!(is_codex_skill_disabled_at(&p, "a", &a));
        assert!(is_codex_skill_disabled_at(&p, "b", &b));
        set_codex_skill_enabled_at(&p, &a, true).unwrap();
        assert!(!is_codex_skill_disabled_at(&p, "a", &a));
        assert!(is_codex_skill_disabled_at(&p, "b", &b));
    }

    #[test]
    fn codex_reports_not_disabled_for_unknown_skill() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        let a = document(d.path(), "a");
        let unknown = document(d.path(), "unknown");
        set_codex_skill_enabled_at(&p, &a, false).unwrap();
        assert!(!is_codex_skill_disabled_at(&p, "unknown", &unknown));
        assert!(!is_codex_skill_disabled_at(
            &d.path().join("missing.toml"),
            "a",
            &a
        ));
    }

    #[test]
    fn dispatch_routes_to_the_right_mechanism() {
        use crate::models::agent::find_agent;
        let d = tempfile::tempdir().unwrap();
        let skill = document(d.path(), "deploy");
        let mut overrides = HashMap::new();
        overrides.insert("claude-code".to_string(), d.path().join("claude"));
        overrides.insert("codex".to_string(), d.path().join("codex"));
        for (id, config) in [
            ("claude-code", "claude/settings.json"),
            ("codex", "codex/config.toml"),
        ] {
            let agent = find_agent(id).unwrap();
            set_skill_enabled_at(agent, &overrides, "deploy", &skill, false).unwrap();
            assert!(is_skill_disabled_at(agent, &overrides, "deploy", &skill));
            assert!(d.path().join(config).is_file());
            set_skill_enabled_at(agent, &overrides, "deploy", &skill, true).unwrap();
            assert!(!is_skill_disabled_at(agent, &overrides, "deploy", &skill));
        }
    }
}
