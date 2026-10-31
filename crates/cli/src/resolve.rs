//! 用名称找东西：skill ID 是路径哈希、分组和项目是 UUID，对人和 Agent 都没有意义。
//! 找不到或对应多个时报错并列出候选，不猜。

use std::path::{Path, PathBuf};

use skill_studio_core::fs::paths;
use skill_studio_core::models::agent::{AgentDescriptor, AGENTS};
use skill_studio_core::models::config::AppConfig;
use skill_studio_core::models::group::Group;
use skill_studio_core::models::project::ProjectBinding;
use skill_studio_core::models::skill::SkillOrigin;
use skill_studio_core::services::studio::SkillView;

use crate::error::CliError;
use crate::report::home;

const AGENT_HINT: &str =
    "Agent 写 claude-code（或 claude）、codex、opencode、pi、grok、deepseek-harness（或 dsh）";

pub fn agent(input: &str) -> Result<&'static AgentDescriptor, CliError> {
    let key: String = input
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .collect();
    let key = match key.as_str() {
        "claude" => "claudecode".to_string(),
        "dsh" => "deepseekharness".to_string(),
        _ => key,
    };
    let squash = |s: &str| -> String {
        s.to_lowercase()
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
            .collect()
    };
    AGENTS
        .iter()
        .find(|a| squash(a.id) == key || squash(a.display_name) == key)
        .ok_or_else(|| {
            CliError::not_found(format!("没有这个 Agent：{input}"))
                .candidates(AGENTS.iter().map(|a| a.id.to_string()).collect())
                .hint(AGENT_HINT)
        })
}

/// 去重，保持输入顺序
pub fn agents(inputs: &[String]) -> Result<Vec<&'static str>, CliError> {
    let mut ids = Vec::new();
    for input in inputs {
        let id = agent(input)?.id;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

pub fn group<'a>(config: &'a AppConfig, input: &str) -> Result<&'a Group, CliError> {
    let input = input.trim();
    let found = config
        .groups
        .iter()
        .find(|g| g.id == input || g.name == input)
        .or_else(|| {
            let lower = input.to_lowercase();
            let matches: Vec<_> = config
                .groups
                .iter()
                .filter(|g| g.name.to_lowercase() == lower)
                .collect();
            (matches.len() == 1).then(|| matches[0])
        });
    found.ok_or_else(|| {
        let mut groups: Vec<_> = config.groups.iter().collect();
        groups.sort_by_key(|g| g.sort_order);
        CliError::not_found(format!("找不到分组「{input}」"))
            .candidates(groups.iter().map(|g| g.name.clone()).collect())
            .hint(if config.groups.is_empty() {
                "还没有分组，用 skill-studio groups create <名称> 新建"
            } else {
                "用 skill-studio groups list 查看全部分组"
            })
    })
}

pub(crate) fn looks_like_path(input: &str) -> bool {
    input == "."
        || input == ".."
        || input.contains('/')
        || input.contains('\\')
        || input.starts_with('~')
}

/// `~`、`~/…` 展开成主目录；其余原样返回。命令行打印出来的路径可以直接粘回来
pub fn expand_path(input: impl AsRef<Path>) -> PathBuf {
    let path = input.as_ref();
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    if text == "~" {
        return paths::home_dir();
    }
    match text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
        Some(rest) => paths::home_dir().join(rest),
        None => path.to_path_buf(),
    }
}

/// 命令行里的路径变成绝对路径：先展开 `~`，相对路径按当前目录
pub fn absolute(input: impl AsRef<Path>) -> PathBuf {
    let path = expand_path(input);
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Normalize parent-directory aliases without following the selected entry itself.
/// A request to remove a symlink must never become a request to remove its target.
pub(crate) fn entry_path(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => dunce::canonicalize(parent)
            .unwrap_or_else(|_| parent.to_path_buf())
            .join(name),
        _ => canonical(path),
    }
}

pub fn skill<'a>(skills: &'a [SkillView], input: &str) -> Result<&'a SkillView, CliError> {
    select_skill(skills, input, true)
}

/// File lifecycle operations must select one source, even when a same-named Hub copy exists.
pub fn skill_source<'a>(skills: &'a [SkillView], input: &str) -> Result<&'a SkillView, CliError> {
    select_skill(skills, input, false)
}

fn select_skill<'a>(
    skills: &'a [SkillView],
    input: &str,
    prefer_hub: bool,
) -> Result<&'a SkillView, CliError> {
    let input = input.trim();
    if let Some(view) = skills.iter().find(|v| v.skill.id == input) {
        return Ok(view);
    }
    if looks_like_path(input) {
        let normalize = if prefer_hub { canonical } else { entry_path };
        let wanted = normalize(&absolute(input));
        if let Some(view) = skills
            .iter()
            .find(|v| normalize(&v.skill.source_path) == wanted)
        {
            return Ok(view);
        }
        if !prefer_hub {
            return Err(CliError::not_found(
                "这不是列表中的 Skill 来源；不会跟随末级软链接操作其目标",
            )
            .candidates(skills.iter().map(|v| home(&v.skill.source_path)).collect())
            .hint("请使用 skills list 列出的实际来源路径或 ID"));
        }
    }
    let named = |exact: bool| -> Vec<&'a SkillView> {
        skills
            .iter()
            .filter(|v| {
                let names =
                    std::iter::once(v.skill.name.as_str()).chain(v.skill.display_name.as_deref());
                let mut names = names;
                if exact {
                    names.any(|n| n == input)
                } else {
                    names.any(|n| n.to_lowercase() == input.to_lowercase())
                }
            })
            .collect()
    };
    let mut matches = named(true);
    if matches.is_empty() {
        matches = named(false);
    }
    match matches.len() {
        1 => Ok(matches[0]),
        0 => {
            let lower = input.to_lowercase();
            let mut similar: Vec<String> = skills
                .iter()
                .filter(|v| v.skill.name.to_lowercase().contains(&lower))
                .map(|v| v.skill.name.clone())
                .collect();
            similar.sort();
            similar.dedup();
            similar.truncate(10);
            Err(CliError::not_found(format!("找不到 skill「{input}」"))
                .candidates(similar)
                .hint("用 skill-studio skills list 查看全部 skill，或 skills search 在 skills.sh 上搜索"))
        }
        _ => {
            // 同名时托管在 Hub 里的那份就是「这个 skill」；Agent 目录里的同名原件要写目录区分
            let hub: Vec<_> = matches
                .iter()
                .filter(|v| matches!(v.skill.origin, SkillOrigin::Hub))
                .collect();
            if prefer_hub && hub.len() == 1 {
                return Ok(hub[0]);
            }
            Err(
                CliError::not_found(format!("有 {} 个 skill 都叫「{input}」", matches.len()))
                    .candidates(
                        matches
                            .iter()
                            .map(|v| format!("{} · {}", v.skill.name, home(&v.skill.source_path)))
                            .collect(),
                    )
                    .hint("改写成 skill 所在的目录，或它的 ID"),
            )
        }
    }
}

/// 多个 skill，返回 ID，去重并保持顺序
pub fn skills(views: &[SkillView], inputs: &[String]) -> Result<Vec<String>, CliError> {
    let mut ids = Vec::new();
    for input in inputs.iter().filter(|i| !i.trim().is_empty()) {
        let id = skill(views, input)?.skill.id.clone();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

pub fn project<'a>(config: &'a AppConfig, input: &str) -> Result<&'a ProjectBinding, CliError> {
    let input = input.trim();
    // IDs and explicit paths must never be shadowed by another project's display name.
    if let Some(project) = config.projects.iter().find(|p| p.id == input) {
        return Ok(project);
    }
    let ambiguous = |matches: Vec<&'a ProjectBinding>| {
        CliError::not_found(format!("有 {} 个项目匹配「{input}」", matches.len()))
            .candidates(
                matches
                    .iter()
                    .map(|p| format!("{} · {} · ID {}", p.name, p.root.display(), p.id))
                    .collect(),
            )
            .hint("请改用项目目录或 ID")
    };
    if looks_like_path(input) {
        // 在项目子目录里运行时选最深的一层；同深度的目录别名也不能任意挑一个。
        let wanted = canonical(&absolute(input));
        let mut matches: Vec<_> = config
            .projects
            .iter()
            .filter_map(|p| {
                let root = canonical(&p.root);
                wanted
                    .starts_with(&root)
                    .then_some((p, root.components().count()))
            })
            .collect();
        matches.sort_by_key(|(_, depth)| std::cmp::Reverse(*depth));
        if let Some((_, depth)) = matches.first() {
            let closest: Vec<_> = matches
                .iter()
                .take_while(|(_, d)| d == depth)
                .map(|(project, _)| *project)
                .collect();
            return if closest.len() == 1 {
                Ok(closest[0])
            } else {
                Err(ambiguous(closest))
            };
        }
    }
    for exact in [true, false] {
        let matches: Vec<_> = config
            .projects
            .iter()
            .filter(|p| {
                if exact {
                    p.name == input
                } else {
                    p.name.to_lowercase() == input.to_lowercase()
                }
            })
            .collect();
        match matches.len() {
            0 => {}
            1 => return Ok(matches[0]),
            _ => return Err(ambiguous(matches)),
        }
    }
    Err(CliError::not_found(format!("找不到项目「{input}」"))
        .candidates(config.projects.iter().map(|p| p.name.clone()).collect())
        .hint(if looks_like_path(input) {
            "这个目录还不是项目，用 skill-studio projects create <目录> 添加"
        } else {
            "用 skill-studio projects list 查看全部项目"
        }))
}
