//! Read plugin manifests without importing extension code or changing the package.
use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::scanner;

pub const PLUGIN_SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
pub const MCP_SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginComponent {
    pub kind: String,
    pub name: String,
    pub path: Option<PathBuf>,
    pub issue: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPackage {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub format: String,
    pub root: PathBuf,
    pub components: Vec<PluginComponent>,
    pub extensions: Vec<String>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

/// This check is intentionally shallow: a plugin in a skill root is not a movable skill.
pub fn is_plugin_directory(path: &Path) -> bool {
    if path.join(".claude-plugin/plugin.json").is_file()
        || path.join(".codex-plugin/plugin.json").is_file()
    {
        return true;
    }
    path.canonicalize()
        .ok()
        .and_then(|root| read_json(&root, &root.join("plugin.json")).ok())
        .and_then(|value| value["$schema"].as_str().map(str::to_owned))
        .is_some_and(|schema| schema.starts_with("https://agent-plugins.org/schemas/"))
}

/// `hint` identifies a native package already declared by that Agent. It does not execute it.
pub fn inspect(path: &Path, hint: Option<&str>) -> PluginPackage {
    inspect_with_manifest(path, hint, None)
}

/// `fallback` is a marketplace entry used when the installed cache has no manifest.
pub fn inspect_with_manifest(
    path: &Path,
    hint: Option<&str>,
    fallback: Option<&Value>,
) -> PluginPackage {
    let mut package = PluginPackage {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        format: "unknown".into(),
        root: path.to_path_buf(),
        ..Default::default()
    };
    match path.canonicalize() {
        Ok(root) if root.is_dir() => package.root = root,
        Ok(root)
            if root.is_file()
                && matches!(
                    root.extension().and_then(|s| s.to_str()),
                    Some("ts" | "js" | "mjs" | "cjs")
                ) =>
        {
            package.root = root.clone();
            package.format = "extension".into();
            package.components.push(PluginComponent {
                kind: "extension".into(),
                name: package.name.clone(),
                path: Some(root),
                issue: None,
            });
            return package;
        }
        Ok(_) => {
            package.error = Some("插件路径必须为目录或 JS/TS 扩展文件".into());
            return package;
        }
        Err(_) => {
            package.error = Some("插件目录不存在或无法读取".into());
            return package;
        }
    }
    if let Err(error) = inspect_inner(&mut package, hint, fallback) {
        package.error = Some(error);
        package.components.clear();
    }
    package
}

fn inspect_inner(
    package: &mut PluginPackage,
    hint: Option<&str>,
    fallback: Option<&Value>,
) -> Result<(), String> {
    let root_manifest = package.root.join("plugin.json");
    if root_manifest.exists() {
        let manifest = read_json(&package.root, &root_manifest)?;
        if manifest
            .get("$schema")
            .and_then(Value::as_str)
            .is_some_and(|s| s.starts_with("https://agent-plugins.org/schemas/"))
        {
            package.format = "agent-plugins-v1".into();
            standard(package, &manifest)?;
            return Ok(());
        }
    }
    for (manifest, format) in [
        (".claude-plugin/plugin.json", "claude"),
        (".codex-plugin/plugin.json", "codex"),
    ] {
        let path = package.root.join(manifest);
        if path.exists() {
            package.format = format.into();
            let manifest = read_json(&package.root, &path)?;
            metadata(package, &manifest)?;
            native_components(package, &manifest);
            return Ok(());
        }
    }
    let path = package.root.join("package.json");
    if path.exists() {
        let manifest = read_json(&package.root, &path)?;
        metadata(package, &manifest)?;
        if manifest["pi"].is_object() || hint == Some("pi") {
            package.format = "pi-package".into();
            pi_components(package, &manifest);
        } else if manifest["dsh"].is_object() || hint == Some("deepseek-harness") {
            package.format = "dsh-bundle".into();
            package
                .warnings
                .push("仅展示 Bundle 清单；Profile 的实际加载状态由 Harness 决定".into());
        } else if hint == Some("opencode") {
            package.format = "opencode".into();
            package.components.push(PluginComponent {
                kind: "extension".into(),
                name: package.name.clone(),
                path: Some(package.root.clone()),
                issue: None,
            });
            package
                .warnings
                .push("不执行插件代码；工具、Hook 和界面能力以 OpenCode 原生清单为准".into());
        } else {
            return Err("该目录未声明可识别的 Agent 插件格式".into());
        }
        return Ok(());
    }
    if hint == Some("pi") {
        package.format = "pi-package".into();
        pi_components(package, &serde_json::json!({}));
        return if package.components.is_empty() {
            Err("未发现 Pi 默认资源目录".into())
        } else {
            Ok(())
        };
    }
    if matches!(hint, Some("claude-code" | "grok")) {
        package.format = "claude".into();
        if let Some(entry) = fallback {
            metadata(package, entry)?;
            native_entry_components(package, entry);
            if package.components.is_empty() {
                package
                    .warnings
                    .push("市场条目未声明可静态枚举的组件".into());
            }
            return Ok(());
        }
        native_components(package, &serde_json::json!({}));
        if package.components.is_empty() {
            return Err("未找到插件清单或默认组件".into());
        }
        return Ok(());
    }
    if hint == Some("opencode") {
        package.format = "opencode".into();
        package.components.push(PluginComponent {
            kind: "extension".into(),
            name: package.name.clone(),
            path: Some(package.root.clone()),
            issue: None,
        });
        return Ok(());
    }
    Err("未找到标准 plugin.json 或已支持 Agent 的原生清单".into())
}

fn metadata(package: &mut PluginPackage, manifest: &Value) -> Result<(), String> {
    if !manifest.is_object() {
        return Err("插件清单必须为 JSON 对象".into());
    }
    if let Some(name) = manifest.get("name") {
        package.name = name
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("插件 name 必须为非空字符串")?
            .into();
    }
    for key in ["version", "description"] {
        if manifest.get(key).is_some_and(|value| !value.is_string()) {
            return Err(format!("插件 {key} 必须为字符串"));
        }
    }
    package.version = manifest["version"].as_str().map(str::to_owned);
    package.description = manifest["description"].as_str().map(str::to_owned);
    Ok(())
}

fn standard(package: &mut PluginPackage, manifest: &Value) -> Result<(), String> {
    if manifest["$schema"] != PLUGIN_SCHEMA {
        return Err("不支持该 Agent Plugins 格式版本；未回退到其他清单".into());
    }
    metadata(package, manifest)?;
    let name = manifest["name"].as_str().ok_or("标准插件缺少 name")?;
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name.as_bytes()[name.len() - 1].is_ascii_alphanumeric()
        || name.contains("--")
        || name.contains("..")
    {
        return Err("标准插件 name 不符合 1–64 位小写名称约束".into());
    }
    for key in ["homepage", "repository", "license"] {
        if manifest.get(key).is_some_and(|value| !value.is_string()) {
            return Err(format!("插件 {key} 必须为字符串"));
        }
    }
    if manifest.get("keywords").is_some_and(|v| !string_array(v)) {
        return Err("keywords 必须为字符串数组".into());
    }
    if let Some(author) = manifest.get("author") {
        let author = author.as_object().ok_or("author 必须为对象")?;
        if author
            .iter()
            .any(|(k, v)| !matches!(k.as_str(), "name" | "email" | "url") || !v.is_string())
        {
            return Err("author 包含未知字段或非字符串值".into());
        }
    }
    for key in manifest.as_object().unwrap().keys() {
        if !matches!(
            key.as_str(),
            "$schema"
                | "name"
                | "version"
                | "description"
                | "author"
                | "homepage"
                | "repository"
                | "license"
                | "keywords"
                | "extensions"
        ) {
            package.warnings.push(format!("忽略未知清单字段：{key}"));
        }
    }
    if let Some(extensions) = manifest.get("extensions") {
        if let Some(extensions) = extensions.as_object() {
            package.extensions = extensions.keys().cloned().collect();
            if !extensions.is_empty() {
                package
                    .warnings
                    .push("客户端专属扩展不属于可移植核心；是否可用由目标 Agent 决定".into());
            }
        } else {
            package
                .warnings
                .push("extensions 不是对象，已按标准忽略".into());
        }
    }
    skills(package, "skills");
    let mcp = package.root.join("mcp.json");
    if mcp.exists() {
        let result = (|| {
            let doc = read_json(&package.root, &mcp)?;
            let map = doc.as_object().ok_or("mcp.json 必须为对象")?;
            if doc["$schema"] != MCP_SCHEMA
                || map
                    .keys()
                    .any(|k| !matches!(k.as_str(), "$schema" | "mcpServers"))
            {
                return Err("mcp.json 的格式版本不匹配或包含未知顶层字段".into());
            }
            let servers = doc["mcpServers"]
                .as_object()
                .ok_or("mcpServers 必须为对象")?;
            for (name, value) in servers {
                let issue = validate_mcp(&package.root, value).err();
                package.components.push(PluginComponent {
                    kind: "mcp".into(),
                    name: name.clone(),
                    path: Some(mcp.clone()),
                    issue,
                });
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            package
                .warnings
                .push(format!("MCP 配置不可用：{error}；其他组件不受影响"));
        }
    }
    Ok(())
}

fn string_array(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|values| values.iter().all(Value::is_string))
}
fn string_map(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|values| values.values().all(Value::is_string))
}

fn validate_mcp(root: &Path, value: &Value) -> Result<(), String> {
    let map = value.as_object().ok_or("服务器配置必须为对象")?;
    let kind = value["type"].as_str().ok_or("服务器缺少 type")?;
    let fields: &[&str] = match kind {
        "stdio" => &["type", "command", "args", "env", "cwd"],
        "streamable-http" | "sse" => &["type", "url", "headers"],
        _ => return Err("不支持的 MCP 传输类型".into()),
    };
    if map.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err("服务器配置包含未知字段或其他传输类型的字段".into());
    }
    if kind == "stdio" {
        let command = value["command"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("stdio 缺少 command")?;
        if command.starts_with("./") {
            contained(root, command, true)?;
        } else if command
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '/' | '\\' | '$'))
            || Path::new(command).is_absolute()
        {
            return Err("command 必须是单个可执行名称或 ./ 开头的包内路径".into());
        }
        if value.get("args").is_some_and(|v| !string_array(v)) {
            return Err("args 必须为字符串数组".into());
        }
        if let Some(env) = value.get("env") {
            if !string_map(env) {
                return Err("env 必须为字符串键值表".into());
            }
            if env.as_object().unwrap().keys().any(|k| {
                k.eq_ignore_ascii_case("PLUGIN_ROOT") || k.eq_ignore_ascii_case("PLUGIN_DATA")
            }) {
                return Err("PLUGIN_ROOT 和 PLUGIN_DATA 由客户端提供，不能在 env 中覆盖".into());
            }
        }
        if let Some(cwd) = value.get("cwd") {
            let cwd = cwd.as_str().ok_or("cwd 必须为字符串")?;
            if cwd.starts_with("./") {
                contained(root, cwd, true)?;
            } else if cwd == "${PLUGIN_ROOT}" || cwd.starts_with("${PLUGIN_ROOT}/") {
                contained(
                    root,
                    &format!(
                        "./{}",
                        cwd.strip_prefix("${PLUGIN_ROOT}")
                            .unwrap()
                            .trim_start_matches('/')
                    ),
                    true,
                )?;
            } else if cwd == "${PLUGIN_DATA}" || cwd.starts_with("${PLUGIN_DATA}/") {
                let suffix = cwd.strip_prefix("${PLUGIN_DATA}").unwrap();
                if suffix.split(['/', '\\']).any(|p| p == "..") {
                    return Err("cwd 越过插件数据目录".into());
                }
            } else {
                return Err("cwd 必须位于插件根或插件数据目录".into());
            }
        }
    } else {
        let url = value["url"].as_str().ok_or("远程服务器缺少 url")?;
        let url = reqwest::Url::parse(url).map_err(|_| "MCP URL 无效")?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err("MCP URL 必须为 HTTP(S)，且不能包含凭据或 fragment".into());
        }
        let loopback = match url.host_str().unwrap_or("").trim_matches(['[', ']']) {
            "localhost" => true,
            host => host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback()),
        };
        if url.scheme() != "https" && !loopback {
            return Err("非回环 MCP 地址必须使用 HTTPS".into());
        }
        if let Some(headers) = value.get("headers") {
            if !string_map(headers) {
                return Err("headers 必须为字符串键值表".into());
            }
            let mut seen = HashSet::new();
            for (name, value) in headers.as_object().unwrap() {
                if name.is_empty()
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
                    || !seen.insert(name.to_ascii_lowercase())
                    || value.as_str().unwrap().bytes().any(|b| {
                        b == b'\r' || b == b'\n' || b == 0 || b == 127 || (b < 32 && b != b'\t')
                    })
                {
                    return Err("HTTP 请求头无效或名称大小写重复".into());
                }
            }
        }
    }
    Ok(())
}

fn contained(root: &Path, relative: &str, must_exist: bool) -> Result<PathBuf, String> {
    if !relative.starts_with("./") {
        return Err("包内路径必须以 ./ 开头".into());
    }
    let path = root.join(relative);
    if relative.split(['/', '\\']).any(|part| part == "..") {
        return Err("包内路径越过插件根目录".into());
    }
    match path.canonicalize() {
        Ok(resolved) if resolved.starts_with(root) => Ok(resolved),
        Ok(_) => Err("符号链接或路径越过插件根目录".into()),
        Err(_) if !must_exist => Ok(path),
        Err(_) => Err("包内文件不存在或无法读取".into()),
    }
}

fn read_json(root: &Path, path: &Path) -> Result<Value, String> {
    let resolved = path.canonicalize().map_err(|_| "清单不存在或无法读取")?;
    if !resolved.starts_with(root) {
        return Err("清单符号链接越过插件根目录".into());
    }
    let metadata = resolved.metadata().map_err(|_| "无法读取清单")?;
    if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return Err("清单必须为不超过 1 MiB 的普通文件".into());
    }
    let mut text = String::new();
    fs::File::open(resolved)
        .map_err(|_| "无法读取清单")?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| "无法读取清单")?;
    if text.len() > MAX_MANIFEST_BYTES as usize {
        return Err("清单超过 1 MiB".into());
    }
    serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|_| "清单不是有效 JSON".into())
}

fn skills(package: &mut PluginPackage, relative: &str) {
    let path = match contained(
        &package.root,
        &format!("./{}", relative.trim_start_matches("./")),
        false,
    ) {
        Ok(path) => path,
        Err(error) => {
            package.warnings.push(format!("Skill 目录不可用：{error}"));
            return;
        }
    };
    if !path.exists() {
        return;
    }
    if !path.is_dir() {
        package.warnings.push("Skill 组件位置不是目录".into());
        return;
    }
    let roots = if package.format != "agent-plugins-v1" && path.join("SKILL.md").is_file() {
        vec![path]
    } else {
        fs::read_dir(&path)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default()
    };
    for root in roots {
        if !root.join("SKILL.md").exists() {
            continue;
        }
        let markdown = match root.join("SKILL.md").canonicalize() {
            Ok(path) if path.starts_with(&package.root) && path.is_file() => path,
            _ => {
                package
                    .warnings
                    .push("跳过越界、损坏或非普通文件的 SKILL.md".into());
                continue;
            }
        };
        let fm = scanner::parse_frontmatter(&markdown);
        let issue = if fm.malformed {
            Some("SKILL.md frontmatter 无效".into())
        } else if package.format == "agent-plugins-v1"
            && (fm.name.as_deref().is_none_or(|s| s.is_empty())
                || fm.description.as_deref().is_none_or(|s| s.is_empty()))
        {
            Some("标准 Skill 必须声明 name 和 description".into())
        } else {
            None
        };
        let name = fm.name.unwrap_or_else(|| {
            root.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        package.components.push(PluginComponent {
            kind: "skill".into(),
            name,
            path: Some(root),
            issue,
        });
    }
}

fn native_components(package: &mut PluginPackage, manifest: &Value) {
    native_components_with(package, manifest, true);
}

fn native_entry_components(package: &mut PluginPackage, entry: &Value) {
    native_components_with(package, entry, false);
}

fn native_components_with(package: &mut PluginPackage, manifest: &Value, implicit: bool) {
    if implicit {
        skills(package, "skills");
    }
    if let Some(paths) = manifest.get("skills") {
        for path in paths
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_else(|| paths.as_str().into_iter().collect())
        {
            if implicit && (path == "./skills" || path == "./skills/") {
                continue;
            }
            skills(package, path);
        }
    }
    if implicit {
        flat_files(package, "commands", "command", &["md"]);
        flat_files(package, "agents", "agent", &["md"]);
    }
    for (field, kind, suffix) in [("commands", "command", "md"), ("agents", "agent", "md")] {
        if let Some(paths) = manifest.get(field) {
            for path in paths
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                .unwrap_or_else(|| paths.as_str().into_iter().collect())
            {
                flat_files(package, path, kind, &[suffix]);
            }
        }
    }
    for (field, default, kind) in [
        ("mcpServers", ".mcp.json", "mcp"),
        ("lspServers", ".lsp.json", "lsp"),
        ("hooks", "hooks/hooks.json", "hook"),
    ] {
        let declared = manifest.get(field);
        let doc = if declared.is_some_and(Value::is_object) {
            declared.cloned()
        } else {
            let relative = declared.and_then(Value::as_str).unwrap_or(default);
            let path = package.root.join(relative);
            if !path.exists() {
                None
            } else {
                match read_json(&package.root, &path) {
                    Ok(doc) => Some(doc),
                    Err(error) => {
                        package.warnings.push(format!("{kind} 清单不可读：{error}"));
                        None
                    }
                }
            }
        };
        if let Some(doc) = doc {
            let map = if kind == "hook" {
                doc.get("hooks").unwrap_or(&doc)
            } else if kind == "mcp" {
                doc.get("mcpServers").unwrap_or(&doc)
            } else {
                &doc
            };
            if let Some(map) = map.as_object() {
                for name in map.keys() {
                    package.components.push(PluginComponent {
                        kind: kind.into(),
                        name: name.clone(),
                        path: None,
                        issue: None,
                    });
                }
            } else {
                package.warnings.push(format!("{kind} 组件清单不是对象"));
            }
        }
    }
    let mut seen = HashSet::new();
    package.components.retain(|component| {
        seen.insert((
            component.kind.clone(),
            component.path.clone(),
            component.name.clone(),
        ))
    });
}

fn pi_components(package: &mut PluginPackage, manifest: &Value) {
    let pi = manifest.get("pi").and_then(Value::as_object);
    let explicit = pi.is_some_and(|pi| {
        ["skills", "extensions", "prompts", "themes"]
            .iter()
            .any(|key| pi.contains_key(*key))
    });
    for (key, kind, extensions) in [
        ("skills", "skill", &[][..]),
        ("extensions", "extension", &["ts", "js", "mjs", "cjs"][..]),
        ("prompts", "prompt", &["md"][..]),
        ("themes", "theme", &["json"][..]),
    ] {
        let paths: Vec<&str> = if explicit {
            pi.and_then(|pi| pi.get(key))
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default()
        } else {
            vec![key]
        };
        for relative in paths {
            if relative.starts_with('!') || relative.starts_with('-') {
                continue;
            }
            // shortcut: native glob declarations remain declarations, enumerate through the native Agent if exact runtime inventory is needed.
            if relative.contains(['*', '?', '[']) {
                package
                    .warnings
                    .push(format!("未展开 {key} 资源模式：{relative}"));
                continue;
            }
            if kind == "skill" {
                skills(package, relative);
            } else {
                flat_files(package, relative, kind, extensions);
            }
        }
    }
}

fn flat_files(package: &mut PluginPackage, relative: &str, kind: &str, extensions: &[&str]) {
    let path = match contained(
        &package.root,
        &format!("./{}", relative.trim_start_matches("./")),
        false,
    ) {
        Ok(path) => path,
        Err(error) => {
            package.warnings.push(format!("{kind} 组件不可读：{error}"));
            return;
        }
    };
    let paths = if path.is_dir() {
        fs::read_dir(path)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    } else {
        vec![path]
    };
    for path in paths {
        if !path.is_file()
            || !extensions.contains(&path.extension().and_then(|e| e.to_str()).unwrap_or(""))
        {
            continue;
        }
        match path.canonicalize() {
            Ok(path) if path.starts_with(&package.root) => {
                package.components.push(PluginComponent {
                    kind: kind.into(),
                    name: path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    path: Some(path),
                    issue: None,
                })
            }
            _ => package
                .warnings
                .push(format!("跳过越过插件根的 {kind} 组件")),
        }
    }
}
