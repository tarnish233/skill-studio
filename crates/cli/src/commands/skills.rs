use std::fmt::Write as _;
use std::path::PathBuf;

use serde_json::{json, Value};
use skill_studio_core::models::agent::AGENTS;
use skill_studio_core::models::skill::SkillOrigin;
use skill_studio_core::services::studio::SkillView;
use skill_studio_service::skills::{self, CatalogInstallResult};

use super::Ctx;
use crate::args::SkillsCommand;
use crate::error::CliError;
use crate::report::{agent_name, agent_names, arg, home, list, tokens, Report};
use crate::resolve;
use crate::view::{self, Data};

pub fn run(command: &SkillsCommand, ctx: &Ctx) -> Result<Report, CliError> {
    match command {
        SkillsCommand::List {
            agent,
            group,
            hub,
            query,
        } => list_skills(
            ctx,
            agent.as_deref(),
            group.as_deref(),
            *hub,
            query.as_deref(),
        ),
        SkillsCommand::Show { skill, content } => show(ctx, skill, *content),
        SkillsCommand::Search { query } => search(query),
        SkillsCommand::Install {
            source,
            skill,
            paths,
        } => install(ctx, source, skill.as_deref(), paths),
        SkillsCommand::Import { path } => import(ctx, path),
        SkillsCommand::Adopt { skill } => super::skill_files::adopt(ctx, skill),
        SkillsCommand::Release { skill } => super::skill_files::release(ctx, skill),
        SkillsCommand::Delete { skill } => super::skill_files::delete(ctx, skill),
        SkillsCommand::Prune => super::skill_files::prune(ctx),
        SkillsCommand::Discover { path } => super::skill_files::discover(ctx, path),
    }
}

/// 来源：Hub 托管、某个 Agent 目录里的原件、外部来源
fn origin(view: &SkillView) -> (&'static str, Option<String>, String) {
    if view.source_ids.iter().any(|id| id == "builtin") {
        return ("builtin", None, "Skill Studio 内置".into());
    }
    match &view.skill.origin {
        SkillOrigin::Hub => ("hub", None, "Hub 托管".into()),
        SkillOrigin::InPlace { owner_agent } => (
            "inPlace",
            Some(owner_agent.clone()),
            format!("{} 目录里的原件", agent_name(owner_agent)),
        ),
        SkillOrigin::External => ("external", None, "外部来源".into()),
    }
}

/// 在哪些 Agent 上已启用（能被读到）
fn enabled_on(view: &SkillView) -> Vec<String> {
    AGENTS
        .iter()
        .filter(|a| view.agents.get(a.id).is_some_and(view::loaded))
        .map(|a| a.id.to_string())
        .collect()
}

fn group_names(data: &Data, view: &SkillView) -> Vec<String> {
    data.groups()
        .into_iter()
        .filter(|g| g.skill_ids.contains(&view.skill.id))
        .map(|g| g.name.clone())
        .collect()
}

fn total_tokens(view: &SkillView) -> u64 {
    (view.skill.tokens.skill_md + view.skill.tokens.extras) as u64
}

fn list_skills(
    ctx: &Ctx,
    agent: Option<&str>,
    group: Option<&str>,
    hub: bool,
    query: Option<&str>,
) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let data = view::load(&state)?;
    let agent = agent.map(resolve::agent).transpose()?;
    let group = group
        .map(|g| resolve::group(&data.config, g).map(|g| g.skill_ids.clone()))
        .transpose()?;
    let query = query.map(str::to_lowercase);
    let mut views: Vec<&SkillView> = data
        .skills
        .iter()
        .filter(|v| !hub || matches!(v.skill.origin, SkillOrigin::Hub))
        .filter(|v| agent.is_none_or(|a| v.agents.get(a.id).is_some_and(view::loaded)))
        .filter(|v| group.as_ref().is_none_or(|ids| ids.contains(&v.skill.id)))
        .filter(|v| {
            query.as_ref().is_none_or(|q| {
                v.skill.name.to_lowercase().contains(q)
                    || v.skill
                        .display_name
                        .as_deref()
                        .is_some_and(|n| n.to_lowercase().contains(q))
                    || v.skill
                        .description
                        .as_deref()
                        .is_some_and(|d| d.to_lowercase().contains(q))
            })
        })
        .collect();
    views.sort_by(|a, b| {
        a.skill
            .name
            .cmp(&b.skill.name)
            .then(a.skill.source_path.cmp(&b.skill.source_path))
    });

    let mut text = format!("共 {} 个 skill", views.len());
    if views.is_empty() {
        text.push_str(if data.skills.is_empty() {
            "。Skill Hub 还是空的：用 skill-studio skills search <词> 搜索，skills install 安装"
        } else {
            "。没有符合条件的"
        });
    }
    let mut items = Vec::new();
    for view in views {
        let (kind, owner, label) = origin(view);
        let groups = group_names(&data, view);
        let enabled = enabled_on(view);
        let mut parts = vec![view.skill.name.clone(), label];
        if !groups.is_empty() {
            parts.push(format!("分组：{}", list(&groups)));
        }
        if !enabled.is_empty() {
            parts.push(format!("已启用：{}", agent_names(&enabled)));
        }
        if total_tokens(view) > 0 {
            parts.push(tokens(total_tokens(view)));
        }
        let _ = write!(text, "\n  {}", parts.join(" · "));
        items.push(json!({
            "id": view.skill.id, "name": view.skill.name,
            "description": view.skill.description, "origin": kind, "owner": owner,
            "path": view.skill.source_path, "tokens": total_tokens(view),
            "groups": groups, "enabledOn": enabled,
        }));
    }
    Ok(Report::new(text, Value::Array(items)))
}

fn show(ctx: &Ctx, input: &str, content: bool) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let data = view::load(&state)?;
    let view = resolve::skill(&data.skills, input)?;
    let (kind, owner, label) = origin(view);
    let groups = group_names(&data, view);
    let projects: Vec<String> = data
        .config
        .projects
        .iter()
        .filter(|p| view::project_members(&data.config, p).contains(&view.skill.id))
        .map(|p| p.name.clone())
        .collect();
    let mut text = format!("{}（{label}）", view.skill.name);
    if let Some(description) = &view.skill.description {
        let _ = write!(text, "\n说明：{description}");
    }
    let _ = write!(text, "\n位置：{}", home(&view.skill.source_path));
    let _ = write!(
        text,
        "\n所在分组：{}",
        if groups.is_empty() {
            "无".into()
        } else {
            list(&groups)
        }
    );
    if !projects.is_empty() {
        let _ = write!(text, "\n所在项目：{}", list(&projects));
    }
    let mut states = serde_json::Map::new();
    let mut lines = Vec::new();
    for agent in AGENTS {
        let Some(state) = view.agents.get(agent.id) else {
            continue;
        };
        // 没有部署过的 Agent 一句带过，只逐个列出有状态的
        if state.status != skill_studio_core::models::skill::LinkStatus::NotLinked || state.disabled
        {
            let mut line = format!(
                "{}：{}",
                agent.display_name,
                view::status_label(state.status)
            );
            if state.disabled {
                line.push_str("（已在 Agent 里停用）");
            }
            if let Some(reason) = &state.unavailable_reason {
                let _ = write!(line, "（{reason}）");
            }
            lines.push(line);
        }
        states.insert(
            agent.id.into(),
            json!({
                "status": state.status, "disabled": state.disabled,
                "loaded": view::loaded(state), "path": state.target_path,
                "unavailable": state.unavailable_reason,
            }),
        );
    }
    let _ = write!(
        text,
        "\n各 Agent：{}",
        if lines.is_empty() {
            "都还没有启用".to_string()
        } else {
            format!("{}，其余未启用", lines.join(" · "))
        }
    );
    if total_tokens(view) > 0 {
        let _ = write!(
            text,
            "\n体量：{}（SKILL.md {}，附带文件 {}）",
            tokens(total_tokens(view)),
            view.skill.tokens.skill_md,
            view.skill.tokens.extras
        );
    }
    let document = content
        .then(|| skills::read_skill_document(&state, view.skill.source_path.clone()))
        .transpose()?;
    if let Some(document) = &document {
        let _ = write!(text, "\n\n{}", document.raw.trim_start_matches('\u{feff}'));
    }
    let json = json!({
        "id": view.skill.id, "name": view.skill.name, "displayName": view.skill.display_name,
        "description": view.skill.description, "origin": kind, "owner": owner,
        "path": view.skill.source_path, "tokens": view.skill.tokens,
        "groups": groups, "projects": projects, "agents": states,
        "content": document.map(|d| d.raw),
    });
    Ok(Report::new(text, json))
}

fn search(query: &str) -> Result<Report, CliError> {
    let found = skills::search_catalog_skills(query.to_string())?;
    let mut text = format!("skills.sh 上找到 {} 个", found.len());
    let mut items = Vec::new();
    for skill in &found {
        let install = format!(
            "skill-studio skills install {}@{}",
            skill.source, skill.skill_id
        );
        let _ = write!(
            text,
            "\n  {} · {} · {} 次安装\n    安装：{install}",
            skill.name, skill.source, skill.installs
        );
        items.push(json!({
            "name": skill.name, "source": skill.source, "skillId": skill.skill_id,
            "installs": skill.installs, "install": install,
        }));
    }
    Ok(Report::new(text, Value::Array(items)))
}

fn install(
    ctx: &Ctx,
    source: &str,
    skill: Option<&str>,
    paths: &[String],
) -> Result<Report, CliError> {
    let (source, skill) = split_source(source, skill).ok_or_else(|| {
        CliError::usage("请写明仓库里的哪个 skill").hint(
            "写成 owner/repo@skill，或加 --skill <名称>；可先用 skill-studio skills search 搜索",
        )
    })?;
    let state = ctx.writer()?;
    match skills::install_catalog_skill(&state, source.clone(), skill.clone(), None, Some(paths.to_vec()))? {
        CatalogInstallResult::Installed { skill } => Ok(Report::write(
            format!("已把「{}」安装到 Skill Hub，还没有启用到任何 Agent", skill.name),
            true,
            false,
            &[],
            json!({ "skill": { "id": skill.id, "name": skill.name, "path": skill.source_path } }),
        )
        .next(format!(
            "加入分组 skill-studio groups add <分组> {}，或加入项目 skill-studio projects add <项目> --skills {}",
            arg(&skill.name),
            arg(&skill.name)
        ))),
        CatalogInstallResult::SelectionRequired { candidates } => Err(CliError::usage(format!(
            "{source} 里的「{skill}」有多个版本，请用 --path 选择要安装的"
        ))
        .candidates(
            candidates
                .iter()
                .map(|c| match (&c.variant_key, &c.description, &c.error) {
                    (_, _, Some(error)) => format!("{} · 不能安装：{error}", c.repository_path),
                    (Some(key), _, _) => format!("{} · {key}", c.repository_path),
                    (_, Some(description), _) => format!("{} · {description}", c.repository_path),
                    _ => c.repository_path.clone(),
                })
                .collect(),
        )
        .hint("可以重复 --path 一次装多个版本")),
    }
}

/// `owner/repo@skill`，或 `owner/repo --skill skill`。两处都写了以 --skill 为准，
/// 并把源里多余的 `@skill` 去掉，别把它当成仓库地址的一部分
fn split_source(source: &str, skill: Option<&str>) -> Option<(String, String)> {
    match (source.rsplit_once('@'), skill) {
        (_, Some(skill)) if !skill.trim().is_empty() => {
            let repo = source
                .rsplit_once('@')
                .filter(|(_, tail)| !tail.contains('/'))
                .map(|(repo, _)| repo.to_string())
                .unwrap_or_else(|| source.to_string());
            Some((repo, skill.to_string()))
        }
        (Some((repo, skill)), None) if !repo.is_empty() && !skill.is_empty() => {
            Some((repo.to_string(), skill.to_string()))
        }
        _ => None,
    }
}

fn import(ctx: &Ctx, path: &std::path::Path) -> Result<Report, CliError> {
    let path: PathBuf = resolve::absolute(path);
    if !path.exists() {
        return Err(CliError::not_found(format!("目录不存在：{}", home(&path))));
    }
    let found = skills::discover_local_skills(path.to_string_lossy().into_owned())?;
    let chosen = match found.len() {
        0 => {
            return Err(CliError::failed(format!(
                "{} 里没有找到 SKILL.md",
                home(&path)
            )))
        }
        1 => &found[0],
        _ => {
            let own = path.canonicalize().ok();
            match found.iter().find(|s| Some(&s.path) == own.as_ref()) {
                Some(skill) => skill,
                None => {
                    return Err(CliError::usage(format!(
                        "{} 下有 {} 个 skill，请指定其中一个目录",
                        home(&path),
                        found.len()
                    ))
                    .candidates(
                        found
                            .iter()
                            .map(|s| format!("{} · {}", s.name, home(&s.path)))
                            .collect(),
                    ))
                }
            }
        }
    };
    if let Some(error) = &chosen.error {
        return Err(CliError::failed(format!(
            "「{}」不能导入：{error}",
            chosen.name
        )));
    }
    let state = ctx.writer()?;
    let imported = skills::import_local_skill(&state, chosen.path.to_string_lossy().into_owned())?;
    let changed = imported.changed;
    let skill = imported.skill;
    let message = if changed {
        format!(
            "已把「{}」导入 Skill Hub，还没有启用到任何 Agent",
            skill.name
        )
    } else {
        format!("「{}」已在 Skill Hub 里，内容没有变化", skill.name)
    };
    Ok(Report::write(
        message,
        changed,
        false,
        &[],
        json!({ "skill": { "id": skill.id, "name": skill.name, "path": skill.source_path } }),
    )
    .next(format!(
        "加入分组 skill-studio groups add <分组> {}",
        arg(&skill.name)
    )))
}

#[cfg(test)]
mod tests {
    use super::split_source;

    #[test]
    fn the_skill_name_comes_from_either_the_at_sign_or_the_flag_but_never_both() {
        let pair = |a, b| split_source(a, b);
        assert_eq!(
            pair("owner/repo@skill", None),
            Some(("owner/repo".into(), "skill".into()))
        );
        assert_eq!(
            pair("owner/repo", Some("skill")),
            Some(("owner/repo".into(), "skill".into()))
        );
        // 两处都写时以 --skill 为准，源里多写的 @skill 只去掉
        assert_eq!(
            pair("owner/repo@old", Some("new")),
            Some(("owner/repo".into(), "new".into()))
        );
        assert_eq!(pair("@skill", None), None);
        assert_eq!(pair("owner/repo", None), None);
        assert_eq!(pair("owner/repo", Some("  ")), None);
    }
}
