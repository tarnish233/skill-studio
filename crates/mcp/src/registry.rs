//! Read-only discovery from the official MCP Registry.
use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;

const URL: &str = "https://registry.modelcontextprotocol.io/v0.1/servers";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultItem {
    pub name: String,
    pub description: String,
    pub version: String,
    pub definition: Option<Value>,
    pub source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub items: Vec<ResultItem>,
    pub next_cursor: Option<String>,
}

fn item(value: &Value) -> Option<ResultItem> {
    let server = &value["server"];
    let name = server["title"]
        .as_str()
        .or_else(|| server["name"].as_str())?
        .to_owned();
    let remote = server["remotes"].as_array().and_then(|remotes| {
        remotes.iter().find_map(|remote| {
            let url = remote["url"].as_str()?;
            (remote["type"] == "streamable-http"
                && url.starts_with("https://")
                && !url.contains('{')
                && remote["headers"].as_array().is_none_or(Vec::is_empty)
                && remote["variables"]
                    .as_object()
                    .is_none_or(serde_json::Map::is_empty))
            .then(|| json!({"type":"http","url":url}))
        })
    });
    let npm = server["packages"].as_array().and_then(|packages| {
        packages.iter().find_map(|package| {
            let identifier = package["identifier"].as_str()?;
            let version = package["version"].as_str()?;
            let safe = identifier
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"@/._-".contains(&b))
                && version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b));
            (package["registryType"] == "npm"
                && package["transport"]["type"] == "stdio"
                && safe
                && package["environmentVariables"].as_array().is_none_or(Vec::is_empty)
                && package["packageArguments"].as_array().is_none_or(Vec::is_empty)
                && package["runtimeArguments"].as_array().is_none_or(Vec::is_empty))
            .then(|| json!({"type":"stdio","command":"npx","args":["-y",format!("{identifier}@{version}")]}))
        })
    });
    let source = if remote.is_some() {
        "HTTP"
    } else if npm.is_some() {
        "npm"
    } else {
        "需手动配置"
    };
    Some(ResultItem {
        name,
        description: server["description"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        version: server["version"].as_str().unwrap_or_default().to_owned(),
        definition: remote.or(npm),
        source: source.into(),
    })
}

fn page(payload: &Value) -> Result<SearchPage> {
    let servers = payload["servers"]
        .as_array()
        .context("官方 MCP 目录返回了无效数据")?;
    Ok(SearchPage {
        items: servers.iter().filter_map(item).collect(),
        next_cursor: payload["metadata"]["nextCursor"]
            .as_str()
            .map(str::to_owned),
    })
}

pub fn validate_query(query: &str) -> Result<&str> {
    let query = query.trim();
    if !(2..=100).contains(&query.chars().count()) {
        bail!("请输入 2 到 100 个字符的服务名称");
    }
    Ok(query)
}

pub async fn search(query: &str, cursor: Option<&str>) -> Result<SearchPage> {
    let query = validate_query(query)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(format!("SkillStudio/{}", env!("CARGO_PKG_VERSION")))
        .build()?;
    let mut request = client
        .get(URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .query(&[("search", query), ("version", "latest"), ("limit", "20")]);
    if let Some(cursor) = cursor {
        request = request.query(&[("cursor", cursor)]);
    }
    let response = request.send().await?;
    if !response.status().is_success() {
        bail!("官方 MCP 目录暂时不可用（HTTP {}）", response.status());
    }
    let payload: Value = response.json().await?;
    page(&payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_limits_count_unicode_characters_instead_of_utf8_bytes() {
        let query = "字".repeat(100);
        assert_eq!(validate_query(&format!("  {query}  ")).unwrap(), query);
        assert!(validate_query(&"字".repeat(101)).is_err());
        assert!(validate_query("字").is_err());
    }

    #[test]
    fn only_prefills_complete_safe_connections() {
        let remote = json!({"server":{"title":"Example","version":"1","remotes":[{"type":"streamable-http","url":"https://example.com/mcp"}]}});
        assert_eq!(
            item(&remote).unwrap().definition.unwrap()["url"],
            "https://example.com/mcp"
        );
        let required = json!({"server":{"name":"example/secret","packages":[{"registryType":"npm","identifier":"@example/mcp","version":"1.0.0","transport":{"type":"stdio"},"environmentVariables":[{"name":"API_KEY","isRequired":true}]}]}});
        assert!(item(&required).unwrap().definition.is_none());
        let template = json!({"server":{"name":"example/templated","remotes":[{"type":"streamable-http","url":"https://{host}/mcp"}]}});
        assert!(item(&template).unwrap().definition.is_none());
    }
    #[test]
    fn retains_registry_cursor_for_next_page() {
        let response = json!({"servers":[],"metadata":{"nextCursor":"opaque=="}});
        let result = page(&response).unwrap();
        assert!(result.items.is_empty());
        assert_eq!(result.next_cursor.as_deref(), Some("opaque=="));
    }
}
