//! Skill 文件生命周期：所有移动、删除和恢复规则由服务层处理。
use super::Ctx;
use crate::{
    error::CliError,
    report::{home, Report},
    resolve, view,
};
use serde_json::json;
use skill_studio_core::{models::skill::SkillOrigin, services::studio::SkillView};
use skill_studio_service::skills;
use std::path::Path;

fn source(ctx: &Ctx, input: &str) -> Result<SkillView, CliError> {
    let state = ctx.reader()?;
    let data = view::load(&state)?;
    Ok(resolve::skill_source(&data.skills, input)?.clone())
}
fn protect(skill: &SkillView) -> Result<(), CliError> {
    if skill.source_ids.iter().any(|id| id == "builtin") {
        return Err(CliError::failed(
            "内置 skill 不能迁移或删除；请通过设置里的 CLI 与内置 skill 开关管理",
        ));
    }
    if matches!(skill.skill.origin, SkillOrigin::External) {
        return Err(CliError::failed("外部来源只读，不能迁移或删除源目录"));
    }
    Ok(())
}
pub fn adopt(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let source = source(ctx, input)?;
    protect(&source)?;
    if matches!(source.skill.origin, SkillOrigin::Hub) {
        return Ok(Report::write(
            "此 skill 已托管到 Hub",
            false,
            false,
            &[],
            json!({"skill":source.skill}),
        ));
    }
    ctx.confirm(format!(
        "托管 {} 会移动原件并留下 Agent 入口，需要 --yes 确认",
        home(&source.skill.source_path)
    ))?;
    let state = ctx.writer()?;
    let skill = skills::adopt_to_hub(&state, source.skill.id)?;
    Ok(Report::write(
        format!(
            "已托管「{}」到 Hub：{}；只处理所选来源，其他副本不动",
            skill.name,
            home(&skill.source_path)
        ),
        true,
        true,
        &[],
        json!({"skill":skill}),
    ))
}
pub fn release(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let source = source(ctx, input)?;
    protect(&source)?;
    ctx.confirm(format!(
        "移出 {} 并还原原件，需要 --yes 确认",
        home(&source.skill.source_path)
    ))?;
    let state = ctx.writer()?;
    let skill = skills::release_from_hub(&state, source.skill.id)?;
    Ok(Report::write(
        format!(
            "已移出 Hub 并还原「{}」到 {}",
            skill.name,
            home(&skill.source_path)
        ),
        true,
        true,
        &[],
        json!({"skill":skill}),
    ))
}
pub fn delete(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let source = source(ctx, input)?;
    protect(&source)?;
    let scope = match &source.skill.origin {
        SkillOrigin::Hub => "hub".into(),
        SkillOrigin::InPlace { owner_agent } => format!("agent:{owner_agent}"),
        SkillOrigin::External => unreachable!(),
    };
    ctx.confirm(format!(
        "删除 {}，非软链接保留备份，需要 --yes 确认",
        home(&source.skill.source_path)
    ))?;
    let state = ctx.writer()?;
    skill_studio_service::projects::delete_skill_file(
        &state,
        scope,
        source.skill.source_path.clone(),
    )?;
    Ok(Report::write(
        format!(
            "已删除「{}」的所选来源；非软链接文件已保留备份",
            source.skill.name
        ),
        true,
        true,
        &[],
        json!({"path":source.skill.source_path}),
    ))
}
pub fn prune(ctx: &Ctx) -> Result<Report, CliError> {
    ctx.confirm("清理已失效 skill 的分组与注册引用，需要 --yes 确认")?;
    let state = ctx.writer()?;
    let mut preview = state.config().clone();
    let removed = if state.studio().prune(&mut preview)? == 0 {
        0
    } else {
        skills::prune_missing(&state)?
    };
    Ok(Report::write(
        format!("已清理 {removed} 条失效引用"),
        removed > 0,
        false,
        &[],
        json!({"removed":removed}),
    ))
}
pub fn discover(_ctx: &Ctx, path: &Path) -> Result<Report, CliError> {
    let found =
        skills::discover_local_skills(resolve::absolute(path).to_string_lossy().into_owned())?;
    let mut text = format!("发现 {} 个 skill（只扫描，未导入）", found.len());
    for item in &found {
        text.push_str(&format!(
            "\n  {} · {}{}",
            item.name,
            home(&item.path),
            item.error
                .as_ref()
                .map(|e| format!(" · {e}"))
                .unwrap_or_default()
        ));
    }
    Ok(Report::new(text, json!(found)))
}
