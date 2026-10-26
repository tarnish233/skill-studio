//! 界面的 MCP 请求（`mcp_request`）：桌面应用和服务器上的辅助程序共用这一份分派，Hub、
//! 分组、项目、备份与解析在本机和服务器上的行为一致。网关的启停与授权、连接测试、官方目录
//! 搜索要异步运行或依赖本机，由调用方自己处理（见 [`handles`]）。

use std::path::Path;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use skill_studio_mcp::{
    config::Server,
    management::{self, groups::GroupSnapshot, projects::Snapshot},
};

use super::{
    backups, catalog, ensure_enabled, entries, gateway_issue, groups, parse, projects, status,
    ImportRef,
};
use crate::state::AppState;

/// 由 [`request`] 处理的请求
const METHODS: &[&str] = &[
    "gatewayCheck",
    "parse",
    "list",
    "listBackups",
    "deleteBackups",
    "deleteLegacyBackups",
    "restoreBackup",
    "saveGroup",
    "removeGroup",
    "reorderGroups",
    "setGroup",
    "reapplyGroups",
    "saveProject",
    "activateProject",
    "removeProject",
    "removeProjectBindings",
    "saveEntry",
    "removeEntry",
    "removeSources",
];

/// 改动了托管状态的请求：桌面应用之后要通知运行中的网关重读
pub const CATALOG_WRITES: &[&str] = &[
    "restoreBackup",
    "saveGroup",
    "removeGroup",
    "reorderGroups",
    "setGroup",
    "reapplyGroups",
    "saveProject",
    "activateProject",
    "removeProject",
    "saveEntry",
    "removeEntry",
    "removeSources",
];

/// 这个请求由 [`request`] 处理；不是的（网关、连接测试、官方目录）由调用方处理
pub fn handles(method: &str) -> bool {
    METHODS.contains(&method)
}

/// 处理一个 MCP 请求。`executable` 是网关接入写进 Agent 配置的命令（桌面应用自己），
/// 没有桌面应用时为 `None`，只能管理 Agent 直连的 MCP；`gateway` 是运行中网关的状态，
/// 只有 `list` 用得到
pub fn request(
    state: &AppState,
    executable: Option<&Path>,
    gateway: Option<Value>,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    ensure_enabled(state)?;
    let exe = executable;
    Ok(match method {
        "gatewayCheck" => {
            let entry: management::Entry = param(params, "entry")?;
            json!({"issue": gateway_issue(&entry)})
        }
        "parse" => parse(params["text"].as_str().ok_or("请粘贴配置")?)?,
        "list" => status(state, gateway)?,
        "listBackups" => to_value(backups::list(state)?)?,
        "deleteBackups" => {
            let which: Option<String> = param(params, "which")?;
            let ids: Option<Vec<String>> = param(params, "ids").map_err(|_| "缺少备份 ID")?;
            json!({"removed": backups::delete(state, which.as_deref(), ids)?})
        }
        "deleteLegacyBackups" => json!({"removed": backups::delete_legacy(state)?}),
        "restoreBackup" => {
            let id: String = param(params, "id").map_err(|_| "缺少备份 ID")?;
            backups::restore(state, &id)?;
            Value::Null
        }
        "removeProjectBindings" => {
            let id: String = param(params, "projectId").map_err(|_| "缺少项目 ID")?;
            to_value(projects::remove_bindings(state, &id)?)?
        }
        "saveGroup" => {
            let group = param(params, "group")?;
            let imports: Vec<ImportRef> = list(params, "imports")?;
            let expected: Option<GroupSnapshot> = param(params, "expected")?;
            groups::save(state, group, &imports, expected.as_ref())?;
            Value::Null
        }
        "removeGroup" => {
            let id: String = param(params, "id").map_err(|_| "缺少分组 ID")?;
            groups::remove(state, &id)?;
            Value::Null
        }
        "reorderGroups" => {
            let ids: Vec<String> = param(params, "ids")?;
            groups::reorder(state, &ids)?;
            Value::Null
        }
        // 只启停这一个分组：这个 Agent 叠加的其他分组由后端按最新状态保留
        "setGroup" => {
            let agent: String = param(params, "agent").map_err(|_| "缺少 Agent")?;
            let group: String = param(params, "groupId").map_err(|_| "缺少分组 ID")?;
            let enabled: bool = param(params, "enabled").map_err(|_| "缺少启停")?;
            to_value(groups::set(state, exe, &agent, &group, enabled)?)?
        }
        "reapplyGroups" => {
            let agent: String = param(params, "agent").map_err(|_| "缺少 Agent")?;
            to_value(groups::reapply(state, exe, &agent)?)?
        }
        "saveProject" => {
            let id: String = param(params, "projectId").map_err(|_| "缺少项目")?;
            let members: projects::Members = whole(params)?;
            let imports: Vec<ImportRef> = list(params, "imports")?;
            let expected: Option<Snapshot> = param(params, "expected")?;
            to_value(projects::save(
                state,
                exe,
                &id,
                members,
                &imports,
                expected.as_ref(),
            )?)?
        }
        "activateProject" => {
            let id: String = param(params, "projectId").map_err(|_| "缺少项目")?;
            let agent: String = param(params, "agent").map_err(|_| "缺少 Agent")?;
            let enabled: bool = param(params, "enabled").map_err(|_| "缺少启停")?;
            to_value(projects::activate(state, exe, &id, &agent, enabled)?)?
        }
        "removeProject" => {
            let id: String = param(params, "projectId").map_err(|_| "缺少项目")?;
            projects::remove(state, &id)?;
            Value::Null
        }
        "saveEntry" => {
            let entry: management::Entry = param(params, "entry")?;
            let expected: Option<management::Entry> = param(params, "expectedEntry")?;
            let placement: entries::Placement = whole(params)?;
            entries::save(state, exe, entry, expected, &placement)?;
            Value::Null
        }
        "removeEntry" => {
            let id: String = param(params, "id").map_err(|_| "缺少 ID")?;
            let restore = params["restore"].as_bool().unwrap_or(true);
            to_value(entries::remove(state, &id, restore)?)?
        }
        "removeSources" => {
            let sources: Vec<entries::SourceRef> = list(params, "sources")?;
            entries::remove_sources(state, &sources)?;
            Value::Null
        }
        _ => return Err(format!("不支持的 MCP 操作：{method}")),
    })
}

/// 连接测试要启动的服务和启动等待上限：只测 Hub 里 Agent 直连的服务。测试本身要异步运行，
/// 由调用方完成
pub fn direct_probe(state: &AppState, id: &str) -> Result<(Server, Duration), String> {
    ensure_enabled(state)?;
    let entry = catalog(state)?
        .entries
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("MCP 不存在")?;
    if entry.mode != "direct" {
        return Err("此服务不是 Agent 直连".into());
    }
    management::direct_probe_server(&entry).map_err(|e| e.to_string())
}

/// 取一个参数；缺少时按 `null` 解析（可选参数得到 `None`）
fn param<T: DeserializeOwned>(params: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(params.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("参数 {key}: {e}"))
}

/// 列表参数：缺少时为空
fn list<T: DeserializeOwned>(params: &Value, key: &str) -> Result<Vec<T>, String> {
    Ok(param::<Option<Vec<T>>>(params, key)?.unwrap_or_default())
}

/// 整个参数对象解析成一个结构（保存位置、项目成员这类平铺在参数里的字段）
fn whole<T: DeserializeOwned>(params: &Value) -> Result<T, String> {
    serde_json::from_value(params.clone()).map_err(|e| format!("参数: {e}"))
}

fn to_value(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_write_is_handled_here_and_runtime_requests_stay_with_the_caller() {
        for method in CATALOG_WRITES {
            assert!(handles(method), "{method}");
        }
        // 网关、连接测试与官方目录要异步运行或依赖本机，不能在这里同步完成
        for method in [
            "testDirect",
            "searchRegistry",
            "start",
            "stop",
            "apply",
            "test",
            "login",
            "loginStatus",
            "logout",
        ] {
            assert!(!handles(method), "{method}");
        }
    }
}
