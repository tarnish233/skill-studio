//! MCP：Hub、分组、项目与备份的管理。桌面应用、命令行共用这一套；网关运行、连接测试、
//! 浏览器授权这些要异步运行或依赖窗口的部分留在桌面应用里。
//!
//! 托管状态（`~/.skill-studio/mcp/catalog.json`）由它自己的锁保护，每次操作都在锁内读最新的
//! 状态再改：启停分组只动目标分组，保存时核对打开编辑时的样子，不会把别处（另一个窗口、
//! 命令行）刚做的修改盖掉。Skill 的配置只取一份快照（设置、项目目录），从不在持有托管状态
//! 锁的时候再去拿配置锁。

pub mod backups;
pub mod entries;
pub mod groups;
mod inventory;
pub mod projects;

pub use inventory::{scan_inventory, Inventory, InventoryEntry};

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};
use skill_studio_core::models::config::Settings;
use skill_studio_mcp::{
    config::{self, Server},
    discovery::{self, ScanResult},
    management::{self, Catalog, Entry},
    native,
};

use crate::state::AppState;

/// MCP 管理关闭时的提示
pub const DISABLED: &str = "MCP 管理已关闭，请先在设置中开启";

/// 托管状态所在的目录
pub fn dir(state: &AppState) -> PathBuf {
    state.studio().store().dir().join("mcp")
}

/// 设置里关掉了 MCP 管理时，所有 MCP 操作都不做
pub fn ensure_enabled(state: &AppState) -> Result<(), String> {
    if state.config().settings.manage_mcp {
        Ok(())
    } else {
        Err(DISABLED.into())
    }
}

/// 写操作的前提：可写打开，且 MCP 管理开着
fn ensure_writable(state: &AppState) -> Result<(), String> {
    state.ensure_writable().map_err(String::from)?;
    ensure_enabled(state)
}

/// MCP 用 `claude`，Skill 与设置里用 `claude-code`
pub fn app_agent(agent: &str) -> &str {
    if agent == "claude" {
        "claude-code"
    } else {
        agent
    }
}

/// 设置里退出管理的 Agent 不再写入；停用（撤回）不受限制
fn ensure_managed(state: &AppState, agent: &str) -> Result<(), String> {
    ensure_managed_in(&state.config().settings, agent)
}

/// 同 [`ensure_managed`]，用一份已经取好的设置判断。持有 MCP 锁的调用方必须走这个：
/// 锁内再读配置会和 `AppState::set_mcp_management`（持配置写锁等 MCP 锁）互相等
fn ensure_managed_in(settings: &Settings, agent: &str) -> Result<(), String> {
    let app = app_agent(agent);
    if settings.disabled_agents.iter().any(|a| a == app) {
        return Err("请先开启此 Agent 的管理".into());
    }
    Ok(())
}

/// 读托管状态。只读打开时（命令行的查看命令）不建文件、不恢复中断的写入
pub fn catalog(state: &AppState) -> Result<Catalog, String> {
    let dir = dir(state);
    if state.is_read_only() {
        management::read_only(&dir)
    } else {
        management::read(&dir)
    }
    .map_err(|e| e.to_string())
}

/// 写入后的收尾：按「配置备份保留份数」清理最旧的 MCP 备份。清理失败不影响刚完成的写入
fn after_write(state: &AppState) {
    let keep = state.config().settings.backup_keep;
    if let Err(error) = management::backups::prune(&dir(state), keep) {
        log::warn!("清理 MCP 配置备份失败: {error:#}");
    }
}

/// 网关接入写进 Agent 配置的命令；没有桌面应用时为空，只能管理直连的 MCP
fn command(executable: Option<&Path>) -> &Path {
    executable.unwrap_or(Path::new(""))
}

/// 一览：托管状态，加上扫描各 Agent 配置找到的 MCP
pub struct Overview {
    pub catalog: Catalog,
    pub scanned: ScanResult,
    /// 扫描时读不了的配置文件
    pub warnings: Vec<String>,
}

/// 读托管状态并扫描 Agent 配置。`servers` 是网关正在转发的服务（网关没在运行时取网关配置里的）
pub fn overview(state: &AppState, servers: Option<Vec<Server>>) -> Result<Overview, String> {
    ensure_enabled(state)?;
    let dir = dir(state);
    let catalog = catalog(state)?;
    let servers = match servers {
        Some(servers) => servers,
        None => config::read(&dir).map_err(|e| e.to_string())?.servers,
    };
    let (files, mut warnings) = discovery::scan_files(&state.config());
    let mut scanned = discovery::scan(&files, &servers, &dir);
    warnings.append(&mut scanned.warnings);
    Ok(Overview {
        catalog,
        scanned,
        warnings,
    })
}

/// 界面的 MCP 一览。`gateway` 是运行中的网关返回的状态；没在运行时为 `None`，按网关配置填
pub fn status(state: &AppState, gateway: Option<Value>) -> Result<Value, String> {
    ensure_enabled(state)?;
    let mut value = match gateway {
        Some(value) => value,
        None => {
            let config = config::read(&dir(state)).map_err(|e| e.to_string())?;
            json!({"running":false,"port":config.port,"servers":config.servers,"bindings":config.bindings})
        }
    };
    let servers: Vec<Server> =
        serde_json::from_value(value["servers"].clone()).map_err(|e| e.to_string())?;
    let Overview {
        catalog,
        scanned,
        warnings,
    } = overview(state, Some(servers))?;
    value["gatewayOutdated"] = json!(
        value["running"].as_bool() == Some(true)
            && value["configurationVersion"].as_u64() != Some(3)
    );
    value["groupIssues"] = json!(management::groups::issues(&catalog));
    value["projects"] = json!(catalog.projects);
    value["projectIssues"] = json!(management::projects::issues(&catalog));
    value["entries"] = json!(catalog.entries);
    value["groups"] = json!(catalog.groups);
    value["backups"] = json!(catalog
        .backups
        .iter()
        .map(|backup| json!({
            "id": backup.id,
            "paths": backup.files.iter().map(|file| &file.path).collect::<Vec<_>>(),
            "createdAt": backup.created_at,
        }))
        .collect::<Vec<_>>());
    value["activeGroups"] = json!(catalog.active_groups);
    value["discovered"] = json!(scanned.discovered);
    value["builtins"] = json!(scanned.builtins);
    value["scanWarnings"] = json!(warnings);
    Ok(value)
}

/// 粘贴的配置、安装命令或网址解析成 MCP 定义（可能有多个）
pub fn parse(text: &str) -> Result<Value, String> {
    native::parse_definition(text).map_err(|e| e.to_string())
}

/// 这个 MCP 能否由网关转发；不能时返回原因
pub fn gateway_issue(entry: &Entry) -> Option<String> {
    management::gateway_server(entry)
        .err()
        .map(|e| e.to_string())
}

/// 选中的原有配置：分组、项目只记下引用，不托管、不改动。位置由后端重新扫描确定，
/// 不信任提交上来的路径
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRef {
    pub id: String,
    pub definition: Value,
    #[serde(default)]
    pub source_ids: Vec<String>,
}

/// 按 [`ImportRef`] 从重新扫描的结果里取出原有配置
fn scanned_imports(
    state: &AppState,
    dir: &Path,
    requested: &[ImportRef],
) -> anyhow::Result<Vec<Entry>> {
    use anyhow::Context;
    if requested.is_empty() {
        return Ok(vec![]);
    }
    let (files, _) = discovery::scan_files(&state.config());
    let scanned = discovery::scan(&files, &[], dir);
    let mut imports = vec![];
    for requested in requested {
        let found = scanned
            .discovered
            .iter()
            .find(|d| {
                d.id == requested.id
                    || d.sources
                        .iter()
                        .any(|source| requested.source_ids.contains(&source.id))
            })
            .context("MCP 来源已变化，请刷新后重试")?;
        let source = found
            .sources
            .iter()
            .find(|source| {
                native::canonical(&source.definition, &source.agent) == requested.definition
            })
            .context("MCP 来源已变化，请刷新后重试")?;
        anyhow::ensure!(
            !found.sources.iter().any(|s| s.gateway),
            "请先在 Hub 管理此网关入口"
        );
        let definition = native::canonical(&source.definition, &source.agent);
        let bindings = found
            .sources
            .iter()
            .map(|s| management::Binding {
                id: s.id.clone(),
                agent: s.agent.clone(),
                path: s.path.clone(),
                project: s.project.clone(),
                key: s.key.clone(),
                original: Some(s.definition.clone()),
                installed: s.definition.clone(),
            })
            .collect();
        imports.push(Entry {
            id: requested.id.clone(),
            name: found.name.clone(),
            mode: "direct".into(),
            definition,
            oauth: false,
            client_id: None,
            scopes: vec![],
            bindings,
        });
    }
    Ok(imports)
}
