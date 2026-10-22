//! 项目里的 MCP：成员（MCP 与 MCP 分组）保存一次，每个 Agent 单独决定是否写进项目。
//! 项目本身用 `skill-studio projects create / delete` 添加和移除。

use std::fmt::Write as _;

use serde_json::{json, Value};
use skill_studio_core::models::project::ProjectBinding;
use skill_studio_mcp::management::projects::{Project, Snapshot};
use skill_studio_service::mcp::{self as service, projects::Members};

use super::{
    executable, import_ref, load, mcp_agent, notify_gateway, project_file, resolve, resolve_group,
    written, Mcp,
};
use crate::args::{ClaudeScope, McpProjectsCommand};
use crate::commands::{retry, Ctx};
use crate::error::CliError;
use crate::report::{agent_name, agent_names, arg, home, list, Report};
use crate::resolve as names;

pub fn run(command: &McpProjectsCommand, ctx: &Ctx) -> Result<Report, CliError> {
    match command {
        McpProjectsCommand::List => list_projects(ctx),
        McpProjectsCommand::Show { project } => show(ctx, project),
        McpProjectsCommand::Add {
            project,
            mcps,
            groups,
            claude,
        } => members(ctx, project, mcps, groups, *claude, true),
        McpProjectsCommand::Remove {
            project,
            mcps,
            groups,
        } => members(ctx, project, mcps, groups, None, false),
        McpProjectsCommand::Enable { project, agents } => {
            switch(ctx, project, &agents.agents, true)
        }
        McpProjectsCommand::Disable { project, agents } => {
            switch(ctx, project, &agents.agents, false)
        }
        McpProjectsCommand::Apply { project } => apply(ctx, project),
    }
}

/// 项目的 MCP 记录；还没选过成员的项目是空的
fn record(mcp: &Mcp, project: &ProjectBinding) -> Project {
    mcp.catalog
        .projects
        .get(&project.id)
        .cloned()
        .unwrap_or_default()
}

fn snapshot(record: &Project) -> Snapshot {
    Snapshot {
        entry_ids: record.entry_ids.clone(),
        group_ids: record.group_ids.clone(),
        claude_local: record.claude_local,
    }
}

fn names_of(mcp: &Mcp, record: &Project) -> (Vec<String>, Vec<String>) {
    let entries = record
        .entry_ids
        .iter()
        .map(|id| mcp.member_name(id))
        .collect();
    let groups = record
        .group_ids
        .iter()
        .map(|id| {
            mcp.group(id)
                .map(|g| g.name.clone())
                .unwrap_or_else(|| format!("已删除的分组 {id}"))
        })
        .collect();
    (entries, groups)
}

fn claude_place(local: bool) -> &'static str {
    if local {
        "~/.claude.json 里这个项目的配置（只给自己用）"
    } else {
        "项目里的 .mcp.json（可以随项目分享）"
    }
}

fn list_projects(ctx: &Ctx) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let mcp = load(&state)?;
    let mut text = format!("共 {} 个项目", mcp.config.projects.len());
    if mcp.config.projects.is_empty() {
        text.push_str("。用 skill-studio projects create <目录> 添加");
    }
    let mut items = Vec::new();
    for project in &mcp.config.projects {
        let record = record(&mcp, project);
        let enabled = mcp.project_agents(&record);
        let states = mcp.project_states(&project.id, &record);
        let pending = states.iter().any(|s| !s.pending.is_empty());
        let issues = states.iter().any(|s| !s.issues.is_empty());
        let missing = mcp.project_missing(&record);
        let members = mcp.project_members(&record).len();
        let mut parts = vec![
            project.name.clone(),
            home(&project.root),
            format!("{members} 个 MCP"),
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
        let (entries, groups) = names_of(&mcp, &record);
        items.push(json!({
            "id": project.id, "name": project.name, "root": project.root,
            "mcps": entries, "groups": groups, "members": members,
            "claudeLocal": record.claude_local,
            "enabled": enabled, "pending": pending, "issues": issues, "missing": missing,
        }));
    }
    Ok(Report::new(text, Value::Array(items)))
}

fn show(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let mcp = load(&state)?;
    let project = names::project(&mcp.config, input)?;
    let record = record(&mcp, project);
    let (entries, groups) = names_of(&mcp, &record);
    let enabled = mcp.project_agents(&record);
    let states = mcp.project_states(&project.id, &record);
    let missing = mcp.project_missing(&record);
    let members = mcp.project_members(&record).len();
    let mut text = format!("{}（{}）", project.name, home(&project.root));
    let none = |items: &[String]| {
        if items.is_empty() {
            "无".to_string()
        } else {
            list(items)
        }
    };
    let _ = write!(text, "\n单独选的 MCP：{}", none(&entries));
    let _ = write!(text, "\n绑定的 MCP 分组：{}", none(&groups));
    let _ = write!(text, "\n合计 {members} 个 MCP");
    let _ = write!(
        text,
        "\nClaude Code 写在：{}",
        claude_place(record.claude_local)
    );
    if !missing.is_empty() {
        let _ = write!(text, "\n已缺失：{}", list(&missing));
    }
    let placed: Vec<String> = enabled
        .iter()
        .map(|a| format!("{}（{}）", agent_name(a), project_file(a, &record)))
        .collect();
    let _ = write!(text, "\n已启用：{}", none(&placed));
    for state in &states {
        if !state.pending.is_empty() {
            let _ = write!(
                text,
                "\n有待应用修改：{}（{}）→ skill-studio mcp projects apply {}",
                agent_name(&state.agent),
                list(&state.pending),
                arg(&project.name)
            );
        }
        for issue in &state.issues {
            let _ = write!(text, "\n需要检查：{} · {issue}", agent_name(&state.agent));
        }
    }
    let json = json!({
        "id": project.id, "name": project.name, "root": project.root,
        "mcps": record.entry_ids.iter().map(|id| json!({"id": id, "name": mcp.name(id)})).collect::<Vec<_>>(),
        "groups": record.group_ids.iter().map(|id| json!({"id": id, "name": mcp.group(id).map(|g| &g.name)})).collect::<Vec<_>>(),
        "members": members, "claudeLocal": record.claude_local, "missing": missing,
        "enabled": enabled,
        "states": states.iter().map(|s| json!({"agent": s.agent, "pending": s.pending, "issues": s.issues})).collect::<Vec<_>>(),
    });
    Ok(Report::new(text, json))
}

fn members(
    ctx: &Ctx,
    input: &str,
    mcps: &[String],
    groups: &[String],
    claude: Option<ClaudeScope>,
    add: bool,
) -> Result<Report, CliError> {
    if mcps.iter().chain(groups).all(|s| s.trim().is_empty()) && claude.is_none() {
        return Err(CliError::usage("请用 --mcp 或 --groups 写明成员")
            .hint("例如 skill-studio mcp projects add . --mcp context7 --groups 常用"));
    }
    let state = ctx.writer()?;
    let (project, touched, skipped, untouched) = retry(|| {
        let mcp = load(&state)?;
        let project = names::project(&mcp.config, input)?.clone();
        let current = record(&mcp, &project);
        let mut next = snapshot(&current);
        let mut imports = Vec::new();
        let mut touched = Vec::new();
        let mut skipped = Vec::new();
        for input in mcps.iter().filter(|s| !s.trim().is_empty()) {
            let found = current
                .entry_ids
                .iter()
                .find(|id| *id == input.trim())
                .filter(|_| !add)
                .map(|id| (id.clone(), mcp.member_name(id)));
            let (id, name) = match found {
                Some(found) => found,
                None => {
                    let row = resolve(&mcp, input)?;
                    if add && !row.managed {
                        imports.push(import_ref(row));
                    }
                    (row.entry.id.clone(), row.entry.name.clone())
                }
            };
            if add != next.entry_ids.contains(&id) {
                if add {
                    next.entry_ids.push(id);
                } else {
                    next.entry_ids.retain(|x| x != &id);
                }
                touched.push(name);
            } else {
                skipped.push(name);
            }
        }
        for group in groups.iter().filter(|g| !g.trim().is_empty()) {
            let (id, name) = match current.group_ids.iter().find(|id| *id == group.trim()) {
                Some(id) if !add => (id.clone(), id.clone()),
                _ => {
                    let found = resolve_group(&mcp, group)?;
                    (found.id.clone(), found.name.clone())
                }
            };
            let label = format!("分组「{name}」");
            if add != next.group_ids.contains(&id) {
                if add {
                    next.group_ids.push(id);
                } else {
                    next.group_ids.retain(|x| x != &id);
                }
                touched.push(label);
            } else {
                skipped.push(label);
            }
        }
        if let Some(scope) = claude {
            let local = scope == ClaudeScope::Local;
            if local != next.claude_local {
                next.claude_local = local;
                touched.push(format!("Claude Code 改为写在{}", claude_place(local)));
            } else {
                skipped.push(format!("Claude Code 写在{}", claude_place(local)));
            }
        }
        let untouched = if touched.is_empty() {
            None
        } else {
            Some(service::projects::save(
                &state,
                executable().as_deref(),
                &project.id,
                Members {
                    entry_ids: next.entry_ids.clone(),
                    group_ids: next.group_ids.clone(),
                    claude_local: next.claude_local,
                },
                &imports,
                Some(&snapshot(&current)),
            )?)
        };
        Ok((project, touched, skipped, untouched))
    })?;
    let skipped_note = if skipped.is_empty() {
        String::new()
    } else if add {
        format!("（{} 原本就在项目里）", list(&skipped))
    } else {
        format!("（{} 本来就不在项目里）", list(&skipped))
    };
    let Some(untouched) = untouched else {
        return Ok(written(
            format!("项目「{}」的 MCP 没有变化{skipped_note}", project.name),
            false,
            false,
            &[],
            json!({ "project": project.name, "skipped": skipped }),
        ));
    };
    notify_gateway(&service::dir(&state));
    let mcp = load(&state)?;
    let written_to = mcp.project_agents(&record(&mcp, &project));
    let done = if add {
        format!("已为项目「{}」加入 {}", project.name, list(&touched))
    } else {
        format!("已从项目「{}」移出 {}", project.name, list(&touched))
    };
    let message = if written_to.is_empty() {
        format!("{done}{skipped_note}；还没有 Agent 启用，只保存了成员")
    } else {
        format!("{done}{skipped_note}，并写入 {}", agent_names(&written_to))
    };
    let report = written(
        message,
        true,
        !written_to.is_empty(),
        &untouched,
        json!({ "project": project.name, "members": touched, "skipped": skipped, "written": written_to }),
    );
    Ok(if written_to.is_empty() && add {
        report.next(format!(
            "为 Agent 启用 skill-studio mcp projects enable {} --agent <Agent>",
            arg(&project.name)
        ))
    } else {
        report
    })
}

fn switch(ctx: &Ctx, input: &str, agents: &[String], enable: bool) -> Result<Report, CliError> {
    let agents = names::agents(agents)?;
    let state = ctx.writer()?;
    let project = names::project(&state.config(), input)?.clone();
    let verb = if enable { "启用" } else { "停用" };
    let exe = executable();
    let mut changed: Vec<String> = Vec::new();
    let mut unchanged: Vec<String> = Vec::new();
    let mut untouched = Vec::new();
    for agent in agents {
        let mcp = load(&state)?;
        let current = record(&mcp, &project);
        if enable {
            if mcp
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
            if mcp.project_members(&current).is_empty() {
                return Err(
                    CliError::failed(format!("项目「{}」还没有选择 MCP", project.name)).hint(
                        format!(
                            "skill-studio mcp projects add {} --mcp a,b --groups <分组>",
                            arg(&project.name)
                        ),
                    ),
                );
            }
        }
        if mcp.project_agents(&current).iter().any(|a| a == agent) == enable {
            unchanged.push(agent.to_string());
            continue;
        }
        let items = service::projects::activate(
            &state,
            exe.as_deref(),
            &project.id,
            mcp_agent(agent),
            enable,
        )
        .map_err(|error| {
            let done = if changed.is_empty() {
                String::new()
            } else {
                format!("已为 {} {verb}；", agent_names(&changed))
            };
            CliError::from(error).context(format!(
                "{done}为 {} {verb}项目「{}」的 MCP 时失败",
                agent_name(agent),
                project.name
            ))
        })?;
        untouched.extend(items);
        changed.push(agent.to_string());
    }
    if !changed.is_empty() {
        notify_gateway(&service::dir(&state));
    }
    let mut message = if changed.is_empty() {
        format!("项目「{}」的 MCP 没有变化", project.name)
    } else {
        format!(
            "已为 {} {verb}项目「{}」的 MCP",
            agent_names(&changed),
            project.name
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
        json!({ "project": project.name, "agents": changed, "unchanged": unchanged }),
    ))
}

fn apply(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    let mcp = load(&state)?;
    let project = names::project(&mcp.config, input)?.clone();
    let agents = mcp.project_agents(&record(&mcp, &project));
    if agents.is_empty() {
        return Ok(written(
            format!(
                "项目「{}」的 MCP 还没有启用任何 Agent，没有可应用的修改",
                project.name
            ),
            false,
            false,
            &[],
            json!({ "project": project.name, "written": [] }),
        ));
    }
    let exe = executable();
    let mut untouched = Vec::new();
    for agent in &agents {
        let items = service::projects::activate(
            &state,
            exe.as_deref(),
            &project.id,
            mcp_agent(agent),
            true,
        )
        .map_err(|error| {
            CliError::from(error).context(format!("写入 {} 时失败", agent_name(agent)))
        })?;
        untouched.extend(items);
    }
    notify_gateway(&service::dir(&state));
    Ok(written(
        format!(
            "已按最新成员重新写入 {}（项目「{}」的 MCP）",
            agent_names(&agents),
            project.name
        ),
        true,
        true,
        &untouched,
        json!({ "project": project.name, "written": agents }),
    ))
}
