//! MCP 分组：与 Skill 分组同样的用法——成员保存一次，每个 Agent 单独启用，多个分组叠加。

use std::fmt::Write as _;

use serde_json::{json, Value};
use skill_studio_mcp::management::groups::{Group, GroupSnapshot, Untouched};
use skill_studio_service::mcp::{self as service, ImportRef};
use skill_studio_service::state::AppState;

use super::{
    executable, import_ref, load, mcp_agent, notify_gateway, resolve, resolve_all, resolve_group,
    written, Mcp,
};
use crate::args::McpGroupsCommand;
use crate::commands::{retry, Ctx};
use crate::error::CliError;
use crate::report::{agent_name, agent_names, arg, list, Report};
use crate::resolve as names;
use crate::view;

pub fn run(command: &McpGroupsCommand, ctx: &Ctx) -> Result<Report, CliError> {
    match command {
        McpGroupsCommand::List => list_groups(ctx),
        McpGroupsCommand::Show { group } => show(ctx, group),
        McpGroupsCommand::Create { name, mcps } => create(ctx, name, mcps),
        McpGroupsCommand::Rename { group, name } => rename(ctx, group, name),
        McpGroupsCommand::Add { group, mcps } => members(ctx, group, mcps, true),
        McpGroupsCommand::Remove { group, mcps } => members(ctx, group, mcps, false),
        McpGroupsCommand::Delete { group } => delete(ctx, group),
        McpGroupsCommand::Enable { group, agents } => switch(ctx, group, &agents.agents, true),
        McpGroupsCommand::Disable { group, agents } => switch(ctx, group, &agents.agents, false),
        McpGroupsCommand::Apply { group, agents } => apply(ctx, group, agents),
    }
}

fn snapshot(group: &Group) -> GroupSnapshot {
    GroupSnapshot {
        name: group.name.clone(),
        entry_ids: group.entry_ids.clone(),
    }
}

fn member_names(mcp: &Mcp, group: &Group) -> Vec<String> {
    group
        .entry_ids
        .iter()
        .map(|id| mcp.member_name(id))
        .collect()
}

fn missing(mcp: &Mcp, group: &Group) -> Vec<String> {
    group
        .entry_ids
        .iter()
        .filter(|id| mcp.row(id).is_none())
        .cloned()
        .collect()
}

/// 绑定了这个分组的项目
fn projects(mcp: &Mcp, group: &Group) -> Vec<String> {
    mcp.config
        .projects
        .iter()
        .filter(|p| {
            mcp.catalog
                .projects
                .get(&p.id)
                .is_some_and(|r| r.group_ids.contains(&group.id))
        })
        .map(|p| p.name.clone())
        .collect()
}

fn list_groups(ctx: &Ctx) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let mcp = load(&state)?;
    let groups = mcp.groups();
    let mut text = format!("共 {} 个 MCP 分组", groups.len());
    if groups.is_empty() {
        text.push_str("。用 skill-studio mcp groups create <名称> --mcp a,b 新建");
    }
    let mut items = Vec::new();
    for group in groups {
        let enabled = mcp.group_agents(&group.id);
        let states = mcp.group_states(group);
        let pending = states.iter().any(|s| !s.pending.is_empty());
        let issues = states.iter().any(|s| !s.issues.is_empty());
        let missing = missing(&mcp, group);
        let mut parts = vec![
            group.name.clone(),
            format!("{} 个 MCP", group.entry_ids.len()),
            if enabled.is_empty() {
                "未启用".to_string()
            } else {
                format!("已启用：{}", agent_names(&enabled))
            },
        ];
        if pending {
            parts.push("有待应用修改".into());
        }
        if issues {
            parts.push("需要检查".into());
        }
        if !missing.is_empty() {
            parts.push(format!("成员已缺失 {} 个", missing.len()));
        }
        let _ = write!(text, "\n  {}", parts.join(" · "));
        items.push(json!({
            "id": group.id, "name": group.name, "members": member_names(&mcp, group),
            "missing": missing, "enabled": enabled, "pending": pending, "issues": issues,
        }));
    }
    Ok(Report::new(text, Value::Array(items)))
}

fn show(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let mcp = load(&state)?;
    let group = resolve_group(&mcp, input)?;
    let names = member_names(&mcp, group);
    let missing = missing(&mcp, group);
    let enabled = mcp.group_agents(&group.id);
    let states = mcp.group_states(group);
    let projects = projects(&mcp, group);
    let mut text = group.name.clone();
    let _ = write!(
        text,
        "\n成员（{}）：{}",
        names.len(),
        if names.is_empty() {
            "无".into()
        } else {
            list(&names)
        }
    );
    let _ = write!(
        text,
        "\n已启用：{}",
        if enabled.is_empty() {
            "无".into()
        } else {
            agent_names(&enabled)
        }
    );
    for state in &states {
        if !state.pending.is_empty() {
            let _ = write!(
                text,
                "\n有待应用修改：{}（{}）→ skill-studio mcp groups apply {} --agent {}",
                agent_name(&state.agent),
                list(&state.pending),
                arg(&group.name),
                state.agent
            );
        }
        for issue in &state.issues {
            let _ = write!(text, "\n需要检查：{} · {issue}", agent_name(&state.agent));
        }
    }
    if !projects.is_empty() {
        let _ = write!(text, "\n绑定的项目：{}", list(&projects));
    }
    let json = json!({
        "id": group.id, "name": group.name,
        "members": group.entry_ids.iter().map(|id| json!({"id": id, "name": mcp.name(id)})).collect::<Vec<_>>(),
        "missing": missing, "enabled": enabled,
        "states": states.iter().map(|s| json!({"agent": s.agent, "pending": s.pending, "issues": s.issues})).collect::<Vec<_>>(),
        "projects": projects,
    });
    Ok(Report::new(text, json))
}

fn create(ctx: &Ctx, name: &str, mcps: &[String]) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    let mcp = load(&state)?;
    let rows = resolve_all(&mcp, mcps)?;
    let imports: Vec<ImportRef> = rows
        .iter()
        .filter(|r| !r.managed)
        .map(|r| import_ref(r))
        .collect();
    let group = Group {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.trim().to_string(),
        entry_ids: rows.iter().map(|r| r.entry.id.clone()).collect(),
        references: vec![],
        sort_order: 0,
    };
    service::groups::save(&state, group.clone(), &imports, None)?;
    let next = if group.entry_ids.is_empty() {
        format!(
            "加入成员 skill-studio mcp groups add {} <MCP>",
            arg(&group.name)
        )
    } else {
        format!(
            "为 Agent 启用 skill-studio mcp groups enable {} --agent <Agent>",
            arg(&group.name)
        )
    };
    Ok(written(
        format!(
            "已新建 MCP 分组「{}」（{} 个 MCP）",
            group.name,
            group.entry_ids.len()
        ),
        true,
        false,
        &[],
        json!({ "group": { "id": group.id, "name": group.name } }),
    )
    .next(next))
}

fn rename(ctx: &Ctx, input: &str, name: &str) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    retry(|| {
        let mcp = load(&state)?;
        let group = resolve_group(&mcp, input)?;
        if group.name == name.trim() {
            return Ok(written(
                format!("分组已经叫「{}」", group.name),
                false,
                false,
                &[],
                json!({}),
            ));
        }
        let mut next = group.clone();
        next.name = name.trim().to_string();
        service::groups::save(&state, next.clone(), &[], Some(&snapshot(group)))?;
        Ok(written(
            format!("已把 MCP 分组「{}」改名为「{}」", group.name, next.name),
            true,
            false,
            &[],
            json!({ "group": { "id": group.id, "name": next.name } }),
        ))
    })
}

/// 按最新成员重新写入这些 Agent；遇到第一个失败就停下，已经写好的保持生效
fn reapply(state: &AppState, agents: &[String], done: &str) -> Result<Vec<Untouched>, CliError> {
    let exe = executable();
    let mut untouched = Vec::new();
    for agent in agents {
        let items =
            service::groups::reapply(state, exe.as_deref(), mcp_agent(agent)).map_err(|error| {
                let prefix = if done.is_empty() {
                    format!("写入 {} 时失败", agent_name(agent))
                } else {
                    format!("{done}，写入 {} 时失败", agent_name(agent))
                };
                CliError::from(error).context(prefix)
            })?;
        untouched.extend(items);
    }
    Ok(untouched)
}

/// 移出成员时先认分组里已有的成员：那个 MCP 可能已经不在 Hub 里了
fn member(mcp: &Mcp, ids: &[String], input: &str) -> Option<(String, String)> {
    let input = input.trim();
    if let Some(id) = ids.iter().find(|id| *id == input) {
        return Some((id.clone(), mcp.member_name(id)));
    }
    let wanted = input.to_lowercase();
    let mut matches = ids
        .iter()
        .filter(|id| mcp.name(id).is_some_and(|n| n.to_lowercase() == wanted));
    let first = matches.next()?;
    matches
        .next()
        .is_none()
        .then(|| (first.clone(), mcp.member_name(first)))
}

fn members(ctx: &Ctx, input: &str, mcps: &[String], add: bool) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    let (group, touched, skipped) = retry(|| {
        let mcp = load(&state)?;
        let group = resolve_group(&mcp, input)?.clone();
        let mut next = group.entry_ids.clone();
        let mut imports = Vec::new();
        let mut touched = Vec::new();
        let mut skipped = Vec::new();
        for input in mcps.iter().filter(|s| !s.trim().is_empty()) {
            let (id, name) = match member(&mcp, &group.entry_ids, input).filter(|_| !add) {
                Some(found) => found,
                None => {
                    let row = resolve(&mcp, input).map_err(|error| {
                        view::missing_member_hint(
                            error,
                            group
                                .entry_ids
                                .iter()
                                .filter(|id| mcp.row(id).is_none())
                                .cloned()
                                .collect(),
                            &format!("MCP 分组「{}」", group.name),
                            "mcp groups show",
                        )
                    })?;
                    if add && !row.managed {
                        imports.push(import_ref(row));
                    }
                    (row.entry.id.clone(), row.entry.name.clone())
                }
            };
            if add != next.contains(&id) {
                if add {
                    next.push(id);
                } else {
                    next.retain(|x| x != &id);
                }
                touched.push(name);
            } else {
                skipped.push(name);
            }
        }
        if !touched.is_empty() {
            let mut saved = group.clone();
            saved.entry_ids = next;
            service::groups::save(&state, saved, &imports, Some(&snapshot(&group))).map_err(
                |error| {
                    let error = CliError::from(error);
                    if error.message.contains("清空成员前请先为这些 Agent 停用") {
                        return error.hint(format!(
                            "skill-studio mcp groups disable {} --agent <Agent>，再移出成员",
                            arg(&group.name)
                        ));
                    }
                    error
                },
            )?;
        }
        Ok((group, touched, skipped))
    })?;
    let skipped_note = if skipped.is_empty() {
        String::new()
    } else if add {
        format!("（{} 原本就在分组里）", list(&skipped))
    } else {
        format!("（{} 本来就不在分组里）", list(&skipped))
    };
    if touched.is_empty() {
        // 分组没变，但部署可能还欠着（上次写入失败后重跑）
        let current = load(&state)?;
        let pending = current
            .group(&group.id)
            .map(|stored| {
                view::pending_note(
                    &current.group_states(stored),
                    &format!(
                        "skill-studio mcp groups apply {} --agent <Agent>",
                        arg(&stored.name)
                    ),
                )
            })
            .unwrap_or_default();
        return Ok(written(
            format!("MCP 分组「{}」没有变化{skipped_note}{pending}", group.name),
            false,
            false,
            &[],
            json!({ "group": group.name, "skipped": skipped, "pending": !pending.is_empty() }),
        ));
    }
    // 与界面一致：保存即写入正在使用这个分组的 Agent
    let agents = load(&state)?.group_agents(&group.id);
    let done = if add {
        format!("已把 {} 加入 MCP 分组「{}」", list(&touched), group.name)
    } else {
        format!("已从 MCP 分组「{}」移出 {}", group.name, list(&touched))
    };
    let untouched = reapply(&state, &agents, &done)?;
    notify_gateway(&service::dir(&state));
    let message = if agents.is_empty() {
        format!("{done}{skipped_note}；还没有 Agent 启用这个分组")
    } else {
        format!("{done}{skipped_note}，并写入 {}", agent_names(&agents))
    };
    Ok(written(
        message,
        true,
        !agents.is_empty(),
        &untouched,
        json!({ "group": group.name, "mcps": touched, "skipped": skipped, "written": agents }),
    ))
}

fn delete(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    let mcp = load(&state)?;
    let group = resolve_group(&mcp, input)?;
    let agents = mcp.group_agents(&group.id);
    if !agents.is_empty() {
        return Err(CliError::failed(format!(
            "MCP 分组「{}」正在 {} 上使用，请先停用",
            group.name,
            agent_names(&agents)
        ))
        .hint(format!(
            "skill-studio mcp groups disable {} --agent {}",
            arg(&group.name),
            agents.join(",")
        )));
    }
    let projects: Vec<String> = projects(&mcp, group)
        .iter()
        .map(|p| format!("「{p}」"))
        .collect();
    let effect = if projects.is_empty() {
        "只删除分组记录，不删除 MCP Hub 里的 MCP".to_string()
    } else {
        format!(
            "只删除分组记录，不删除 MCP Hub 里的 MCP；项目{}绑定了它，之后要在项目里移出",
            projects.concat()
        )
    };
    if !ctx.yes {
        return Err(CliError::confirm(format!(
            "删除 MCP 分组「{}」需要确认：加 --yes 再运行",
            group.name
        ))
        .hint(effect));
    }
    service::groups::remove(&state, &group.id)?;
    notify_gateway(&service::dir(&state));
    Ok(written(
        format!("已删除 MCP 分组「{}」。{effect}", group.name),
        true,
        false,
        &[],
        json!({ "group": group.name }),
    ))
}

fn switch(ctx: &Ctx, input: &str, agents: &[String], enable: bool) -> Result<Report, CliError> {
    let agents = names::agents(agents)?;
    let state = ctx.writer()?;
    let group = resolve_group(&load(&state)?, input)?.clone();
    let verb = if enable { "启用" } else { "停用" };
    let exe = executable();
    let mut changed: Vec<String> = Vec::new();
    let mut unchanged: Vec<String> = Vec::new();
    let mut untouched = Vec::new();
    for agent in agents {
        let mcp = load(&state)?;
        if enable
            && mcp
                .config
                .settings
                .disabled_agents
                .iter()
                .any(|a| a == agent)
        {
            return Err(CliError::failed(format!(
                "{} 已退出管理，请先在 Skill Studio 设置里开启它的管理",
                agent_name(agent)
            )));
        }
        if mcp.group_agents(&group.id).iter().any(|a| a == agent) == enable {
            unchanged.push(agent.to_string());
            continue;
        }
        let items =
            service::groups::set(&state, exe.as_deref(), mcp_agent(agent), &group.id, enable)
                .map_err(|error| {
                    let done = if changed.is_empty() {
                        String::new()
                    } else {
                        format!("已为 {} {verb}；", agent_names(&changed))
                    };
                    let error = CliError::from(error).context(format!(
                        "{done}为 {} {verb} MCP 分组「{}」时失败",
                        agent_name(agent),
                        group.name
                    ));
                    if error.message.contains("还没有成员") {
                        return error.hint(format!(
                            "skill-studio mcp groups add {} <MCP>",
                            arg(&group.name)
                        ));
                    }
                    error
                })?;
        untouched.extend(items);
        changed.push(agent.to_string());
    }
    if !changed.is_empty() {
        notify_gateway(&service::dir(&state));
    }
    let mut message = if changed.is_empty() {
        format!("MCP 分组「{}」没有变化", group.name)
    } else {
        format!(
            "已为 {} {verb} MCP 分组「{}」",
            agent_names(&changed),
            group.name
        )
    };
    if !unchanged.is_empty() {
        let _ = write!(message, "（{} 上原本就已{verb}）", agent_names(&unchanged));
    }
    Ok(written(
        message,
        !changed.is_empty(),
        true,
        &untouched,
        json!({ "group": group.name, "agents": changed, "unchanged": unchanged }),
    ))
}

fn apply(ctx: &Ctx, input: &str, only: &[String]) -> Result<Report, CliError> {
    let only = names::agents(only)?;
    let state = ctx.writer()?;
    let mcp = load(&state)?;
    let group = resolve_group(&mcp, input)?;
    let agents: Vec<String> = mcp
        .group_agents(&group.id)
        .into_iter()
        .filter(|a| only.is_empty() || only.contains(&a.as_str()))
        .collect();
    if agents.is_empty() {
        return Ok(written(
            format!(
                "MCP 分组「{}」还没有在这些 Agent 上启用，没有可应用的修改",
                group.name
            ),
            false,
            false,
            &[],
            json!({ "group": group.name, "written": [] }),
        ));
    }
    let untouched = reapply(&state, &agents, "")?;
    notify_gateway(&service::dir(&state));
    Ok(written(
        format!(
            "已按最新成员重新写入 {}（MCP 分组「{}」）",
            agent_names(&agents),
            group.name
        ),
        true,
        true,
        &untouched,
        json!({ "group": group.name, "written": agents }),
    ))
}
