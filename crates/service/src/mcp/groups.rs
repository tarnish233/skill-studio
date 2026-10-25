//! MCP 分组：一组 MCP，可为每个 Agent 单独启用，多个分组可以叠加。

use std::path::Path;

use skill_studio_mcp::{
    agents as mcp_agents,
    management::groups::{self, Group, GroupSnapshot, Untouched},
};

use super::{
    after_write, command, dir, ensure_managed_in, ensure_writable, scanned_imports, ImportRef,
};
use crate::state::AppState;

/// 保存分组的名称和成员。`imports` 是选中的原有配置；`expected` 是打开编辑时的样子
/// （新建为 `None`），别处改过就拒绝保存
pub fn save(
    state: &AppState,
    group: Group,
    imports: &[ImportRef],
    expected: Option<&GroupSnapshot>,
) -> Result<(), String> {
    ensure_writable(state)?;
    let dir = dir(state);
    let imports = scanned_imports(state, &dir, imports).map_err(|e| e.to_string())?;
    groups::save_group(&dir, group, imports, expected).map_err(|e| e.to_string())?;
    after_write(state);
    Ok(())
}

/// 删除分组记录，不删 Hub 里的 MCP；还有 Agent 在用时先停用
pub fn remove(state: &AppState, id: &str) -> Result<(), String> {
    ensure_writable(state)?;
    groups::remove_group(&dir(state), id).map_err(|e| e.to_string())?;
    after_write(state);
    Ok(())
}

pub fn reorder(state: &AppState, ids: &[String]) -> Result<(), String> {
    ensure_writable(state)?;
    groups::reorder(&dir(state), ids).map_err(|e| e.to_string())?;
    after_write(state);
    Ok(())
}

/// 为一个 Agent 启用或停用一个分组，它叠加的其他分组不变；已是目标状态时什么也不写。
/// 返回部署后被其他程序改写、原样留下的接入
pub fn set(
    state: &AppState,
    executable: Option<&Path>,
    agent: &str,
    group_id: &str,
    enabled: bool,
) -> Result<Vec<Untouched>, String> {
    ensure_writable(state)?;
    // 设置先拷一份：MCP 锁内不再读配置（见 ensure_managed_in 的说明）
    let settings = state.config().settings.clone();
    if enabled {
        ensure_managed_in(&settings, agent)?;
    }
    let untouched = groups::set_active(
        &dir(state),
        agent,
        group_id,
        enabled,
        || mcp_agents::write_path(agent, &settings.agent_dir_overrides, None),
        command(executable),
    )
    .map_err(|e| e.to_string())?;
    after_write(state);
    Ok(untouched)
}

/// 应用修改：按最新成员重新写入这个 Agent 叠加的全部分组
pub fn reapply(
    state: &AppState,
    executable: Option<&Path>,
    agent: &str,
) -> Result<Vec<Untouched>, String> {
    ensure_writable(state)?;
    // 设置先拷一份：MCP 锁内不再读配置（见 ensure_managed_in 的说明）
    let settings = state.config().settings.clone();
    let untouched = groups::reapply(
        &dir(state),
        agent,
        // 没有启用的分组时什么也不写，也就不用管它是否在管理中
        || {
            ensure_managed_in(&settings, agent).map_err(anyhow::Error::msg)?;
            mcp_agents::write_path(agent, &settings.agent_dir_overrides, None)
        },
        command(executable),
    )
    .map_err(|e| e.to_string())?;
    after_write(state);
    Ok(untouched)
}
