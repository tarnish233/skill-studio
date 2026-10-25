pub mod agent;
pub mod config;
pub mod group;
pub mod project;
pub mod skill;

/// 两组 ID 是同一批成员：顺序不算修改，重复项不影响（与界面 `sameMembers` 同口径）。
/// 分组、项目的「保存前核对」和命令行、卡片上的「有待应用修改」都用这一份
pub fn same_members(a: &[String], b: &[String]) -> bool {
    a.iter().collect::<std::collections::BTreeSet<_>>()
        == b.iter().collect::<std::collections::BTreeSet<_>>()
}

pub use agent::{
    find_agent, require_agent, AgentDescriptor, AgentInfo, SkillRoot, ToggleMechanism, AGENTS,
};
pub use config::{AppConfig, Registration, Registrations, Settings, SkillMeta, CONFIG_VERSION};
pub use group::Group;
pub use project::ProjectBinding;
pub use skill::{skill_id_for, LinkMode, LinkReport, LinkResult, LinkStatus, Skill, SkillOrigin};
