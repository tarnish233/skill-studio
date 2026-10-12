//! Native MCP definitions retain client-specific fields; gateway compatibility is separate.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use toml_edit::{DocumentMut, Item};

pub fn canonical(value: &Value, agent: &str) -> Value {
    let mut value = value.clone();
    if let Some(map) = value.as_object_mut() {
        if agent == "codex" {
            if let Some(headers) = map.remove("http_headers") {
                map.insert("headers".into(), headers);
            }
        }
        if agent == "opencode" {
            if let Some(disabled) = map.get("disabled").and_then(Value::as_bool) {
                map.remove("disabled");
                map.insert("enabled".into(), json!(!disabled));
            }
        }
        if agent == "opencode" {
            if let Some(command) = map.get("command").and_then(Value::as_array).cloned() {
                if let Some(program) = command.first() {
                    map.insert("command".into(), program.clone());
                    map.insert("args".into(), json!(&command[1..]));
                }
            }
            if let Some(env) = map.remove("environment") {
                map.insert("env".into(), env);
            }
            if let Some(kind) = map.get("type").and_then(Value::as_str) {
                let kind = match kind {
                    "local" => "stdio",
                    "remote" => "http",
                    other => other,
                };
                map.insert("type".into(), json!(kind));
            }
        }
        let kind = if map.contains_key("url") {
            "http"
        } else {
            "stdio"
        };
        map.entry("type").or_insert(json!(kind));
        if map.get("type").and_then(Value::as_str) == Some("streamable-http") {
            map.insert("type".into(), json!("http"));
        }
    }
    value
}
pub fn validate(value: &Value) -> Result<()> {
    if crate::discovery::is_codex_builtin(value) {
        bail!("Codex 内置服务由 Codex 管理，不能托管或修改");
    }
    let map = value.as_object().context("MCP 配置必须为对象")?;
    let kind = map
        .get("type")
        .and_then(Value::as_str)
        .context("请选择连接类型")?;
    match kind {
        "stdio" => {
            if map
                .get("command")
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty())
            {
                bail!("请填写启动命令");
            }
            if map.contains_key("url") {
                bail!("本地进程不能同时包含 URL");
            }
        }
        "http" | "streamable-http" | "sse" => {
            if map
                .get("url")
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty())
            {
                bail!("请填写 MCP 地址");
            }
            if map.contains_key("command") {
                bail!("HTTP 服务不能同时包含启动命令");
            }
        }
        _ => bail!("暂不支持该连接类型"),
    }
    if map.get("enabled").is_some_and(|v| !v.is_boolean()) {
        bail!("enabled 必须为布尔值");
    }
    if let Some(args) = map.get("args") {
        if !args
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_string))
        {
            bail!("参数必须为字符串数组");
        }
    }
    for field in ["headers", "env"] {
        if let Some(value) = map.get(field) {
            if !value.as_object().is_some_and(|m| {
                m.keys().all(|key| !key.trim().is_empty()) && m.values().all(Value::is_string)
            }) {
                bail!("{field} 必须为字符串键值表");
            }
        }
    }
    if map
        .get("cwd")
        .is_some_and(|v| !v.is_null() && !v.is_string())
    {
        bail!("工作目录必须为字符串");
    }
    Ok(())
}
pub fn for_agent(definition: &Value, agent: &str) -> Result<Value> {
    validate(definition)?;
    let mut value = definition.clone();
    let map = value.as_object_mut().unwrap();
    if agent == "codex" {
        if map.get("type").and_then(Value::as_str) == Some("sse") {
            bail!("Codex 不支持原生 SSE，请选择其他连接方式");
        }
        map.remove("type");
        if let Some(headers) = map.remove("headers") {
            map.insert("http_headers".into(), headers);
        }
        if map.get("cwd").is_some_and(Value::is_null) {
            map.remove("cwd");
        }
    } else if agent == "grok" || agent == "pi" {
        // Grok and native Pi infer the transport from command or URL.
        if map.get("type").and_then(Value::as_str) == Some("sse") {
            bail!("此 Agent 的 Studio 接入仅支持 stdio 和 Streamable HTTP");
        }
        map.remove("type");
        if map.get("cwd").is_some_and(Value::is_null) {
            map.remove("cwd");
        }
    } else if agent == "opencode" {
        let local = map["type"] == "stdio";
        if map["type"] == "sse" {
            bail!("OpenCode 接入仅支持 stdio 和 Streamable HTTP");
        }
        map.insert("type".into(), json!(if local { "local" } else { "remote" }));
        if local {
            let mut command = vec![map.remove("command").unwrap()];
            if let Some(args) = map.remove("args").and_then(|v| v.as_array().cloned()) {
                command.extend(args);
            }
            map.insert("command".into(), json!(command));
        }
        if let Some(env) = map.remove("env") {
            map.insert("environment".into(), env);
        }
        if let Some(enabled) = map.remove("enabled").and_then(|v| v.as_bool()) {
            map.insert("disabled".into(), json!(!enabled));
        }
        if map.get("cwd").is_some_and(Value::is_null) {
            map.remove("cwd");
        }
    } else if agent != "claude" {
        bail!("不支持的 Agent");
    }
    Ok(value)
}
pub fn entry(text: &str, agent: &str, project: Option<&str>, key: &str) -> Result<Option<Value>> {
    let doc = document(text, agent)?;
    if project.is_some() && agent != "claude" {
        bail!("项目本地作用域仅支持 Claude Code");
    }
    let root = project.map(|p| &doc["projects"][p]).unwrap_or(&doc);
    Ok(entries(root, agent)?.and_then(|t| t.get(key)).cloned())
}

pub fn document(text: &str, agent: &str) -> Result<Value> {
    crate::agents::validate(agent)?;
    let doc: Value = if text.trim().is_empty() {
        json!({})
    } else if matches!(agent, "codex" | "grok") {
        toml_edit::de::from_str(text).map_err(|_| anyhow::anyhow!("MCP TOML 格式无效"))?
    } else if matches!(agent, "opencode" | "pi") {
        jsonc_parser::parse_to_serde_value(text, &Default::default())
            .map_err(|_| anyhow::anyhow!("MCP JSONC 格式无效"))?
    } else {
        serde_json::from_str(text).map_err(|_| anyhow::anyhow!("MCP JSON 格式无效"))?
    };
    if !doc.is_object() {
        bail!("配置根节点必须为对象");
    }
    Ok(doc)
}
pub fn entries<'a>(
    doc: &'a Value,
    agent: &str,
) -> Result<Option<&'a serde_json::Map<String, Value>>> {
    let value = if agent == "opencode" {
        if doc.get("mcp").is_some_and(|v| !v.is_object()) {
            bail!("mcp 必须为对象");
        }
        if doc.get("mcp").and_then(Value::as_object).is_some_and(|m| {
            m.values()
                .any(|v| matches!(v["type"].as_str(), Some("local" | "remote")))
        }) {
            bail!("仅支持 OpenCode v2，请先将 v1 配置迁移到 mcp.servers");
        }
        doc.get("mcp").and_then(|m| m.get("servers"))
    } else {
        doc.get(if matches!(agent, "codex" | "grok") {
            "mcp_servers"
        } else {
            "mcpServers"
        })
    };
    value
        .map(|v| v.as_object().context("MCP 列表必须为对象"))
        .transpose()
}
pub fn enabled(value: &Value, agent: &str) -> bool {
    if agent == "opencode" && value["disabled"].as_bool() == Some(true) {
        return false;
    }
    value["enabled"].as_bool().unwrap_or(true)
}
pub fn check_activation(text: &str, agent: &str, key: &str) -> Result<()> {
    if agent == "pi" {
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            bail!("Pi MCP 名称仅支持英文字母、数字、横线和下划线");
        }
        let doc = document(text, agent)?;
        if let Some(entries) = entries(&doc, agent)? {
            if entries
                .keys()
                .any(|name| name != key && name.replace('-', "_") == key.replace('-', "_"))
            {
                bail!("Pi MCP 名称「{key}」与已有配置冲突：横线和下划线视为相同字符");
            }
        }
    }
    if agent == "grok" {
        let first = key.bytes().next();
        if first.is_none_or(|b| !b.is_ascii_alphabetic() && b != b'_')
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || key.contains("__")
            || key.ends_with('_')
        {
            bail!("Grok MCP 名称须以字母或下划线开头，仅含字母、数字、横线、下划线，不能包含双下划线或以下划线结尾");
        }
        let doc = document(text, agent)?;
        if globally_disabled(&doc, key, agent) {
            bail!("MCP「{key}」已被 Grok 全局停用，请先在 Grok 中启用");
        }
    }
    Ok(())
}
pub fn globally_disabled(doc: &Value, key: &str, agent: &str) -> bool {
    agent == "grok"
        && doc["disabled_mcp_servers"]
            .as_array()
            .is_some_and(|names| names.iter().any(|name| name.as_str() == Some(key)))
}
fn unique_properties(object: &jsonc_parser::cst::CstObject) -> Result<()> {
    let mut names = std::collections::HashSet::new();
    for property in object.properties() {
        if !names.insert(property.decoded_name()) {
            bail!("JSONC 包含重复属性，请先在原配置中消除歧义");
        }
    }
    Ok(())
}
fn cst_value(value: &Value) -> jsonc_parser::cst::CstInputValue {
    use jsonc_parser::cst::CstInputValue as C;
    match value {
        Value::Null => C::Null,
        Value::Bool(v) => C::Bool(*v),
        Value::String(v) => C::String(v.clone()),
        Value::Number(v) => C::Number(v.to_string()),
        Value::Array(v) => C::Array(v.iter().map(cst_value).collect()),
        Value::Object(v) => C::Object(v.iter().map(|(k, v)| (k.clone(), cst_value(v))).collect()),
    }
}

pub fn patch(
    text: &str,
    agent: &str,
    project: Option<&str>,
    key: &str,
    expected: &Option<Value>,
    next: &Option<Value>,
) -> Result<String> {
    let current = entry(text, agent, project, key)?;
    if agent == "codex"
        && current
            .iter()
            .chain(next.iter())
            .any(crate::discovery::is_codex_builtin)
    {
        bail!("Codex 内置服务由 Codex 管理，不能修改或删除");
    }
    if &current != expected {
        bail!("MCP「{key}」已被其他程序修改或存在同名配置，请刷新后重试");
    }
    if expected == next {
        return Ok(text.into());
    }
    if matches!(agent, "codex" | "grok") {
        let mut doc: DocumentMut = text
            .parse()
            .map_err(|_| anyhow::anyhow!("MCP TOML 格式无效"))?;
        if doc.get("mcp_servers").is_none() {
            doc["mcp_servers"] = Item::Table(Default::default());
        }
        let table = doc["mcp_servers"]
            .as_table_like_mut()
            .context("mcp_servers 必须为表")?;
        if let Some(value) = next {
            let encoded = toml_edit::ser::to_document(&json!({"mcp_servers": { key: value }}))
                .map_err(|_| anyhow::anyhow!("配置包含不能写入 TOML 的值"))?;
            table.insert(key, encoded["mcp_servers"][key].clone());
        } else {
            table.remove(key);
        }
        Ok(doc.to_string())
    } else if matches!(agent, "opencode" | "pi") {
        let root = jsonc_parser::cst::CstRootNode::parse(text, &Default::default())
            .map_err(|_| anyhow::anyhow!("MCP JSONC 格式无效"))?;
        let object = root.object_value_or_create().context("配置必须为对象")?;
        unique_properties(&object)?;
        let table = if agent == "pi" {
            object
                .object_value_or_create("mcpServers")
                .context("mcpServers 必须为对象")?
        } else {
            let mcp = object
                .object_value_or_create("mcp")
                .context("mcp 必须为对象")?;
            unique_properties(&mcp)?;
            mcp.object_value_or_create("servers")
                .context("servers 必须为对象")?
        };
        unique_properties(&table)?;
        match (table.get(key), next) {
            (Some(prop), Some(value)) => prop.set_value(cst_value(value)),
            (Some(prop), None) => prop.remove(),
            (None, Some(value)) => {
                table.append(key, cst_value(value));
            }
            (None, None) => {}
        }
        Ok(root.to_string())
    } else if agent == "claude" {
        let mut doc: Value = if text.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(text)?
        };
        let mut root = &mut doc;
        if let Some(project) = project {
            if root.get("projects").is_none() {
                root["projects"] = json!({});
            }
            if !root["projects"].is_object() {
                bail!("projects 必须为对象");
            }
            if root["projects"].get(project).is_none() {
                root["projects"][project] = json!({});
            }
            root = &mut root["projects"][project];
            if !root.is_object() {
                bail!("项目配置必须为对象");
            }
        }
        if root.get("mcpServers").is_none() {
            root["mcpServers"] = json!({});
        }
        let table = root["mcpServers"]
            .as_object_mut()
            .context("mcpServers 必须为对象")?;
        if let Some(value) = next {
            table.insert(key.into(), value.clone());
        } else {
            table.remove(key);
        }
        Ok(format!("{}\n", serde_json::to_string_pretty(&doc)?))
    } else {
        bail!("不支持的 Agent");
    }
}
pub fn parse_definition(text: &str) -> anyhow::Result<Value> {
    let trimmed = text.trim();
    if let Some(parsed) = crate::install::parse_input(trimmed)? {
        return Ok(parsed);
    }
    let json_text = if trimmed.starts_with('"') {
        format!("{{{trimmed}}}")
    } else {
        trimmed.into()
    };
    let value: Value = jsonc_parser::parse_to_serde_value(&json_text, &Default::default())
        .map_err(|_| ())
        .or_else(|_| toml_edit::de::from_str(trimmed).map_err(|_| ()))
        .map_err(|_| anyhow::anyhow!("无法解析，请粘贴 JSON 或 TOML 配置"))?;
    let (agent, entries) = if let Some(entries) = value.get("mcp_servers") {
        ("codex", Some(entries))
    } else if value.get("mcp").is_some() {
        ("opencode", Some(&value["mcp"]["servers"]))
    } else if let Some(entries) = value.get("mcpServers") {
        ("claude", Some(entries))
    } else {
        (
            if matches!(value["type"].as_str(), Some("local" | "remote")) {
                "opencode"
            } else {
                "claude"
            },
            None,
        )
    };
    let mut results = vec![];
    if let Some(entries) = entries {
        for (name, definition) in entries
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("服务列表必须为对象"))?
        {
            results.push(json!({"name":name,"definition":canonical(definition,agent)}));
        }
    } else if value.get("command").is_none()
        && value.get("url").is_none()
        && value.as_object().is_some_and(|m| m.len() == 1)
    {
        let (name, definition) = value.as_object().unwrap().iter().next().unwrap();
        results.push(json!({"name":name,"definition":canonical(definition,"claude")}));
    } else {
        results.push(json!({"name":"","definition":canonical(&value,agent)}));
    }
    if results.is_empty() {
        anyhow::bail!("没有找到 MCP 配置");
    }
    for result in &results {
        validate(&result["definition"])?;
    }
    Ok(json!(results))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_json_fragments_and_toml_and_preserves_extensions() {
        let parsed =
            parse_definition(r#""tools": {"command":"demo","startup_timeout_sec":120}"#).unwrap();
        assert_eq!(parsed[0]["name"], "tools");
        assert_eq!(parsed[0]["definition"]["startup_timeout_sec"], 120);
        let parsed=parse_definition("[mcp_servers.one]\nurl='https://example.com/mcp'\nhttp_headers={Authorization='key'}\n[mcp_servers.two]\ncommand='demo'\n").unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 2);
        assert_eq!(parsed[0]["definition"]["headers"]["Authorization"], "key");
    }
    #[test]
    fn malformed_roots_and_values_fail_before_writing() {
        assert!(patch(
            "[]",
            "claude",
            None,
            "tool",
            &None,
            &Some(json!({"command":"demo"}))
        )
        .is_err());
        assert!(patch(
            r#"{"projects":42}"#,
            "claude",
            Some("/work"),
            "tool",
            &None,
            &Some(json!({"command":"demo"}))
        )
        .is_err());
        assert!(validate(&json!({"type":"stdio","command":"demo","env":{"":"value"}})).is_err());
    }
}
