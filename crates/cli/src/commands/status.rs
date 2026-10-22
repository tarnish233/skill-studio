use std::fmt::Write as _;

use serde_json::{json, Value};
use skill_studio_core::models::skill::SkillOrigin;

use super::mcp::Mcp;
use super::Ctx;
use crate::error::CliError;
use crate::report::{agent_name, agent_names, arg, home, list, Report};
use crate::view::{self, Data};

/// 需要检查、有待应用修改：给 Agent 的一份扁平清单，和给人看的几行字
pub struct Attention {
    pub json: Vec<Value>,
    pub issues: Vec<String>,
    pub pending: Vec<String>,
}

pub fn attention(data: &Data, mcp: Option<&Mcp>) -> Attention {
    let mut out = Attention {
        json: Vec::new(),
        issues: Vec::new(),
        pending: Vec::new(),
    };
    let mut push = |target: &str, name: &str, apply: String, states: Vec<view::AgentState>| {
        let label = match target {
            "group" => "分组",
            "project" => "项目",
            "mcpGroup" => "MCP 分组",
            _ => "项目的 MCP",
        };
        for state in states {
            let agent = agent_name(&state.agent);
            for issue in &state.issues {
                out.issues
                    .push(format!("{label}「{name}」在 {agent} 上：{issue}"));
                out.json.push(json!({
                    "kind": "issue", "target": target, "name": name,
                    "agent": state.agent, "detail": issue,
                }));
            }
            if !state.pending.is_empty() {
                let detail = list(&state.pending);
                out.pending.push(format!(
                    "{label}「{name}」在 {agent} 上：{detail} → {apply}"
                ));
                out.json.push(json!({
                    "kind": "pending", "target": target, "name": name,
                    "agent": state.agent, "detail": detail, "fix": apply,
                }));
            }
        }
    };
    for group in data.groups() {
        let apply = format!("skill-studio groups apply {}", arg(&group.name));
        push("group", &group.name, apply, view::group_states(data, group));
    }
    for project in &data.config.projects {
        let apply = format!("skill-studio projects apply {}", arg(&project.name));
        push(
            "project",
            &project.name,
            apply,
            view::project_states(data, project),
        );
    }
    let Some(mcp) = mcp else {
        return out;
    };
    for group in mcp.groups() {
        let apply = format!("skill-studio mcp groups apply {}", arg(&group.name));
        push("mcpGroup", &group.name, apply, mcp.group_states(group));
    }
    for project in &mcp.config.projects {
        if let Some(record) = mcp.catalog.projects.get(&project.id) {
            let apply = format!("skill-studio mcp projects apply {}", arg(&project.name));
            push(
                "mcpProject",
                &project.name,
                apply,
                mcp.project_states(&project.id, record),
            );
        }
    }
    out
}

/// MCP 读不出来的原因；管理关闭时为 None
pub fn load_mcp(
    state: &skill_studio_service::state::AppState,
    data: &Data,
) -> Option<Result<Mcp, CliError>> {
    data.config
        .settings
        .manage_mcp
        .then(|| super::mcp::load(state))
}

/// 总览里的 MCP 部分：Hub 有多少、MCP 分组与项目在哪些 Agent 上启用
fn mcp_section(text: &mut String, mcp: Option<&Result<Mcp, CliError>>) -> Value {
    let mcp = match mcp {
        None => {
            let _ = writeln!(
                text,
                "\nMCP：管理已关闭（在 Skill Studio 设置里开启后才能管理）"
            );
            return json!({ "enabled": false });
        }
        Some(Err(error)) => {
            let _ = writeln!(text, "\nMCP：读不出来（{}）", error.message);
            return json!({ "enabled": true, "error": error.message });
        }
        Some(Ok(mcp)) => mcp,
    };
    let hub = mcp.rows.iter().filter(|r| r.managed).count();
    let _ = writeln!(
        text,
        "\nMCP（Hub 里 {hub} 个 · 原有配置 {} 个）",
        mcp.rows.len() - hub
    );
    let groups = mcp.groups();
    if groups.is_empty() {
        let _ = writeln!(
            text,
            "  还没有 MCP 分组，用 skill-studio mcp groups create <名称> --mcp a,b 新建"
        );
    }
    let state = |agents: &[String]| {
        if agents.is_empty() {
            "未启用".to_string()
        } else {
            format!("已启用：{}", agent_names(agents))
        }
    };
    let mut group_json = Vec::new();
    for group in &groups {
        let enabled = mcp.group_agents(&group.id);
        let _ = writeln!(
            text,
            "  分组「{}」· {} 个 MCP · {}",
            group.name,
            group.entry_ids.len(),
            state(&enabled)
        );
        group_json.push(json!({
            "id": group.id, "name": group.name, "members": group.entry_ids.len(), "enabled": enabled,
        }));
    }
    let mut project_json = Vec::new();
    for project in &mcp.config.projects {
        let Some(record) = mcp.catalog.projects.get(&project.id) else {
            continue;
        };
        let members = mcp.project_members(record).len();
        let enabled = mcp.project_agents(record);
        if members == 0 && enabled.is_empty() {
            continue;
        }
        let _ = writeln!(
            text,
            "  项目「{}」· {members} 个 MCP · {}",
            project.name,
            state(&enabled)
        );
        project_json.push(json!({
            "id": project.id, "name": project.name, "members": members, "enabled": enabled,
        }));
    }
    json!({
        "enabled": true, "hub": hub, "original": mcp.rows.len() - hub,
        "groups": group_json, "projects": project_json,
    })
}

pub fn status(ctx: &Ctx) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let data = view::load(&state)?;
    let agents = skill_studio_service::agents::list_agents(&state, Some(true))?;
    let disabled = &data.config.settings.disabled_agents;
    let managed = |id: &String| !disabled.contains(id);

    let mut text = String::new();
    let _ = writeln!(
        text,
        "Skill Studio {} · 配置目录 {}",
        env!("CARGO_PKG_VERSION"),
        home(ctx.store.dir())
    );
    let names = |filter: &dyn Fn(&&skill_studio_core::models::agent::AgentInfo) -> bool| {
        agents
            .iter()
            .filter(filter)
            .map(|a| a.display_name.clone())
            .collect::<Vec<_>>()
    };
    let using = names(&|a| a.detected && managed(&a.id));
    let off = names(&|a| !managed(&a.id));
    let absent = names(&|a| !a.detected && managed(&a.id));
    let mut line = format!(
        "Agent：{}",
        if using.is_empty() {
            "无".to_string()
        } else {
            list(&using)
        }
    );
    if !off.is_empty() {
        let _ = write!(line, " · 已退出管理：{}", list(&off));
    }
    if !absent.is_empty() {
        let _ = write!(line, " · 未安装：{}", list(&absent));
    }
    let _ = writeln!(text, "{line}");
    let hub = data
        .skills
        .iter()
        .filter(|v| matches!(v.skill.origin, SkillOrigin::Hub))
        .count();
    let _ = writeln!(
        text,
        "Skill：{} 个，其中托管在 Skill Hub 的 {hub} 个",
        data.skills.len()
    );

    let groups = data.groups();
    let _ = writeln!(text, "\n分组（{}）", groups.len());
    if groups.is_empty() {
        let _ = writeln!(
            text,
            "  还没有分组，用 skill-studio groups create <名称> 新建"
        );
    }
    let mut group_json = Vec::new();
    for group in &groups {
        let enabled = view::group_agents(&data.config, &group.id);
        let missing: Vec<&String> = group
            .skill_ids
            .iter()
            .filter(|id| data.skill(id).is_none())
            .collect();
        let _ = writeln!(
            text,
            "  {} · {} 个 skill · {}{}",
            group.name,
            group.skill_ids.len(),
            if enabled.is_empty() {
                "未启用".to_string()
            } else {
                format!("已启用：{}", agent_names(&enabled))
            },
            if missing.is_empty() {
                String::new()
            } else {
                format!(" · 成员已缺失 {} 个", missing.len())
            }
        );
        group_json.push(json!({
            "id": group.id, "name": group.name, "members": group.skill_ids.len(),
            "missing": missing, "enabled": enabled,
        }));
    }

    let _ = writeln!(text, "\n项目（{}）", data.config.projects.len());
    if data.config.projects.is_empty() {
        let _ = writeln!(
            text,
            "  还没有项目，用 skill-studio projects create <目录> 添加"
        );
    }
    let mut project_json = Vec::new();
    for project in &data.config.projects {
        let enabled = view::project_agents(project);
        let members = view::project_members(&data.config, project).len();
        let _ = writeln!(
            text,
            "  {} · {} · {members} 个 skill · {}",
            project.name,
            home(&project.root),
            if enabled.is_empty() {
                "未启用".to_string()
            } else {
                format!("已启用：{}", agent_names(&enabled))
            }
        );
        project_json.push(json!({
            "id": project.id, "name": project.name, "root": project.root,
            "members": members, "enabled": enabled,
        }));
    }

    let mcp = load_mcp(&state, &data);
    let mcp_json = mcp_section(&mut text, mcp.as_ref());
    let attention = attention(&data, mcp.as_ref().and_then(|m| m.as_ref().ok()));
    if !attention.issues.is_empty() {
        let _ = writeln!(text, "\n需要检查（{}）", attention.issues.len());
        for line in &attention.issues {
            let _ = writeln!(text, "  {line}");
        }
    }
    if !attention.pending.is_empty() {
        let _ = writeln!(text, "\n有待应用修改（{}）", attention.pending.len());
        for line in &attention.pending {
            let _ = writeln!(text, "  {line}");
        }
    }

    let json = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "configDir": ctx.store.dir(),
        "agents": agents.iter().map(|a| json!({
            "id": a.id, "name": a.display_name, "installed": a.detected, "managed": managed(&a.id),
        })).collect::<Vec<_>>(),
        "skills": { "total": data.skills.len(), "hub": hub },
        "groups": group_json,
        "projects": project_json,
        "mcp": mcp_json,
        "attention": attention.json,
    });
    Ok(Report::new(text.trim_end(), json))
}

pub fn agents(ctx: &Ctx, probe: bool) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let agents = skill_studio_service::agents::list_agents(&state, Some(!probe))?;
    let disabled = state.config().settings.disabled_agents.clone();
    let mut text = String::new();
    let mut items = Vec::new();
    for agent in &agents {
        let managed = !disabled.contains(&agent.id);
        let mut parts = vec![
            format!("{}（{}）", agent.display_name, agent.id),
            if agent.detected {
                "已安装"
            } else {
                "未安装"
            }
            .to_string(),
            if managed {
                "管理中"
            } else {
                "已退出管理"
            }
            .to_string(),
        ];
        if probe {
            parts.push(
                match (&agent.cli_version, agent.cli_available, agent.cli_broken) {
                    (_, _, true) => "命令行无法运行".to_string(),
                    (Some(version), true, _) => format!("命令行 {version}"),
                    (None, true, _) => "命令行可用".to_string(),
                    _ => "没有找到命令行".to_string(),
                },
            );
        }
        let dirs: Vec<String> = agent.global_skill_dirs.iter().map(|d| home(d)).collect();
        if agent.detected {
            parts.push(list(&dirs));
        }
        let _ = writeln!(text, "{}", parts.join(" · "));
        items.push(json!({
            "id": agent.id, "name": agent.display_name, "installed": agent.detected,
            "managed": managed, "skillDirs": agent.global_skill_dirs,
            "projectSkillDir": agent.project_skill_dir,
            "cli": probe.then(|| json!({
                "available": agent.cli_available, "broken": agent.cli_broken,
                "version": agent.cli_version,
            })),
        }));
    }
    Ok(Report::new(text.trim_end(), Value::Array(items)))
}
