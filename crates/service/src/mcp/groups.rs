//! MCP 分组：一组 MCP，可为每个 Agent 单独启用，多个分组可以叠加。

use std::path::Path;

use skill_studio_mcp::{
    agents as mcp_agents,
    management::groups::{self, Group, GroupSnapshot, Untouched},
};

use super::{
    after_write, command, ensure_managed_in, ensure_writable, scanned_imports, with_write,
    ImportRef,
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
    with_write(state, |dir, config| {
        let imports = scanned_imports(config, dir, imports)?;
        groups::save_group(dir, group, imports, expected)
    })?;
    after_write(state);
    Ok(())
}

/// 删除分组记录，不删 Hub 里的 MCP；还有 Agent 在用时先停用
pub fn remove(state: &AppState, id: &str) -> Result<(), String> {
    ensure_writable(state)?;
    with_write(state, |dir, _| groups::remove_group(dir, id))?;
    after_write(state);
    Ok(())
}

pub fn reorder(state: &AppState, ids: &[String]) -> Result<(), String> {
    ensure_writable(state)?;
    with_write(state, |dir, _| groups::reorder(dir, ids))?;
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
    let untouched = with_write(state, |dir, config| {
        let settings = &config.settings;
        if enabled {
            ensure_managed_in(settings, agent).map_err(anyhow::Error::msg)?;
        }
        groups::set_active(
            dir,
            agent,
            group_id,
            enabled,
            || mcp_agents::write_path(agent, &settings.agent_dir_overrides, None),
            command(executable),
        )
    })?;
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
    let untouched = with_write(state, |dir, config| {
        let settings = &config.settings;
        groups::reapply(
            dir,
            agent,
            || {
                ensure_managed_in(settings, agent).map_err(anyhow::Error::msg)?;
                mcp_agents::write_path(agent, &settings.agent_dir_overrides, None)
            },
            command(executable),
        )
    })?;
    after_write(state);
    Ok(untouched)
}
