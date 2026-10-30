//! 项目原件与分组/项目部署分开处理，路径必须来自这个项目的重新扫描结果。
use super::Ctx;
use crate::{
    args::ProjectSkillsCommand,
    error::CliError,
    report::{home, Report},
    resolve,
};
use serde_json::json;
use skill_studio_service::projects;

pub fn run(command: &ProjectSkillsCommand, ctx: &Ctx) -> Result<Report, CliError> {
    let project_input = match command {
        ProjectSkillsCommand::List { project }
        | ProjectSkillsCommand::Adopt { project, .. }
        | ProjectSkillsCommand::Enable { project, .. }
        | ProjectSkillsCommand::Disable { project, .. }
        | ProjectSkillsCommand::Delete { project, .. } => project,
    };
    let state = ctx.reader()?;
    let project = resolve::project(&state.config(), project_input)?.clone();
    let locals = projects::list_project_skills(&state, project.id.clone())?;
    if matches!(command, ProjectSkillsCommand::List { .. }) {
        let mut text = format!("项目「{}」已有 {} 个 skill", project.name, locals.len());
        for skill in &locals {
            text.push_str(&format!(
                "\n  {} · {}{}{}",
                skill.entry.name,
                home(&skill.entry.path),
                if skill.disabled { " · 已停用" } else { "" },
                if skill.managed { " · 已部署" } else { "" }
            ));
        }
        return Ok(Report::new(text, json!(locals)));
    }
    let input = match command {
        ProjectSkillsCommand::Adopt { skill, .. }
        | ProjectSkillsCommand::Enable { skill, .. }
        | ProjectSkillsCommand::Disable { skill, .. }
        | ProjectSkillsCommand::Delete { skill, .. } => skill,
        ProjectSkillsCommand::List { .. } => unreachable!(),
    };
    let matches = if resolve::looks_like_path(input) {
        let wanted = resolve::entry_path(&resolve::absolute(input));
        locals
            .iter()
            .filter(|s| resolve::entry_path(&s.entry.path) == wanted)
            .collect::<Vec<_>>()
    } else {
        locals
            .iter()
            .filter(|s| s.entry.name.eq_ignore_ascii_case(input.trim()))
            .collect::<Vec<_>>()
    };
    let skill = match matches.as_slice() {
        [skill] => *skill,
        _ => {
            return Err(CliError::not_found(
                "项目 skill 不存在或有多个同名来源，请指定列表中的原路径",
            )
            .candidates(
                locals
                    .iter()
                    .map(|s| format!("{} · {}", s.entry.name, home(&s.entry.path)))
                    .collect(),
            ))
        }
    };
    if skill.managed {
        return Err(CliError::failed(
            "此 skill 是 Studio 部署的副本，请通过项目或分组成员管理，不直接操作文件",
        ));
    }
    let path = skill.entry.path.clone();
    let name = skill.entry.name.clone();
    let (operation, changing) = match command {
        ProjectSkillsCommand::Enable { .. } => ("启用", skill.disabled),
        ProjectSkillsCommand::Disable { .. } => ("停用", !skill.disabled),
        ProjectSkillsCommand::Adopt { .. } => ("收录到 Hub", !skill.collected),
        ProjectSkillsCommand::Delete { .. } => {
            ctx.confirm(format!("删除项目原件 {}，需要 --yes 确认", home(&path)))?;
            ("删除", true)
        }
        ProjectSkillsCommand::List { .. } => unreachable!(),
    };
    if !changing {
        return Ok(Report::write(
            format!("「{name}」已{operation}，没有变化"),
            false,
            false,
            &[],
            json!({"path":path}),
        ));
    }
    let state = ctx.writer()?;
    match command {
        ProjectSkillsCommand::Enable { .. } => {
            projects::set_project_skill_enabled(&state, project.id, path.clone(), true)?
        }
        ProjectSkillsCommand::Disable { .. } => {
            projects::set_project_skill_enabled(&state, project.id, path.clone(), false)?
        }
        ProjectSkillsCommand::Adopt { .. } => {
            projects::collect_project_skill(&state, project.id, path.clone())?;
        }
        ProjectSkillsCommand::Delete { .. } => {
            projects::delete_project_local_skill(&state, project.id, path.clone())?
        }
        ProjectSkillsCommand::List { .. } => unreachable!(),
    }
    Ok(Report::write(
        format!(
            "已{operation}项目 skill「{name}」{}",
            if matches!(command, ProjectSkillsCommand::Adopt { .. }) {
                "，项目原文件保留"
            } else {
                ""
            }
        ),
        true,
        !matches!(command, ProjectSkillsCommand::Adopt { .. }),
        &[],
        json!({"path":path}),
    ))
}
