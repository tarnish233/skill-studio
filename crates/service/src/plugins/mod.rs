//! Plugin Hub: native package management only, never an extension runtime or format converter.
mod inventory;
mod native;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use skill_studio_core::{
    fs::{atomic, paths},
    models::config::AppConfig,
    services::plugins::PluginPackage,
};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    pub id: String,
    pub agent_id: String,
    pub native_id: String,
    pub source: String,
    pub scope: String,
    pub profile: Option<String>,
    pub config_path: PathBuf,
    /// Availability of package files, not proof that an Agent session loaded them.
    pub available: bool,
    pub configured: bool,
    pub enabled: Option<bool>,
    pub revision: String,
    pub actions: Vec<PluginAction>,
    pub read_only_reason: Option<String>,
    pub package: PluginPackage,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInventory {
    pub plugins: Vec<PluginView>,
    pub warnings: Vec<String>,
    pub pending: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PluginAction {
    Install,
    Enable,
    Disable,
    Update,
    Uninstall,
    Forget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginOperation {
    pub action: PluginAction,
    pub agent_id: String,
    pub plugin_id: Option<String>,
    pub expected_revision: Option<String>,
    pub source: Option<String>,
    pub marketplace: Option<String>,
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginOperationResult {
    pub changed: bool,
    pub pending: bool,
    pub message: String,
    pub warnings: Vec<String>,
    pub backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCapability {
    pub agent_id: String,
    pub cli_version: Option<String>,
    pub install: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Catalog {
    pub paused: Vec<PausedPlugin>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PausedPlugin {
    pub id: String,
    pub agent: String,
    pub config_path: PathBuf,
    pub source: String,
    pub original: Value,
    pub applied: Value,
    pub index: usize,
    pub package_root: Option<PathBuf>,
    /// 撤回前声明所在的配置键；OpenCode 旧版 plugin 键需要它精确还原。
    #[serde(default)]
    pub field: Option<String>,
}

pub(crate) struct Found {
    pub view: PluginView,
    pub source: String,
    pub declaration: Value,
    pub index: Option<usize>,
    pub paused: Option<PausedPlugin>,
    /// 声明所在的配置键。None 时按 Agent 推断（pi 为 packages，其余为 plugins）。
    pub field: Option<String>,
}

pub fn scan_plugins(state: &AppState) -> Result<PluginInventory, String> {
    state.ensure_loaded().map_err(String::from)?;
    let config = state.config().clone();
    let catalog = read_catalog(state)?;
    let (found, warnings) = inventory::scan(&config, &catalog);
    Ok(PluginInventory {
        plugins: found.into_iter().map(|found| found.view).collect(),
        warnings,
        pending: dir(state).join("pending-toggle.json").exists(),
    })
}

pub fn inspect_plugin(
    state: &AppState,
    path: PathBuf,
    agent_id: Option<String>,
) -> Result<PluginPackage, String> {
    state.ensure_loaded().map_err(String::from)?;
    if let Some(agent) = &agent_id {
        skill_studio_core::models::agent::require_agent(agent).map_err(String::from)?;
    }
    Ok(skill_studio_core::services::plugins::inspect(
        &path,
        agent_id.as_deref(),
    ))
}

pub fn probe_plugin_cli(state: &AppState) -> Result<Vec<PluginCapability>, String> {
    state.ensure_loaded().map_err(String::from)?;
    let config = state.config().clone();
    Ok(skill_studio_core::models::agent::AGENTS
        .iter()
        .map(|agent| native::capability(&config, agent.id))
        .collect())
}

pub fn plugin_operation(
    state: &AppState,
    input: PluginOperation,
) -> Result<PluginOperationResult, String> {
    native::operate(state, input, false)
}

pub fn recover_plugin_write(
    state: &AppState,
    confirmed: bool,
) -> Result<PluginOperationResult, String> {
    native::repair(state, confirmed)
}

pub fn plugin_operation_cli(
    state: &AppState,
    input: PluginOperation,
    capture_signals: bool,
) -> Result<PluginOperationResult, String> {
    native::operate(state, input, capture_signals)
}

pub(crate) fn dir(state: &AppState) -> PathBuf {
    state.studio().store().dir().join("plugins")
}
pub(crate) fn read_catalog(state: &AppState) -> Result<Catalog, String> {
    atomic::read_json_file(&dir(state).join("catalog.json"))
        .map(|value| value.unwrap_or_default())
        .map_err(|_| "插件管理记录损坏，未回退为空记录".into())
}
pub(crate) fn digest(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
pub(crate) fn key(agent: &str, config: &Path, source: &str) -> String {
    digest(format!("{agent}\0{}\0{source}", config.display()).as_bytes())
}

pub(crate) fn read_text(path: &Path) -> Result<Option<String>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(format!("无法读取 {}", path.display())),
    };
    if !file
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= 4 * 1024 * 1024)
    {
        return Err(format!("{} 不是普通配置文件或超过 4 MiB", path.display()));
    }
    let mut text = String::new();
    file.take(4 * 1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|_| format!("无法读取 {}", path.display()))?;
    if text.len() > 4 * 1024 * 1024 {
        return Err("配置超过 4 MiB".into());
    }
    Ok(Some(text))
}
pub(crate) fn config_document(path: &Path, agent: &str) -> Result<Value, String> {
    let text = read_text(path)?.unwrap_or_else(|| "{}".into());
    if agent == "deepseek-harness" || agent == "claude-code" {
        serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|_| format!("{} 不是有效 JSON", path.display()))
    } else {
        skill_studio_mcp::native::document(&text, agent)
            .map_err(|_| format!("{} 配置格式无效", path.display()))
    }
}

pub(crate) fn source_of<'a>(value: &'a Value, agent: &str) -> Option<&'a str> {
    value.as_str().or_else(|| {
        value
            .get(if agent == "opencode" {
                "package"
            } else {
                "source"
            })
            .and_then(Value::as_str)
    })
}

pub(crate) fn npm_name(source: &str) -> Option<&str> {
    let spec = source.strip_prefix("npm:").unwrap_or(source);
    let name = if spec.starts_with('@') {
        spec.rfind('@')
            .filter(|index| *index > 0)
            .map(|index| &spec[..index])
            .unwrap_or(spec)
    } else {
        spec.split('@').next()?
    };
    let valid_part = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if let Some(scoped) = name.strip_prefix('@') {
        let (scope, package) = scoped.split_once('/')?;
        (valid_part(scope) && valid_part(package)).then_some(name)
    } else {
        valid_part(name).then_some(name)
    }
}

pub(crate) fn local_source(source: &str, base: &Path) -> Option<PathBuf> {
    let source = source
        .strip_prefix("link:")
        .or_else(|| {
            source
                .strip_prefix("file:")
                .filter(|s| !s.starts_with("//"))
        })
        .unwrap_or(source);
    if source.starts_with("file://") {
        return url::Url::parse(source).ok()?.to_file_path().ok();
    }
    let path = if let Some(rest) = source.strip_prefix("~/") {
        paths::home_dir().join(rest)
    } else if Path::new(source).is_absolute() {
        PathBuf::from(source)
    } else if source.starts_with("./") || source.starts_with("../") {
        base.join(source)
    } else {
        return None;
    };
    Some(path.canonicalize().unwrap_or(path))
}

pub(crate) fn package_local_path(agent: &str, source: &str, base: &Path) -> Option<PathBuf> {
    local_source(source, base).or_else(|| {
        if agent == "pi"
            && !source.starts_with("npm:")
            && !source.starts_with("git:")
            && !source.contains("://")
            && !source.starts_with("git@")
        {
            let path = base.join(source);
            Some(path.canonicalize().unwrap_or(path))
        } else {
            None
        }
    })
}

pub(crate) fn source_identity(agent: &str, source: &str, base: &Path) -> String {
    if let Some(path) = package_local_path(agent, source, base) {
        return format!("local:{}", path.display());
    }
    if let Some(name) = npm_name(source) {
        return format!("npm:{name}");
    }
    let source = source.strip_prefix("git:").unwrap_or(source);
    let source = source
        .rfind('@')
        .filter(|index| *index > source.rfind('/').unwrap_or(usize::MAX))
        .map(|index| &source[..index])
        .unwrap_or(source);
    let repo = if let Ok(url) = url::Url::parse(source) {
        format!("{}{}", url.host_str().unwrap_or(""), url.path())
    } else {
        source.trim_start_matches("git@").replacen(':', "/", 1)
    };
    format!("git:{}", repo.trim_end_matches(".git"))
}

pub(crate) fn display_source(source: &str) -> String {
    let prefix = if source.starts_with("git:") {
        "git:"
    } else {
        ""
    };
    let raw = source.strip_prefix(prefix).unwrap_or(source);
    if let Ok(mut url) = url::Url::parse(raw) {
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_query(None);
        format!("{prefix}{url}")
    } else {
        source.to_string()
    }
}
