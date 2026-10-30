use super::Ctx;
use crate::{
    args::{SettingsCommand, SettingsSet},
    error::CliError,
    report::Report,
    resolve,
};
use serde_json::{json, Value};
use skill_studio_service::settings::{self, ManagedAgentPatch, SettingsPatch};
use std::collections::HashMap;

pub fn run(command: &SettingsCommand, ctx: &Ctx) -> Result<Report, CliError> {
    match command {
        SettingsCommand::Show => {
            let state = ctx.reader()?;
            let value = json!(settings::get_settings(&state)?);
            Ok(Report::new(
                serde_json::to_string_pretty(&value).unwrap(),
                value,
            ))
        }
        SettingsCommand::Set(input) => set(input, ctx),
    }
}
fn set(input: &SettingsSet, ctx: &Ctx) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let before = settings::get_settings(&state)?;
    let mut patch = SettingsPatch {
        manage_mcp: input.manage_mcp,
        preserve_manual_skills: input.preserve_manual_skills,
        cli_skill_enabled: input.cli_enabled,
        default_link_mode: input.default_link_mode.map(Into::into),
        hub_dir: input.hub_dir.as_ref().map(resolve::absolute),
        clear_hub_dir: input.clear_hub_dir.then_some(true),
        backup_keep: input.backup_keep.map(|n| n as usize),
        show_codex_builtin_mcp: input.show_codex_builtin_mcp,
        theme: input.theme.clone(),
        language: input.language.clone(),
        ..Default::default()
    };
    if !input.manage_agent.is_empty() || !input.unmanage_agent.is_empty() {
        if input.manage_agent.len() + input.unmanage_agent.len() != 1 {
            return Err(CliError::usage("每次只切换一个 Agent 的管理状态"));
        }
        let agent = input
            .manage_agent
            .first()
            .or_else(|| input.unmanage_agent.first())
            .unwrap();
        patch.managed_agent = Some(ManagedAgentPatch {
            agent: resolve::agent(agent)?.id.into(),
            enabled: !input.manage_agent.is_empty(),
        });
    }
    let mut directories = HashMap::new();
    for setting in &input.agent_dir {
        let (agent, path) = setting
            .split_once('=')
            .filter(|(_, p)| !p.trim().is_empty())
            .ok_or_else(|| {
                CliError::usage("--agent-dir 写成 Agent=目录；取消覆盖用 --clear-agent-dir")
            })?;
        let id = resolve::agent(agent)?.id.to_string();
        if directories
            .insert(id, Some(resolve::absolute(path)))
            .is_some()
        {
            return Err(CliError::usage("同一个 Agent 的目录只能指定一次"));
        }
    }
    for agent in &input.clear_agent_dir {
        let id = resolve::agent(agent)?.id.to_string();
        if directories.insert(id, None).is_some() {
            return Err(CliError::usage("同一个 Agent 的目录不能同时修改和清除"));
        }
    }
    if !directories.is_empty() {
        patch.agent_dir_updates = Some(directories);
    }
    let value = json!(patch);
    let fields = value
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, v)| !v.is_null())
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return Err(CliError::usage(
            "请指定要修改的设置，运行 settings set --help 查看参数",
        ));
    }
    let isolated = patch.managed_agent.is_some()
        || patch.preserve_manual_skills.is_some()
        || patch.manage_mcp.is_some()
        || patch.cli_skill_enabled.is_some();
    if isolated && fields.len() > 1 {
        return Err(CliError::usage(
            "管理开关与保留手动 skill 策略必须分别保存，不能混合其他设置",
        ));
    }
    let before_json = json!(before);
    let changing = fields.iter().any(|(key, wanted)| match key.as_str() {
        "clearHubDir" => !before_json["hubDir"].is_null(),
        "managedAgent" => {
            before
                .disabled_agents
                .iter()
                .any(|id| Some(id.as_str()) == wanted["agent"].as_str())
                == wanted["enabled"].as_bool().unwrap()
        }
        "agentDirUpdates" => wanted
            .as_object()
            .unwrap()
            .iter()
            .any(|(agent, path)| before_json["agentDirOverrides"][agent] != *path),
        _ => before_json[*key] != **wanted,
    });
    if !changing {
        return Ok(Report::write(
            "设置没有变化",
            false,
            false,
            &[],
            json!({"settings":before_json}),
        ));
    }
    if isolated
        || input
            .backup_keep
            .is_some_and(|n| (n as usize) < before.backup_keep)
    {
        ctx.confirm("此设置会改变管理策略、撤回接入或清理备份，需要 --yes 确认")?;
    }
    // Re-enabling suspended gateway bindings needs a running local gateway. Do not
    // silently start an application/daemon or publish unusable gateway entries.
    if input.manage_mcp == Some(true) && !before.manage_mcp {
        let dir = skill_studio_service::mcp::dir(&state);
        let catalog = skill_studio_mcp::management::read_only(&dir)
            .map_err(|e| CliError::from(e.to_string()))?;
        let needs_gateway = catalog
            .entries
            .iter()
            .any(|e| e.mode == "gateway" && !e.bindings.is_empty())
            || catalog
                .active_groups
                .values()
                .flatten()
                .flat_map(|g| &g.entries)
                .any(|e| e.mode == "gateway")
            || catalog
                .projects
                .values()
                .flat_map(|p| p.active.values())
                .flat_map(|p| &p.entries)
                .any(|e| e.mode == "gateway");
        if needs_gateway {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| CliError::failed(e.to_string()))?;
            if runtime
                .block_on(skill_studio_mcp::gateway::request(
                    &dir,
                    "list",
                    Value::Null,
                ))
                .is_err()
            {
                return Err(CliError::failed("恢复这些 MCP 接入需要运行中的本机网关")
                    .hint("请先在桌面应用中开启 MCP 管理；CLI 不会自动启动网关"));
            }
        }
    }
    let state = ctx.writer()?;
    let expected_backup_keep =
        (input.backup_keep.is_some() && !ctx.yes).then_some(before.backup_keep);
    let after =
        settings::update_settings_with_backup_snapshot(&state, patch, expected_backup_keep)?;
    let mcp_dir = skill_studio_service::mcp::dir(&state);
    if input.backup_keep.is_some() {
        skill_studio_service::mcp::prune_backups(&mcp_dir, after.backup_keep);
    }
    if input.manage_mcp == Some(false) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| CliError::failed(e.to_string()))?;
        let _ = runtime.block_on(skill_studio_mcp::gateway::request(
            &mcp_dir,
            "stop",
            Value::Null,
        ));
    } else if input.manage_mcp.is_some() {
        super::mcp::notify_gateway(&mcp_dir);
    }
    let after_json = json!(after);
    Ok(Report::write(
        "已保存设置",
        after_json != before_json,
        isolated,
        &[],
        json!({"settings":after_json}),
    ))
}
