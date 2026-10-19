//! Only edit the Studio-owned entry. The stdio adapter reads credentials locally,
//! so neither project configuration nor the Agent receives an upstream token.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use skill_studio_core::fs::atomic;
use std::path::Path;
use toml_edit::{value, Array, DocumentMut, Item, Table};

pub fn apply(
    path: &Path,
    agent: &str,
    id: &str,
    command: &str,
    dir: &str,
    remove: bool,
) -> Result<()> {
    let old = read(path)?;
    let next = render(&old, agent, id, command, dir, remove)?;
    if next == old {
        return Ok(());
    }
    if path.exists() {
        // 每个配置文件只留最近一份，放在 Studio 目录；可能含凭据，只给自己读
        let backup = crate::management::backups::gateway_backup(Path::new(dir), agent, path)?;
        atomic::atomic_write_private(&backup, old.as_bytes())?;
    }
    if read(path)? != old {
        bail!("配置已被其他程序修改，请刷新后重试");
    }
    atomic::atomic_write(path, next.as_bytes())?;
    Ok(())
}
fn read(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}
fn render(
    old: &str,
    agent: &str,
    id: &str,
    command: &str,
    dir: &str,
    remove: bool,
) -> Result<String> {
    let key = format!("studio-{id}");
    let args = ["--mcp-client", id, dir];
    if agent == "claude" {
        let mut doc: Value = if old.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(old)?
        };
        let root = doc.as_object_mut().context("Claude 配置必须为 JSON 对象")?;
        if !root.contains_key("mcpServers") {
            if remove {
                return Ok(old.into());
            }
            root.insert("mcpServers".into(), json!({}));
        }
        let entries = root
            .get_mut("mcpServers")
            .unwrap()
            .as_object_mut()
            .context("mcpServers 必须为对象")?;
        if let Some(entry) = entries.get(&key) {
            if entry.get("command").and_then(Value::as_str) != Some(command)
                || entry.get("args") != Some(&json!(args))
            {
                bail!("同名 MCP 已存在或被修改，已保留：{key}");
            }
        }
        if remove {
            entries.remove(&key);
        } else {
            entries.insert(key, json!({"type":"stdio","command":command,"args":args}));
        }
        Ok(format!("{}\n", serde_json::to_string_pretty(&doc)?))
    } else if agent == "codex" {
        let mut doc: DocumentMut = old.parse()?;
        if doc.get("mcp_servers").is_none() {
            if remove {
                return Ok(old.into());
            }
            doc["mcp_servers"] = Item::Table(Table::new());
        }
        let entries = doc["mcp_servers"]
            .as_table_mut()
            .context("mcp_servers 必须为 TOML 表")?;
        if let Some(entry) = entries.get(&key) {
            let existing_args = entry.get("args").and_then(Item::as_array).map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or(""))
                    .collect::<Vec<_>>()
            });
            if entry.get("command").and_then(Item::as_str) != Some(command)
                || existing_args.as_deref() != Some(args.as_slice())
            {
                bail!("同名 MCP 已存在或被修改，已保留：{key}");
            }
        }
        if remove {
            entries.remove(&key);
        } else {
            let mut entry = Table::new();
            entry["command"] = value(command);
            let mut array = Array::new();
            for arg in args {
                array.push(arg);
            }
            entry["args"] = value(array);
            entries.insert(&key, Item::Table(entry));
        }
        Ok(doc.to_string())
    } else if matches!(agent, "opencode" | "pi" | "grok") {
        let existing = crate::native::entry(old, agent, None, &key)?;
        let desired = crate::native::for_agent(
            &json!({"type":"stdio","command":command,"args":args}),
            agent,
        )?;
        if existing.as_ref().is_some_and(|v| v != &desired) {
            bail!("同名 MCP 已存在或被修改，已保留：{key}");
        }
        crate::native::patch(
            old,
            agent,
            None,
            &key,
            &existing,
            &if remove { None } else { Some(desired) },
        )
    } else {
        bail!("不支持的 Agent")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_and_unrelated_mcp_survive() {
        let old="# provider\nmodel_provider = 'ccswitch'\n[model_providers.ccswitch]\nbase_url = 'http://localhost:15721'\n[mcp_servers.other]\ncommand = 'other'\n";
        let out = render(
            old,
            "codex",
            "demo",
            "/Applications/Skill Studio",
            "/data/mcp",
            false,
        )
        .unwrap();
        assert!(out.contains("# provider"));
        assert!(out.contains("base_url = 'http://localhost:15721'"));
        assert!(out.contains("command = 'other'"));
        let out = render(
            &out,
            "codex",
            "demo",
            "/Applications/Skill Studio",
            "/data/mcp",
            true,
        )
        .unwrap();
        assert!(!out.contains("studio-demo"));
        assert!(out.contains("model_provider = 'ccswitch'"));
    }
    #[test]
    fn refuse_modified_or_foreign_entry() {
        let old = r#"{"mcpServers":{"studio-demo":{"command":"other"}}}"#;
        assert!(render(old, "claude", "demo", "/app", "/data", false).is_err());
        let old = r#"{"env":{"ANTHROPIC_BASE_URL":"http://localhost:15721"},"mcpServers":{"other":{"command":"other"}}}"#;
        let out = render(old, "claude", "demo", "/app", "/data", false).unwrap();
        let out: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(out["env"]["ANTHROPIC_BASE_URL"], "http://localhost:15721");
        assert_eq!(out["mcpServers"]["other"]["command"], "other");
        assert!(out["mcpServers"]["studio-demo"].get("headers").is_none());
    }
}
