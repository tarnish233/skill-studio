//! 官方目录和一次性 MCP 连接验证；不启动网关、不改 Agent 配置。
use super::{load, redact, resolve};
use crate::{
    commands::Ctx,
    error::CliError,
    redact::{redact_url, HIDDEN},
    report::Report,
};
use base64::Engine as _;
use serde_json::{json, Value};
use skill_studio_mcp::{gateway, registry};
use skill_studio_service::mcp::{self as service, requests};
use std::time::Duration;

fn runtime() -> Result<tokio::runtime::Runtime, CliError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CliError::failed(e.to_string()))
}
pub fn search(_ctx: &Ctx, query: &str, cursor: Option<&str>) -> Result<Report, CliError> {
    let query =
        registry::validate_query(query).map_err(|error| CliError::usage(error.to_string()))?;
    let page = runtime()?
        .block_on(registry::search(query, cursor))
        .map_err(|_| CliError::failed("官方 MCP 目录搜索失败，请检查网络后重试"))?;
    Ok(search_report(page))
}
fn search_report(page: registry::SearchPage) -> Report {
    let mut text = format!(
        "官方 MCP 目录找到 {} 个服务（只查询，未安装）",
        page.items.len()
    );
    let items = page.items.into_iter().map(|item| {
        let definition = item.definition.as_ref().map(redact);
        let install = definition.as_ref().map(|definition| {
            let config = json!({"mcpServers":{(item.name.clone()): definition}}).to_string();
            format!("skill-studio mcp add --config '{}'", config.replace('\'', "'\\''"))
        });
        text.push_str(&format!("\n  {} · {} · {}", item.name, item.version, item.source));
        if let Some(install) = &install { text.push_str(&format!("\n    安装：{install}")); }
        json!({"name":item.name,"description":item.description,"version":item.version,"source":item.source,"definition":definition,"install":install})
    }).collect::<Vec<_>>();
    if page.next_cursor.is_some() {
        text.push_str("\n还有下一页，使用 --cursor 与 JSON 中的 nextCursor 继续查询");
    }
    Report::new(text, json!({"items":items,"nextCursor":page.next_cursor}))
}

/// Upstream errors may echo credential-bearing URLs, environment values or arguments.
/// Collect the fields hidden by the normal definition renderer and scrub their original bytes.
pub(super) fn safe_error(message: &str, definition: &Value) -> String {
    fn collect(original: &Value, safe: &Value, secrets: &mut Vec<String>) {
        match original {
            Value::String(value) if safe.as_str() != Some(value) && !value.is_empty() => {
                secrets.push(value.clone());
                if let Some((scheme, token)) = value.split_once(' ') {
                    if (scheme.eq_ignore_ascii_case("bearer")
                        || scheme.eq_ignore_ascii_case("basic"))
                        && !token.is_empty()
                    {
                        secrets.push(token.into());
                    }
                }
                if let Some((scheme, token)) = value.split_once(' ') {
                    if scheme.eq_ignore_ascii_case("basic") {
                        let decoded = base64::engine::general_purpose::STANDARD
                            .decode(token)
                            .or_else(|_| {
                                base64::engine::general_purpose::STANDARD_NO_PAD.decode(token)
                            });
                        if let Ok(decoded) = decoded {
                            if let Ok(credential) = String::from_utf8(decoded) {
                                secrets.extend(
                                    credential
                                        .split(':')
                                        .filter(|part| !part.is_empty())
                                        .map(str::to_string),
                                );
                            }
                        }
                    }
                }
                if let Some((_, value)) = value.split_once('=') {
                    if !value.is_empty() {
                        secrets.push(value.into());
                    }
                }
            }
            Value::Object(map) => {
                for (key, value) in map {
                    collect(value, &safe[key], secrets);
                }
            }
            Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    collect(value, &safe[index], secrets);
                }
            }
            _ => {}
        }
    }
    let mut secrets = vec![];
    collect(definition, &redact(definition), &mut secrets);
    if let Some(url) = definition["url"].as_str() {
        if let Some((_, query)) = url.split_once('?') {
            for pair in query.split('#').next().unwrap_or("").split('&') {
                let value = pair.split_once('=').map_or(pair, |(_, value)| value);
                if !value.is_empty() {
                    secrets.push(value.into());
                    secrets.push(
                        percent_encoding::percent_decode_str(&value.replace('+', " "))
                            .decode_utf8_lossy()
                            .into_owned(),
                    );
                }
            }
        }
        if let Some((_, fragment)) = url.split_once('#') {
            if !fragment.is_empty() {
                secrets.push(fragment.into());
                secrets.push(
                    percent_encoding::percent_decode_str(fragment)
                        .decode_utf8_lossy()
                        .into_owned(),
                );
            }
        }
        if let Some((_, rest)) = url.split_once("://") {
            if let Some((userinfo, _)) = rest
                .split(['/', '?', '#'])
                .next()
                .unwrap_or("")
                .rsplit_once('@')
            {
                for value in userinfo.split(':').filter(|value| !value.is_empty()) {
                    secrets.push(value.into());
                    secrets.push(
                        percent_encoding::percent_decode_str(value)
                            .decode_utf8_lossy()
                            .into_owned(),
                    );
                }
            }
        }
    }
    secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    let mut message = message.to_string();
    if let Some(url) = definition["url"].as_str() {
        message = message.replace(url, &redact_url(url));
    }
    for secret in secrets {
        message = message.replace(&secret, HIDDEN);
    }
    message
}
pub(super) fn safe_cli_error(message: &str, definition: &Value) -> CliError {
    // Classify before scrubbing: a credential can happen to contain a lock/conflict marker.
    let mut error = CliError::from(message.to_string());
    error.message = safe_error(&error.message, definition);
    error.hint = error.hint.map(|hint| safe_error(&hint, definition));
    error.candidates = error
        .candidates
        .into_iter()
        .map(|candidate| safe_error(&candidate, definition))
        .collect();
    error
}

fn scrub(value: &mut Value, definition: &Value) {
    match value {
        Value::String(text) => *text = safe_error(text, definition),
        Value::Object(map) => {
            for (key, mut value) in std::mem::take(map) {
                scrub(&mut value, definition);
                map.insert(safe_error(&key, definition), value);
            }
        }
        Value::Array(values) => {
            for value in values {
                scrub(value, definition);
            }
        }
        _ => {}
    }
}
fn cancellation(
    enabled: bool,
) -> Result<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>, CliError> {
    if !enabled {
        return Ok(Box::pin(std::future::pending()));
    }
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut interrupt = signal(SignalKind::interrupt())
            .map_err(|error| CliError::failed(format!("无法监听取消信号：{error}")))?;
        let mut terminate = signal(SignalKind::terminate())
            .map_err(|error| CliError::failed(format!("无法监听取消信号：{error}")))?;
        Ok(Box::pin(async move {
            tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} }
        }))
    }
    #[cfg(windows)]
    {
        let mut interrupt = tokio::signal::windows::ctrl_c()
            .map_err(|error| CliError::failed(format!("无法监听取消信号：{error}")))?;
        let mut terminate = tokio::signal::windows::ctrl_break()
            .map_err(|error| CliError::failed(format!("无法监听取消信号：{error}")))?;
        Ok(Box::pin(async move {
            tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} }
        }))
    }
    #[cfg(not(any(unix, windows)))]
    {
        Ok(Box::pin(std::future::pending()))
    }
}

async fn wait_for_probe<F: std::future::Future>(
    operation: F,
    duration: Duration,
    capture_signals: bool,
    label: &str,
) -> Result<F::Output, CliError> {
    // Register handlers before polling the operation, which can spawn its child immediately.
    let cancellation = cancellation(capture_signals)?;
    tokio::select! {
        _ = cancellation => Err(CliError::failed(format!("{label}已取消"))),
        result = tokio::time::timeout(duration, operation) => result.map_err(|_| CliError::failed(format!("{label}超过 {} 秒，已取消", duration.as_secs()))),
    }
}

pub fn test(ctx: &Ctx, input: &str, timeout: u64) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let mcp = load(&state)?;
    let row = resolve(&mcp, input)?;
    if !row.managed {
        return Err(CliError::failed(
            "请先将这个 MCP 保存或托管到 Hub，再测试连接",
        ));
    }
    let duration = Duration::from_secs(timeout);
    let rt = runtime()?;
    let result = if row.entry.mode == "direct" {
        let (server, startup) = requests::direct_probe(&state, &row.entry.id)
            .map_err(|error| safe_cli_error(&error, &row.entry.definition))?;
        rt.block_on(wait_for_probe(
            gateway::probe_direct(server, startup.min(duration)),
            duration,
            ctx.capture_signals,
            "MCP 连接测试",
        ))?
        .map_err(|error| {
            CliError::failed(format!(
                "MCP 连接测试失败：{}",
                safe_error(&format!("{error:#}"), &row.entry.definition)
            ))
        })?
    } else {
        rt.block_on(wait_for_probe(
            gateway::request(&service::dir(&state), "test", json!({"id":row.entry.id})),
            duration,
            ctx.capture_signals,
            "网关测试",
        ))?
        .map_err(|error| {
            CliError::failed(format!(
                "网关测试失败：{}",
                safe_error(&format!("{error:#}"), &row.entry.definition)
            ))
            .hint("请先在桌面应用中启动网关；CLI 不会自动启动或授权")
        })?
    };
    if result["authRequired"] == true {
        return Err(CliError::failed(
            "服务需要认证，请在 Agent（直连）或桌面应用（网关）中登录后重试",
        ));
    }
    let mut tools = result;
    scrub(&mut tools, &row.entry.definition);
    let items = tools
        .as_array()
        .ok_or_else(|| CliError::failed("服务返回的工具列表格式无效"))?;
    let mut text = format!(
        "「{}」连接已验证，发现 {} 个工具；不代表 Agent 已重新加载或完成登录",
        row.entry.name,
        items.len()
    );
    for tool in items {
        text.push_str(&format!("\n  {}", tool["name"].as_str().unwrap_or("")));
    }
    Ok(Report::new(
        text,
        json!({"ok":true,"status":"ok","mcp":row.entry.id,"tools":tools}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic_auth_passwords_are_scrubbed_after_server_decoding() {
        let definition = json!({"headers":{"Authorization":"Basic dXNlcjpwcml2YXRlLXNlY3JldA=="}});
        let message = safe_error("private-secret", &definition);
        assert!(!message.contains("private-secret"));
    }

    #[test]
    fn encoded_query_credentials_are_scrubbed_when_decoded_by_the_server() {
        let definition =
            json!({"type":"http","url":"https://example.com/mcp?token=query%20secret"});
        let mut tools = json!([{"name":"echo","description":"query secret"}]);
        scrub(&mut tools, &definition);
        assert!(!tools.to_string().contains("query secret"));
    }
    #[test]
    fn credentials_echoed_as_json_property_names_are_scrubbed() {
        let definition = json!({"headers":{"X-Token":"schema-secret"}});
        let mut tools = json!([{"name":"echo","inputSchema":{"properties":{"schema-secret":{"type":"string"}}}}]);
        scrub(&mut tools, &definition);
        assert!(!tools.to_string().contains("schema-secret"));
    }
    #[test]
    fn redaction_preserves_the_no_audit_contract_for_a_disabled_cli() {
        let message = crate::cli_disabled().message;
        let definition = json!({"env":{"PRIVATE_MESSAGE":message}});
        let error = safe_cli_error(&message, &definition);
        let home = tempfile::tempdir().unwrap();
        let ctx = crate::commands::Ctx {
            yes: true,
            store: skill_studio_core::services::store::Store::new(home.path().into()),
            wait: std::time::Duration::ZERO,
            ignored_env: vec![],
            capture_signals: false,
        };
        let args = ["skill-studio", "mcp", "set", "entry"].map(std::ffi::OsString::from);
        crate::audit(&ctx, &args, &Err(error));
        assert!(!home.path().join("logs").exists());
    }

    #[test]
    fn redaction_does_not_change_the_busy_exit_code() {
        let busy = skill_studio_core::services::store::CONFIG_BUSY;
        let definition = json!({"env":{"PRIVATE_MESSAGE":busy}});
        let error = safe_cli_error(busy, &definition);
        assert_eq!(error.exit_code(), 4);
        assert!(!error.message.contains(busy));
    }

    #[test]
    fn registry_results_preserve_pagination_and_manual_configuration_candidates() {
        let report = search_report(registry::SearchPage {
            items: vec![
                registry::ResultItem {
                    name: "example".into(),
                    description: "demo".into(),
                    version: "1".into(),
                    definition: Some(json!({"type":"http","url":"https://example.com/mcp"})),
                    source: "HTTP".into(),
                },
                registry::ResultItem {
                    name: "manual".into(),
                    description: "needs input".into(),
                    version: "2".into(),
                    definition: None,
                    source: "需手动配置".into(),
                },
            ],
            next_cursor: Some("opaque==".into()),
        });
        assert_eq!(report.json["nextCursor"], "opaque==");
        assert!(report.json["items"][0]["install"]
            .as_str()
            .unwrap()
            .contains("mcp add --config"));
        assert!(report.json["items"][1]["install"].is_null());
    }
    #[test]
    fn upstream_error_and_tools_cannot_echo_definition_credentials() {
        let definition = json!({"url":"https://user:password@example.com/mcp?token=abc_secret","env":{"KEY":"env_secret"},"args":["--token=arg_secret"],"extra":{"password":"nested_secret"}});
        let message = safe_error(
            "abc_secret password env_secret arg_secret nested_secret",
            &definition,
        );
        for value in [
            "abc_secret",
            "password",
            "env_secret",
            "arg_secret",
            "nested_secret",
        ] {
            assert!(!message.contains(value), "{message}");
        }
    }
}
