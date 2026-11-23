use super::Ctx;
use crate::{args::PluginsCommand, error::CliError, report::Report, resolve};
use serde_json::json;
use skill_studio_service::plugins::{
    self, PluginAction, PluginInventory, PluginOperation, PluginView,
};

pub fn run(command: &PluginsCommand, ctx: &Ctx) -> Result<Report, CliError> {
    match command {
        PluginsCommand::List { agent, query } => {
            let state = ctx.reader()?;
            let mut inventory = plugins::scan_plugins(&state)?;
            let agent = agent
                .as_deref()
                .map(resolve::agent)
                .transpose()?
                .map(|agent| agent.id);
            inventory.plugins.retain(|plugin| {
                agent.is_none_or(|agent| plugin.agent_id == agent)
                    && query.as_ref().is_none_or(|query| {
                        format!("{} {}", plugin.package.name, plugin.source)
                            .to_lowercase()
                            .contains(&query.to_lowercase())
                    })
            });
            Ok(list_report(inventory))
        }
        PluginsCommand::Show { plugin } => {
            let state = ctx.reader()?;
            let inventory = plugins::scan_plugins(&state)?;
            let plugin = select(&inventory.plugins, plugin)?;
            Ok(Report::new(
                serde_json::to_string_pretty(plugin).unwrap(),
                json!(plugin),
            ))
        }
        PluginsCommand::Inspect { path, agent } => {
            let state = ctx.reader()?;
            let agent = agent
                .as_deref()
                .map(resolve::agent)
                .transpose()?
                .map(|agent| agent.id.to_owned());
            let package = plugins::inspect_plugin(&state, resolve::absolute(path), agent)?;
            let mut report = Report::new(
                serde_json::to_string_pretty(&package).unwrap(),
                json!(package),
            );
            if package.error.is_some() {
                report.exit = 1;
            }
            Ok(report)
        }
        PluginsCommand::Agents => {
            let state = ctx.reader()?;
            let capabilities = plugins::probe_plugin_cli(&state)?;
            Ok(Report::new(
                serde_json::to_string_pretty(&capabilities).unwrap(),
                json!({"managementEnabled": state.config().settings.manage_plugins, "agents":capabilities}),
            ))
        }
        PluginsCommand::Recover => {
            ctx.confirm("此操作只核对并完成插件管理记录，不修改 Agent，需要 --yes")?;
            let state = ctx.writer()?;
            let result = plugins::recover_plugin_write(&state, true)?;
            Ok(Report::new(result.message.clone(), json!(result)))
        }
        PluginsCommand::Install {
            source,
            agent,
            marketplace,
        } => {
            let agent = resolve::agent(agent)?.id.to_owned();
            ctx.confirm("原生插件安装可能执行本机代码并写入个人配置，需要 --yes 确认")?;
            let source = if (agent == "pi"
                && !source.starts_with("npm:")
                && !source.starts_with("git:")
                && !source.contains("://")
                && !source.starts_with("git@"))
                || (resolve::looks_like_path(source)
                    && (source.starts_with('.')
                        || source.starts_with('~')
                        || std::path::Path::new(source).is_absolute()))
            {
                resolve::absolute(source).to_string_lossy().into_owned()
            } else {
                source.clone()
            };
            operate(
                ctx,
                PluginOperation {
                    action: PluginAction::Install,
                    agent_id: agent,
                    plugin_id: None,
                    expected_revision: None,
                    source: Some(source),
                    marketplace: marketplace.clone(),
                    confirmed: true,
                },
            )
        }
        PluginsCommand::Enable { plugin }
        | PluginsCommand::Disable { plugin }
        | PluginsCommand::Update { plugin }
        | PluginsCommand::Uninstall { plugin }
        | PluginsCommand::Forget { plugin } => {
            let state = ctx.reader()?;
            let inventory = plugins::scan_plugins(&state)?;
            let selected = select(&inventory.plugins, plugin)?;
            let action = match command {
                PluginsCommand::Enable { .. } => PluginAction::Enable,
                PluginsCommand::Disable { .. } => PluginAction::Disable,
                PluginsCommand::Update { .. } => PluginAction::Update,
                PluginsCommand::Uninstall { .. } => PluginAction::Uninstall,
                _ => PluginAction::Forget,
            };
            if !selected.actions.contains(&action) {
                return Err(CliError::failed(
                    selected
                        .read_only_reason
                        .clone()
                        .unwrap_or_else(|| "此插件不支持该操作".into()),
                ));
            }
            ctx.confirm("此操作会改变插件个人配置或安装状态；原生工具可能删除选项、密钥或替换代码，快照不能恢复运行数据，需要 --yes")?;
            let input = PluginOperation {
                action,
                agent_id: selected.agent_id.clone(),
                plugin_id: Some(selected.id.clone()),
                expected_revision: Some(selected.revision.clone()),
                source: None,
                marketplace: None,
                confirmed: true,
            };
            drop(state);
            operate(ctx, input)
        }
    }
}
fn operate(ctx: &Ctx, input: PluginOperation) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    let result = plugins::plugin_operation_cli(&state, input, ctx.capture_signals)?;
    let mut report = Report::new(result.message.clone(), json!(result));
    if result.pending {
        report.exit = 1;
    }
    Ok(report)
}
fn list_report(inventory: PluginInventory) -> Report {
    let mut text = format!(
        "本机插件 · {} 条原生声明/停用记录\n",
        inventory.plugins.len()
    );
    for plugin in &inventory.plugins {
        text.push_str(&format!(
            "{} · {} · {} · {}\n  ID: {}\n",
            plugin.package.name,
            plugin.agent_id,
            plugin.scope,
            match plugin.enabled {
                Some(true) => "配置启用",
                Some(false) => "配置停用",
                None => "状态未确认",
            },
            plugin.id
        ));
    }
    for warning in &inventory.warnings {
        text.push_str(&format!("提示：{warning}\n"));
    }
    if inventory.pending {
        text.push_str("有一项插件配置写入待处理；查看不会自动恢复\n");
    }
    Report::new(text.trim_end(), json!(inventory))
}
fn select<'a>(plugins: &'a [PluginView], input: &str) -> Result<&'a PluginView, CliError> {
    if let Some(plugin) = plugins.iter().find(|plugin| plugin.id == input) {
        return Ok(plugin);
    }
    let matches: Vec<_> = plugins
        .iter()
        .filter(|plugin| {
            plugin.native_id == input || plugin.package.name.eq_ignore_ascii_case(input)
        })
        .collect();
    match matches.as_slice() {
        [plugin] => Ok(plugin),
        [] => {
            Err(CliError::not_found("找不到插件")
                .hint("运行 skill-studio plugins list，使用具体 ID"))
        }
        _ => Err(
            CliError::not_found("同名插件有多个来源或作用域，请使用具体 ID").candidates(
                matches
                    .iter()
                    .map(|plugin| {
                        format!(
                            "{} · {} · {} · {}",
                            plugin.id, plugin.package.name, plugin.agent_id, plugin.scope
                        )
                    })
                    .collect(),
            ),
        ),
    }
}
