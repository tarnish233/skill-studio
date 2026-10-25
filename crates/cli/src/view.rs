//! 和界面同一个口径的状态：分组、项目在每个 Agent 上是否「有待应用修改」「需要检查」。
//! 对应前端 GroupsPage 的 status() 与 ProjectsPage 的 cardState()。

use std::collections::BTreeSet;

use skill_studio_core::models::agent::AGENTS;
use skill_studio_core::models::config::AppConfig;
use skill_studio_core::models::group::Group;
use skill_studio_core::models::project::ProjectBinding;
use skill_studio_core::models::skill::LinkStatus;
use skill_studio_core::services::project_write::project_deployments;
use skill_studio_core::services::studio::{AgentSkillState, SkillView};
use skill_studio_service::state::AppState;

use crate::error::CliError;
use crate::report::{agent_name, list};

pub struct Data {
    pub config: AppConfig,
    pub skills: Vec<SkillView>,
}

/// 只读打开时只扫描；可写打开时与界面一样先校正「保留手动 skill」再扫描
pub fn load(state: &AppState) -> Result<Data, CliError> {
    let skills = skill_studio_service::skills::scan_skills(state)?;
    let config = state.config().clone();
    Ok(Data { config, skills })
}

impl Data {
    pub fn skill(&self, id: &str) -> Option<&SkillView> {
        self.skills.iter().find(|v| v.skill.id == id)
    }

    pub fn skill_name(&self, id: &str) -> Option<&str> {
        self.skill(id).map(|v| v.skill.name.as_str())
    }

    pub fn group(&self, id: &str) -> Option<&Group> {
        self.config.groups.iter().find(|g| g.id == id)
    }

    pub fn groups(&self) -> Vec<&Group> {
        let mut groups: Vec<_> = self.config.groups.iter().collect();
        groups.sort_by_key(|g| g.sort_order);
        groups
    }
}

/// Agent 能读到这个 skill：副本在、没被原生开关停用（与前端 isRegistered 一致）
pub fn loaded(state: &AgentSkillState) -> bool {
    !state.disabled
        && matches!(
            state.status,
            LinkStatus::Source
                | LinkStatus::Linked
                | LinkStatus::Copied
                | LinkStatus::CopyStale
                | LinkStatus::CopyModified
                | LinkStatus::CopyConflict
        )
}

/// 必须由人动手才能用（与前端 needsManualFix 一致）
fn manual_fix(status: LinkStatus) -> bool {
    matches!(
        status,
        LinkStatus::NotLinked
            | LinkStatus::CopyConflict
            | LinkStatus::CopyDamaged
            | LinkStatus::Foreign
            | LinkStatus::BrokenLink
            | LinkStatus::Conflict
    )
}

pub fn status_label(status: LinkStatus) -> &'static str {
    match status {
        LinkStatus::NotLinked => "未注册",
        LinkStatus::Source => "真身在此",
        LinkStatus::Linked => "已软链",
        LinkStatus::Copied => "已复制",
        LinkStatus::CopyStale => "副本过期",
        LinkStatus::CopyModified => "本地已修改",
        LinkStatus::CopyConflict => "副本冲突",
        LinkStatus::CopyDamaged => "副本损坏",
        LinkStatus::Foreign => "被占用",
        LinkStatus::BrokenLink => "链接失效",
        LinkStatus::Conflict => "指向冲突",
    }
}

fn same_members(a: &[String], b: &[String]) -> bool {
    a.iter().collect::<BTreeSet<_>>() == b.iter().collect::<BTreeSet<_>>()
}

/// 按 Agent 的固定顺序排
fn ordered(mut ids: Vec<String>) -> Vec<String> {
    ids.sort_by_key(|id| AGENTS.iter().position(|a| a.id == id).unwrap_or(usize::MAX));
    ids.dedup();
    ids
}

/// 一个分组在某个 Agent 上的状态。没有启用的 Agent 不列出
pub struct AgentState {
    pub agent: String,
    /// 有待应用修改的原因：成员变了、副本过期……
    pub pending: Vec<String>,
    /// 需要检查的原因
    pub issues: Vec<String>,
}

/// 哪些 Agent 启用了这个分组
pub fn group_agents(config: &AppConfig, group_id: &str) -> Vec<String> {
    ordered(
        config
            .active_groups
            .iter()
            .filter(|(_, records)| records.iter().any(|r| r.group_id == group_id))
            .map(|(agent, _)| agent.clone())
            .collect(),
    )
}

/// 按名称移出「已经不在 Hub 里」的成员时，`resolve` 给的是去搜索的提示——那会装回一个
/// 同名的新成员，正是当初要避免的。目标里已有缺失成员时，改成提示按 ID 移出。
/// `subject` 如「分组「前端」」，`show` 如 `groups show`
pub fn missing_member_hint(
    error: CliError,
    missing: Vec<String>,
    subject: &str,
    show: &str,
) -> CliError {
    if error.kind != crate::Kind::NotFound || missing.is_empty() {
        return error;
    }
    error.hint(format!(
        "{subject}里已不在 Hub 的成员只能按 ID 移出：{}（用 {show} 查看）",
        list(&missing)
    ))
}

/// 「没有变化」的回答里补一句：哪些 Agent 上还欠着写入。调用方（尤其是 Agent）重跑
/// 同一条命令时，不能只看到「没有变化」就以为已经做完
pub fn pending_note(states: &[AgentState], apply: &str) -> String {
    let pending: Vec<String> = states
        .iter()
        .filter(|state| !state.pending.is_empty())
        .map(|state| format!("{}（{}）", agent_name(&state.agent), list(&state.pending)))
        .collect();
    if pending.is_empty() {
        String::new()
    } else {
        format!("；但 {}上还有待应用修改 → {apply}", pending.join("、"))
    }
}

pub fn group_states(data: &Data, group: &Group) -> Vec<AgentState> {
    group_agents(&data.config, &group.id)
        .into_iter()
        .filter_map(|agent| {
            let record = data
                .config
                .active_groups
                .get(&agent)?
                .iter()
                .find(|r| r.group_id == group.id)?;
            let state_of = |id: &str| data.skill(id).and_then(|v| v.agents.get(&agent));
            let mut pending = Vec::new();
            if record
                .entries
                .iter()
                .any(|e| state_of(&e.skill_id).is_some_and(|s| s.status == LinkStatus::CopyStale))
            {
                pending.push("Hub 里的源已更新，副本过期".to_string());
            }
            if !same_members(&record.skill_ids, &group.skill_ids) {
                pending.push("成员已变化".to_string());
            }
            if record.preserve_manual_skills != data.config.settings.preserve_manual_skills {
                pending.push("「保留手动 skill」设置已变化".to_string());
            }
            let issues = record
                .skill_ids
                .iter()
                .filter_map(|id| {
                    let Some(name) = data.skill_name(id) else {
                        return Some(format!("{id} 已缺失"));
                    };
                    let Some(state) = state_of(id) else {
                        return Some(format!("{name} 在这个 Agent 上不可用"));
                    };
                    if state.disabled {
                        return Some(format!("{name} 已被手动停用"));
                    }
                    if state.status == LinkStatus::CopyModified {
                        return Some(format!("{name} 的副本已被修改"));
                    }
                    manual_fix(state.status)
                        .then(|| format!("{name}：{}", status_label(state.status)))
                })
                .collect();
            Some(AgentState {
                agent,
                pending,
                issues,
            })
        })
        .collect()
}

/// 项目的全部成员：单独选的 skill 加上绑定分组的成员
pub fn project_members(config: &AppConfig, project: &ProjectBinding) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let groups = project
        .group_ids
        .iter()
        .filter_map(|id| config.groups.iter().find(|g| &g.id == id));
    for id in project
        .skill_ids
        .iter()
        .chain(groups.flat_map(|g| g.skill_ids.iter()))
    {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}

pub fn project_agents(project: &ProjectBinding) -> Vec<String> {
    ordered(
        project_deployments(project)
            .into_iter()
            .map(|d| d.agent_id)
            .collect(),
    )
}

pub fn project_states(data: &Data, project: &ProjectBinding) -> Vec<AgentState> {
    let existing: Vec<String> = project_members(&data.config, project)
        .into_iter()
        .filter(|id| data.skill(id).is_some())
        .collect();
    let mut states: Vec<_> = project_deployments(project)
        .into_iter()
        .map(|d| {
            let mut pending = Vec::new();
            if d.stale {
                pending.push("Hub 里的源已更新，副本过期".to_string());
            }
            if !same_members(&d.skill_ids, &existing) {
                pending.push("成员已变化".to_string());
            }
            AgentState {
                agent: d.agent_id,
                pending,
                issues: d.issue.into_iter().collect(),
            }
        })
        .collect();
    states.sort_by_key(|s| AGENTS.iter().position(|a| a.id == s.agent));
    states
}

/// 成员里已经不在 Hub 的 skill，和已删除的分组
pub fn project_missing(data: &Data, project: &ProjectBinding) -> Vec<String> {
    let mut missing: Vec<String> = project_members(&data.config, project)
        .into_iter()
        .filter(|id| data.skill(id).is_none())
        .collect();
    missing.extend(
        project
            .group_ids
            .iter()
            .filter(|id| data.group(id).is_none())
            .map(|id| format!("分组 {id}")),
    );
    missing
}
