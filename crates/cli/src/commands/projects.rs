use std::fmt::Write as _;
use std::path::Path;

use serde_json::{json, Value};
use skill_studio_core::models::group::UntouchedCopy;
use skill_studio_core::models::project::ProjectBinding;
use skill_studio_core::models::skill::{LinkMode, LinkReport};
use skill_studio_service::projects;
use skill_studio_service::state::ProjectMembers;

use super::{retry, Ctx};
use crate::args::ProjectsCommand;
use crate::error::CliError;
use crate::report::{agent_name, agent_names, arg, home, list, Report};
use crate::resolve;
use crate::view::{self, Data};

pub fn run(command: &ProjectsCommand, ctx: &Ctx) -> Result<Report, CliError> {
    match command {
        ProjectsCommand::List => list_projects(ctx),
        ProjectsCommand::Skills(command) => super::project_skills::run(command, ctx),
        ProjectsCommand::Set { project, link_mode } => set_mode(ctx, project, (*link_mode).into()),
        ProjectsCommand::Gitignore { project } => gitignore(ctx, project),
        ProjectsCommand::Show { project } => show(ctx, project),
        ProjectsCommand::Create { dir, name } => create(ctx, dir, name.as_deref()),
        ProjectsCommand::Add {
            project,
            skills,
            groups,
        } => members(ctx, project, skills, groups, true),
        ProjectsCommand::Remove {
            project,
            skills,
            groups,
        } => members(ctx, project, skills, groups, false),
        ProjectsCommand::Enable { project, agents } => switch(ctx, project, agents, true),
        ProjectsCommand::Disable { project, agents } => switch(ctx, project, agents, false),
        ProjectsCommand::Apply { project } => apply(ctx, project),
        ProjectsCommand::Delete { project } => delete(ctx, project),
    }
}

fn current(project: &ProjectBinding) -> ProjectMembers {
    ProjectMembers {
        skill_ids: project.skill_ids.clone(),
        group_ids: project.group_ids.clone(),
        link_mode: project.link_mode,
    }
}

/// 有写入失败的 skill 时整体报错：界面同样把它当作失败
fn checked(report: LinkReport) -> Result<LinkReport, CliError> {
    if report.failed.is_empty() {
        return Ok(report);
    }
    let names: Vec<String> = report.failed.iter().map(|f| f.skill_name.clone()).collect();
    let reasons: Vec<String> = report
        .failed
        .iter()
        .filter_map(|f| f.message.as_ref().map(|m| format!("{}：{m}", f.skill_name)))
        .collect();
    Err(CliError::failed(format!("部分 skill 写入失败：{}", list(&names))).candidates(reasons))
}

fn link_mode(mode: LinkMode) -> &'static str {
    match mode {
        LinkMode::Copy => "复制",
        LinkMode::Symlink => "软链",
        _ => "自动（优先软链）",
    }
}

fn names(data: &Data, project: &ProjectBinding) -> (Vec<String>, Vec<String>) {
    let skills = project
        .skill_ids
        .iter()
        .map(|id| {
            data.skill_name(id)
                .map(str::to_string)
                .unwrap_or_else(|| id.clone())
        })
        .collect();
    let groups = project
        .group_ids
        .iter()
        .map(|id| {
            data.group(id)
                .map(|g| g.name.clone())
                .unwrap_or_else(|| id.clone())
        })
        .collect();
    (skills, groups)
}

/// 移出成员时先认项目里已有的成员，理由同分组：那个成员可能已经不在 Hub 里
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

fn list_projects(ctx: &Ctx) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let data = view::load(&state)?;
    let mut text = format!("共 {} 个项目", data.config.projects.len());
    if data.config.projects.is_empty() {
        text.push_str("。用 skill-studio projects create <目录> 添加");
    }
    let mut items = Vec::new();
    for project in &data.config.projects {
        let enabled = view::project_agents(project);
        let states = view::project_states(&data, project);
        let pending = states.iter().any(|s| !s.pending.is_empty());
        let issues = states.iter().any(|s| !s.issues.is_empty());
        let missing = view::project_missing(&data, project);
        let members = view::project_members(&data.config, project).len();
        let mut parts = vec![
            project.name.clone(),
            home(&project.root),
            format!("{members} 个 skill"),
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
        let (skills, groups) = names(&data, project);
        items.push(json!({
            "id": project.id, "name": project.name, "root": project.root,
            "skills": skills, "groups": groups, "members": members,
            "enabled": enabled, "pending": pending, "issues": issues, "missing": missing,
        }));
    }
    Ok(Report::new(text, Value::Array(items)))
}

fn show(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let data = view::load(&state)?;
    let project = resolve::project(&data.config, input)?;
    let (skills, groups) = names(&data, project);
    let enabled = view::project_agents(project);
    let states = view::project_states(&data, project);
    let missing = view::project_missing(&data, project);
    let members = view::project_members(&data.config, project).len();

    let mut text = format!("{}（{}）", project.name, home(&project.root));
    let _ = write!(
        text,
        "\n单独选的 skill：{}",
        if skills.is_empty() {
            "无".into()
        } else {
            list(&skills)
        }
    );
    let _ = write!(
        text,
        "\n绑定的分组：{}",
        if groups.is_empty() {
            "无".into()
        } else {
            list(&groups)
        }
    );
    let _ = write!(
        text,
        "\n合计 {members} 个 skill · 写入方式：{}",
        link_mode(project.link_mode)
    );
    if !missing.is_empty() {
        let _ = write!(text, "\n已缺失：{}", list(&missing));
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
                "\n有待应用修改：{}（{}）→ skill-studio projects apply {}",
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
        "linkMode": project.link_mode,
        "skills": project.skill_ids.iter().map(|id| json!({"id": id, "name": data.skill_name(id)})).collect::<Vec<_>>(),
        "groups": project.group_ids.iter().map(|id| json!({"id": id, "name": data.group(id).map(|g| &g.name)})).collect::<Vec<_>>(),
        "members": members, "missing": missing, "enabled": enabled,
        "states": states.iter().map(|s| json!({"agent": s.agent, "pending": s.pending, "issues": s.issues})).collect::<Vec<_>>(),
    });
    Ok(Report::new(text, json))
}

fn set_mode(ctx: &Ctx, input: &str, mode: LinkMode) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    retry(|| {
        let data = view::load(&state)?;
        let project = resolve::project(&data.config, input)?;
        if project.link_mode == mode {
            return Ok(Report::write(
                "项目写入方式没有变化",
                false,
                false,
                &[],
                json!({}),
            ));
        }
        let mut members = current(project);
        members.link_mode = mode;
        let report = checked(projects::save_project(
            &state,
            project.id.clone(),
            members,
            Some(current(project)),
        )?)?;
        Ok(Report::write(
            format!(
                "已将项目「{}」的写入方式改为{}，并应用到已启用的 Agent",
                project.name,
                link_mode(mode)
            ),
            true,
            !view::project_agents(project).is_empty(),
            &report.untouched,
            json!({"project":project.id,"linkMode":mode}),
        ))
    })
}
fn gitignore(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    let project = resolve::project(&state.config(), input)?.clone();
    let changed = projects::write_project_gitignore(&state, project.id.clone())?;
    Ok(Report::write(
        if changed {
            "已写入 .gitignore，原有规则已保留"
        } else {
            ".gitignore 已包含这些规则，没有变化"
        },
        changed,
        false,
        &[],
        json!({"project":project.id}),
    ))
}

fn create(ctx: &Ctx, dir: &Path, name: Option<&str>) -> Result<Report, CliError> {
    let root = resolve::absolute(dir);
    // Windows 上 std 的 canonicalize 得到 \\?\C:\…；Agent 和桌面应用都用普通写法
    let root = dunce::canonicalize(&root)
        .map_err(|_| CliError::not_found(format!("目录不存在：{}", home(&root))))?;
    let name = name
        .map(str::to_string)
        .or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "项目".into());
    let state = ctx.writer()?;
    let project = projects::create_project(&state, name, root)?;
    Ok(Report::write(
        format!("已添加项目「{}」（{}）", project.name, home(&project.root)),
        true,
        false,
        &[],
        json!({ "project": { "id": project.id, "name": project.name, "root": project.root } }),
    )
    .next(format!(
        "选择成员 skill-studio projects add {} --skills a,b --groups <分组>",
        arg(&project.name)
    )))
}

fn members(
    ctx: &Ctx,
    input: &str,
    skills: &[String],
    groups: &[String],
    add: bool,
) -> Result<Report, CliError> {
    if skills.iter().chain(groups).all(|s| s.trim().is_empty()) {
        return Err(CliError::usage("请用 --skills 或 --groups 写明成员")
            .hint("例如 skill-studio projects add . --skills obsidian-markdown --groups 笔记"));
    }
    let state = ctx.writer()?;
    let (project, touched, skipped, report) = retry(|| {
        let data = view::load(&state)?;
        let project = resolve::project(&data.config, input)?;
        let mut next = current(project);
        let mut touched = Vec::new();
        let mut skipped = Vec::new();
        for skill in skills.iter().filter(|s| !s.trim().is_empty()) {
            let (id, name) = match member(&data, &project.skill_ids, skill).filter(|_| !add) {
                Some(found) => found,
                None => {
                    let view = resolve::skill(&data.skills, skill).map_err(|error| {
                        view::missing_member_hint(
                            error,
                            project
                                .skill_ids
                                .iter()
                                .filter(|id| data.skill(id).is_none())
                                .cloned()
                                .collect(),
                            &format!("项目「{}」", project.name),
                            "projects show",
                        )
                    })?;
                    (view.skill.id.clone(), view.skill.name.clone())
                }
            };
            if add != next.skill_ids.contains(&id) {
                if add {
                    next.skill_ids.push(id);
                } else {
                    next.skill_ids.retain(|x| x != &id);
                }
                touched.push(name);
            } else {
                skipped.push(name);
            }
        }
        for group in groups.iter().filter(|g| !g.trim().is_empty()) {
            let (id, name) = match project.group_ids.iter().find(|id| *id == group) {
                Some(id) if !add => (id.clone(), id.clone()),
                _ => {
                    let found = resolve::group(&data.config, group)?;
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
        let report = if touched.is_empty() {
            None
        } else {
            Some(checked(projects::save_project(
                &state,
                project.id.clone(),
                next,
                Some(current(project)),
            )?)?)
        };
        Ok((project.clone(), touched, skipped, report))
    })?;
    // 操作之后项目里已缺失的成员只剩 ID，只能按 ID 移出
    let data = view::load(&state)?;
    let missing: Vec<String> = data
        .config
        .project(&project.id)
        .map(|p| p.skill_ids.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|id| data.skill_name(id).is_none())
        .collect();
    let missing_note = if missing.is_empty() {
        String::new()
    } else {
        format!(
            "；项目里还有已缺失的成员 {}，只能按 ID 移出",
            list(&missing)
        )
    };
    let skipped_note = if skipped.is_empty() {
        String::new()
    } else if add {
        format!("（{} 原本就在项目里）", list(&skipped))
    } else {
        format!("（{} 本来就不在项目里）", list(&skipped))
    };
    let Some(report) = report else {
        // 成员没变，但部署可能还欠着（上次写入失败后重跑）
        let pending = data
            .config
            .project(&project.id)
            .map(|project| {
                view::pending_note(
                    &view::project_states(&data, project),
                    &format!("skill-studio projects apply {}", arg(&project.name)),
                )
            })
            .unwrap_or_default();
        return Ok(Report::write(
            format!(
                "项目「{}」没有变化{skipped_note}{missing_note}{pending}",
                project.name
            ),
            false,
            false,
            &[],
            json!({ "project": project.name, "skipped": skipped, "missing": missing, "pending": !pending.is_empty() }),
        ));
    };
    // 保存即写入项目已启用的 Agent（由后端按实际部署确定）
    let written = state
        .config()
        .project(&project.id)
        .map(view::project_agents)
        .unwrap_or_default();
    let done = if add {
        format!("已为项目「{}」加入 {}", project.name, list(&touched))
    } else {
        format!("已从项目「{}」移出 {}", project.name, list(&touched))
    };
    let message = if written.is_empty() {
        format!("{done}{skipped_note}{missing_note}；还没有 Agent 启用这个项目，只保存了成员")
    } else {
        format!(
            "{done}{skipped_note}{missing_note}，并写入 {}",
            agent_names(&written)
        )
    };
    Ok(Report::write(
        message,
        true,
        !written.is_empty(),
        &report.untouched,
        json!({ "project": project.name, "members": touched, "skipped": skipped, "missing": missing, "written": written }),
    ))
}

fn switch(ctx: &Ctx, input: &str, agents: &[String], enable: bool) -> Result<Report, CliError> {
    let agents = resolve::agents(agents)?;
    let state = ctx.writer()?;
    let project = resolve::project(&state.config(), input)?.clone();
    let verb = if enable { "启用" } else { "停用" };
    let mut changed: Vec<String> = Vec::new();
    let mut unchanged: Vec<String> = Vec::new();
    let mut untouched: Vec<UntouchedCopy> = Vec::new();
    for agent in agents {
        let config = state.config().clone();
        let latest = config
            .project(&project.id)
            .cloned()
            .unwrap_or_else(|| project.clone());
        if enable {
            if config.settings.disabled_agents.iter().any(|a| a == agent) {
                return Err(CliError::failed(format!(
                    "{} 已退出管理，请先在 Skill Studio 设置里开启它的管理",
                    agent_name(agent)
                )));
            }
            if view::project_members(&config, &latest).is_empty() {
                return Err(
                    CliError::failed(format!("项目「{}」还没有选择 skill", project.name)).hint(
                        format!(
                            "skill-studio projects add {} --skills a,b --groups <分组>",
                            arg(&project.name)
                        ),
                    ),
                );
            }
        }
        if view::project_agents(&latest).iter().any(|a| a == agent) == enable {
            unchanged.push(agent.to_string());
            continue;
        }
        let report =
            projects::set_project_agent(&state, project.id.clone(), agent.to_string(), enable)
                .map_err(|error| {
                    let done = if changed.is_empty() {
                        String::new()
                    } else {
                        format!("已为 {} {verb}；", agent_names(&changed))
                    };
                    CliError::from(error).context(format!(
                        "{done}为 {} {verb}项目「{}」时失败",
                        agent_name(agent),
                        project.name
                    ))
                })?;
        untouched.extend(checked(report)?.untouched);
        changed.push(agent.to_string());
    }
    // 项目没变，但部署可能还欠着（上次写入失败后重跑）：说清楚，别只回「没有变化」
    let pending = if changed.is_empty() {
        let data = view::load(&state)?;
        data.config
            .project(&project.id)
            .map(|stored| {
                view::pending_note(
                    &view::project_states(&data, stored),
                    &format!("skill-studio projects apply {}", arg(&project.name)),
                )
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let mut message = if changed.is_empty() {
        format!("项目「{}」没有变化{pending}", project.name)
    } else {
        format!(
            "已为 {} {verb}项目「{}」",
            agent_names(&changed),
            project.name
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
        json!({
            "project": project.name,
            "agents": changed,
            "unchanged": unchanged,
            "pending": !pending.is_empty(),
        }),
    ))
}

fn apply(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    let project = resolve::project(&state.config(), input)?.clone();
    let agents = view::project_agents(&project);
    if agents.is_empty() {
        return Ok(Report::write(
            format!(
                "项目「{}」还没有启用任何 Agent，没有可应用的修改",
                project.name
            ),
            false,
            false,
            &[],
            json!({ "project": project.name, "written": [] }),
        ));
    }
    let report = checked(projects::reapply_project(&state, project.id.clone())?)?;
    Ok(Report::write(
        format!(
            "已按最新成员重新写入 {}（项目「{}」）",
            agent_names(&agents),
            project.name
        ),
        true,
        true,
        &report.untouched,
        json!({ "project": project.name, "written": agents }),
    ))
}

fn delete(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    let project = resolve::project(&state.config(), input)?.clone();
    let agents = view::project_agents(&project);
    if !agents.is_empty() {
        return Err(CliError::failed(format!(
            "项目「{}」正在 {} 上使用，请先停用",
            project.name,
            agent_names(&agents)
        ))
        .hint(format!(
            "skill-studio projects disable {} --agent {}",
            arg(&project.name),
            agents.join(",")
        )));
    }
    // 项目的 MCP 还写在哪些 Agent 的配置里：项目没了就再也没有地方停用，先停用
    // （MCP 管理关闭时接入已还原，删除项目时一并清掉记录即可）
    let used_mcp = skill_studio_service::mcp::dir(&state)
        .join("catalog.json")
        .exists();
    let mcp = if used_mcp {
        skill_studio_service::mcp::catalog(&state)?
    } else {
        Default::default()
    };
    let mcp_agents: Vec<String> = match mcp.projects.get(&project.id) {
        Some(record) if !mcp.suspended => {
            let mut agents: Vec<String> = record
                .active
                .keys()
                .map(|a| skill_studio_service::mcp::app_agent(a).to_string())
                .collect();
            agents.sort_by_key(|id| {
                skill_studio_core::models::agent::AGENTS
                    .iter()
                    .position(|a| a.id == id)
            });
            agents
        }
        _ => Vec::new(),
    };
    if !mcp_agents.is_empty() {
        return Err(CliError::failed(format!(
            "项目「{}」的 MCP 还写在 {} 的配置里，请先停用",
            project.name,
            agent_names(&mcp_agents)
        ))
        .hint(format!(
            "skill-studio mcp projects disable {} --agent {}",
            arg(&project.name),
            mcp_agents.join(",")
        )));
    }
    if !ctx.yes {
        return Err(CliError::confirm(format!(
            "移除项目「{}」需要确认：加 --yes 再运行",
            project.name
        ))
        .hint("只删除项目记录（连同它的 MCP 成员选择），项目目录里的文件不动"));
    }
    projects::delete_project(&state, project.id.clone())?;
    Ok(Report::write(
        format!("已移除项目「{}」，项目目录里的文件没有改动", project.name),
        true,
        false,
        &[],
        json!({ "project": project.name }),
    ))
}
