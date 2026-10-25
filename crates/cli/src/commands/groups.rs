use std::fmt::Write as _;

use serde_json::{json, Value};
use skill_studio_core::models::group::{Group, UntouchedCopy};
use skill_studio_service::groups::{self, GroupSnapshot};
use skill_studio_service::state::AppState;

use super::{retry, Ctx};
use crate::args::GroupsCommand;
use crate::error::CliError;
use crate::report::{agent_name, agent_names, arg, list, tokens, Report};
use crate::resolve;
use crate::view::{self, Data};

pub fn run(command: &GroupsCommand, ctx: &Ctx) -> Result<Report, CliError> {
    match command {
        GroupsCommand::List => list_groups(ctx),
        GroupsCommand::Show { group } => show(ctx, group),
        GroupsCommand::Create { name, skills } => create(ctx, name, skills),
        GroupsCommand::Rename { group, name } => rename(ctx, group, name),
        GroupsCommand::Add { group, skills } => members(ctx, group, skills, true),
        GroupsCommand::Remove { group, skills } => members(ctx, group, skills, false),
        GroupsCommand::Delete { group } => delete(ctx, group),
        GroupsCommand::Enable { group, agents } => switch(ctx, group, agents, true),
        GroupsCommand::Disable { group, agents } => switch(ctx, group, agents, false),
        GroupsCommand::Apply { group, agents } => apply(ctx, group, agents),
    }
}

fn snapshot(group: &Group) -> GroupSnapshot {
    GroupSnapshot {
        name: group.name.clone(),
        skill_ids: group.skill_ids.clone(),
    }
}

/// 移出成员时先认分组里已有的成员：那个成员可能已经不在 Hub 里，按名称重新解析会
/// 落到同名的新 skill 上，结果「本来就不在分组里」。同名多个时不猜，交给完整调用方处理
fn member(data: &Data, member_ids: &[String], input: &str) -> Option<(String, String)> {
    let input = input.trim();
    if let Some(id) = member_ids.iter().find(|id| *id == input) {
        return Some((id.clone(), data.skill_name(id).unwrap_or(id).to_string()));
    }
    if input.contains('/') || input.contains('\\') || input.starts_with('~') {
        return None;
    }
    let wanted = input.to_lowercase();
    let mut matches = member_ids.iter().filter(|id| {
        data.skill_name(id)
            .is_some_and(|n| n.to_lowercase() == wanted)
    });
    let first = matches.next()?;
    matches.next().is_none().then(|| {
        (
            first.clone(),
            data.skill_name(first).unwrap_or(first).to_string(),
        )
    })
}

fn member_names(data: &Data, group: &Group) -> Vec<String> {
    group
        .skill_ids
        .iter()
        .map(|id| {
            data.skill_name(id)
                .map(str::to_string)
                .unwrap_or_else(|| id.clone())
        })
        .collect()
}

fn list_groups(ctx: &Ctx) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let data = view::load(&state)?;
    let groups = data.groups();
    let mut text = format!("共 {} 个分组", groups.len());
    if groups.is_empty() {
        text.push_str("。用 skill-studio groups create <名称> --skills a,b 新建");
    }
    let mut items = Vec::new();
    for group in groups {
        let enabled = view::group_agents(&data.config, &group.id);
        let states = view::group_states(&data, group);
        let pending = states.iter().any(|s| !s.pending.is_empty());
        let issues = states.iter().any(|s| !s.issues.is_empty());
        let missing: Vec<&String> = group
            .skill_ids
            .iter()
            .filter(|id| data.skill(id).is_none())
            .collect();
        let mut parts = vec![
            group.name.clone(),
            format!("{} 个 skill", group.skill_ids.len()),
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
            "id": group.id, "name": group.name, "members": member_names(&data, group),
            "missing": missing, "enabled": enabled, "pending": pending, "issues": issues,
        }));
    }
    Ok(Report::new(text, Value::Array(items)))
}

fn show(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let data = view::load(&state)?;
    let group = resolve::group(&data.config, input)?;
    let names = member_names(&data, group);
    let missing: Vec<&String> = group
        .skill_ids
        .iter()
        .filter(|id| data.skill(id).is_none())
        .collect();
    let enabled = view::group_agents(&data.config, &group.id);
    let states = view::group_states(&data, group);
    let projects: Vec<String> = data
        .config
        .projects
        .iter()
        .filter(|p| p.group_ids.contains(&group.id))
        .map(|p| p.name.clone())
        .collect();
    let total: u64 = group
        .skill_ids
        .iter()
        .filter_map(|id| data.skill(id))
        .map(|v| (v.skill.tokens.skill_md + v.skill.tokens.extras) as u64)
        .sum();

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
    if !missing.is_empty() {
        let _ = write!(text, "\n已缺失：{}（已不在 Skill Hub 里）", list(&missing));
    }
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
                "\n有待应用修改：{}（{}）→ skill-studio groups apply {} --agent {}",
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
    if total > 0 {
        let _ = write!(text, "\n体量：{}", tokens(total));
    }
    let json = json!({
        "id": group.id, "name": group.name,
        "members": group.skill_ids.iter().map(|id| json!({"id": id, "name": data.skill_name(id)})).collect::<Vec<_>>(),
        "missing": missing, "enabled": enabled,
        "states": states.iter().map(|s| json!({"agent": s.agent, "pending": s.pending, "issues": s.issues})).collect::<Vec<_>>(),
        "projects": projects, "tokens": total,
    });
    Ok(Report::new(text, json))
}

fn create(ctx: &Ctx, name: &str, skills: &[String]) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    let data = view::load(&state)?;
    let ids = resolve::skills(&data.skills, skills)?;
    let group = groups::save_group(&state, None, name.to_string(), ids, None)?;
    Ok(Report::write(
        format!(
            "已新建分组「{}」（{} 个 skill）",
            group.name,
            group.skill_ids.len()
        ),
        true,
        false,
        &[],
        json!({ "group": { "id": group.id, "name": group.name } }),
    )
    .next(if group.skill_ids.is_empty() {
        format!(
            "加入成员 skill-studio groups add {} <skill>",
            arg(&group.name)
        )
    } else {
        format!(
            "为 Agent 启用 skill-studio groups enable {} --agent <Agent>",
            arg(&group.name)
        )
    }))
}

fn rename(ctx: &Ctx, input: &str, name: &str) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    retry(|| {
        let config = state.config().clone();
        let group = resolve::group(&config, input)?;
        if group.name == name.trim() {
            return Ok(Report::write(
                format!("分组已经叫「{}」", group.name),
                false,
                false,
                &[],
                json!({}),
            ));
        }
        let saved = groups::save_group(
            &state,
            Some(group.id.clone()),
            name.to_string(),
            group.skill_ids.clone(),
            Some(snapshot(group)),
        )?;
        Ok(Report::write(
            format!("已把分组「{}」改名为「{}」", group.name, saved.name),
            true,
            false,
            &[],
            json!({ "group": { "id": saved.id, "name": saved.name } }),
        ))
    })
}

/// 按最新成员重新写入这些 Agent；遇到第一个失败就停下，已经写好的保持生效
fn reapply(
    state: &AppState,
    agents: &[String],
    done: &str,
) -> Result<Vec<UntouchedCopy>, CliError> {
    let mut untouched = Vec::new();
    for agent in agents {
        let copies = groups::reapply_agent_groups(state, agent.clone()).map_err(|error| {
            let prefix = if done.is_empty() {
                format!("写入 {} 时失败", agent_name(agent))
            } else {
                format!("{done}，写入 {} 时失败", agent_name(agent))
            };
            CliError::from(error).context(prefix)
        })?;
        untouched.extend(copies);
    }
    Ok(untouched)
}

fn members(ctx: &Ctx, input: &str, skills: &[String], add: bool) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    let (group, touched, skipped) = retry(|| {
        let data = view::load(&state)?;
        let group = resolve::group(&data.config, input)?;
        let mut next = group.skill_ids.clone();
        let mut touched = Vec::new();
        let mut skipped = Vec::new();
        for skill in skills.iter().filter(|s| !s.trim().is_empty()) {
            let (id, name) = match member(&data, &group.skill_ids, skill).filter(|_| !add) {
                Some(found) => found,
                None => {
                    let view = resolve::skill(&data.skills, skill).map_err(|error| {
                        view::missing_member_hint(
                            error,
                            group
                                .skill_ids
                                .iter()
                                .filter(|id| data.skill(id).is_none())
                                .cloned()
                                .collect(),
                            &format!("分组「{}」", group.name),
                            "groups show",
                        )
                    })?;
                    (view.skill.id.clone(), view.skill.name.clone())
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
            groups::save_group(
                &state,
                Some(group.id.clone()),
                group.name.clone(),
                next,
                Some(snapshot(group)),
            )
            .map_err(|error| {
                let error = CliError::from(error);
                // 正在用的分组被清空：把下一步直接写进提示
                if error.message.contains("清空成员前请先为这些 Agent 停用") {
                    return error.hint(format!(
                        "skill-studio groups disable {} --agent <Agent>，再移出成员",
                        arg(&group.name)
                    ));
                }
                error
            })?;
        }
        Ok((group.clone(), touched, skipped))
    })?;
    // 操作之后分组里已缺失的成员只剩 ID：想移出必须按 ID 来，别让它悄悄留在这里
    let data = view::load(&state)?;
    let missing: Vec<String> = data
        .config
        .group(&group.id)
        .map(|g| g.skill_ids.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|id| data.skill_name(id).is_none())
        .collect();
    let missing_note = if missing.is_empty() {
        String::new()
    } else {
        format!(
            "；分组里还有已缺失的成员 {}，只能按 ID 移出",
            list(&missing)
        )
    };
    let skipped_note = if skipped.is_empty() {
        String::new()
    } else if add {
        format!("（{} 原本就在分组里）", list(&skipped))
    } else {
        format!("（{} 本来就不在分组里）", list(&skipped))
    };
    if touched.is_empty() {
        // 分组没变，但部署可能还欠着（上次写入失败后重跑）：说清楚，别只回「没有变化」
        let pending = data
            .config
            .group(&group.id)
            .map(|group| {
                view::pending_note(
                    &view::group_states(&data, group),
                    &format!(
                        "skill-studio groups apply {} --agent <Agent>",
                        arg(&group.name)
                    ),
                )
            })
            .unwrap_or_default();
        return Ok(Report::write(
            format!(
                "分组「{}」没有变化{skipped_note}{missing_note}{pending}",
                group.name
            ),
            false,
            false,
            &[],
            json!({ "group": group.name, "skipped": skipped, "missing": missing, "pending": !pending.is_empty() }),
        ));
    }
    // 与界面一致：保存即写入正在使用这个分组的 Agent
    let agents = view::group_agents(&state.config(), &group.id);
    let done = if add {
        format!("已把 {} 加入分组「{}」", list(&touched), group.name)
    } else {
        format!("已从分组「{}」移出 {}", group.name, list(&touched))
    };
    let untouched = reapply(&state, &agents, &done)?;
    let message = if agents.is_empty() {
        format!("{done}{skipped_note}{missing_note}；还没有 Agent 启用这个分组")
    } else {
        format!(
            "{done}{skipped_note}{missing_note}，并写入 {}",
            agent_names(&agents)
        )
    };
    Ok(Report::write(
        message,
        true,
        !agents.is_empty(),
        &untouched,
        json!({ "group": group.name, "skills": touched, "skipped": skipped, "missing": missing, "written": agents }),
    ))
}

fn delete(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    let config = state.config().clone();
    let group = resolve::group(&config, input)?;
    let agents = view::group_agents(&config, &group.id);
    if !agents.is_empty() {
        return Err(CliError::failed(format!(
            "分组「{}」正在 {} 上使用，请先停用",
            group.name,
            agent_names(&agents)
        ))
        .hint(format!(
            "skill-studio groups disable {} --agent {}",
            arg(&group.name),
            agents.join(",")
        )));
    }
    let projects: Vec<String> = config
        .projects
        .iter()
        .filter(|p| p.group_ids.contains(&group.id))
        .map(|p| format!("「{}」", p.name))
        .collect();
    let effect = if projects.is_empty() {
        "只删除分组记录，不删除 Skill Hub 里的 skill".to_string()
    } else {
        format!(
            "只删除分组记录，不删除 Skill Hub 里的 skill；项目{}会移除这个分组",
            projects.concat()
        )
    };
    if !ctx.yes {
        return Err(CliError::confirm(format!(
            "删除分组「{}」需要确认：加 --yes 再运行",
            group.name
        ))
        .hint(effect));
    }
    groups::delete_group(&state, group.id.clone())?;
    Ok(Report::write(
        format!("已删除分组「{}」。{effect}", group.name),
        true,
        false,
        &[],
        json!({ "group": group.name }),
    ))
}

fn switch(ctx: &Ctx, input: &str, agents: &[String], enable: bool) -> Result<Report, CliError> {
    let agents = resolve::agents(agents)?;
    let state = ctx.writer()?;
    let config = state.config().clone();
    let group = resolve::group(&config, input)?.clone();
    let verb = if enable { "启用" } else { "停用" };
    let mut changed: Vec<String> = Vec::new();
    let mut unchanged: Vec<String> = Vec::new();
    let mut untouched = Vec::new();
    for agent in agents {
        let config = state.config().clone();
        if enable && config.settings.disabled_agents.iter().any(|a| a == agent) {
            return Err(CliError::failed(format!(
                "{} 已退出管理，请先在 Skill Studio 设置里开启它的管理",
                agent_name(agent)
            )));
        }
        if view::group_agents(&config, &group.id)
            .iter()
            .any(|a| a == agent)
            == enable
        {
            unchanged.push(agent.to_string());
            continue;
        }
        let copies = groups::set_agent_group(&state, agent.to_string(), group.id.clone(), enable)
            .map_err(|error| {
            let done = if changed.is_empty() {
                String::new()
            } else {
                format!("已为 {} {verb}；", agent_names(&changed))
            };
            CliError::from(error).context(format!(
                "{done}为 {} {verb}分组「{}」时失败",
                agent_name(agent),
                group.name
            ))
        })?;
        untouched.extend(copies);
        changed.push(agent.to_string());
    }
    let mut message = if changed.is_empty() {
        format!("分组「{}」没有变化", group.name)
    } else {
        format!(
            "已为 {} {verb}分组「{}」",
            agent_names(&changed),
            group.name
        )
    };
    if !unchanged.is_empty() {
        let _ = write!(message, "（{} 上原本就已{verb}）", agent_names(&unchanged));
    }
    Ok(Report::write(
        message,
        !changed.is_empty(),
        true,
        &untouched,
        json!({ "group": group.name, "agents": changed, "unchanged": unchanged }),
    ))
}

fn apply(ctx: &Ctx, input: &str, only: &[String]) -> Result<Report, CliError> {
    let only = resolve::agents(only)?;
    let state = ctx.writer()?;
    let config = state.config().clone();
    let group = resolve::group(&config, input)?;
    let agents: Vec<String> = view::group_agents(&config, &group.id)
        .into_iter()
        .filter(|a| only.is_empty() || only.contains(&a.as_str()))
        .collect();
    if agents.is_empty() {
        return Ok(Report::write(
            format!(
                "分组「{}」还没有在这些 Agent 上启用，没有可应用的修改",
                group.name
            ),
            false,
            false,
            &[],
            json!({ "group": group.name, "written": [] }),
        ));
    }
    let untouched = reapply(&state, &agents, "")?;
    Ok(Report::write(
        format!(
            "已按最新成员重新写入 {}（分组「{}」）",
            agent_names(&agents),
            group.name
        ),
        true,
        true,
        &untouched,
        json!({ "group": group.name, "written": agents }),
    ))
}
