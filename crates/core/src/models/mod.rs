pub mod agent;
pub mod config;
pub mod group;
pub mod project;
pub mod skill;

pub use agent::{
    find_agent, require_agent, AgentDescriptor, AgentInfo, SkillRoot, ToggleMechanism, AGENTS,
};
pub use config::{AppConfig, Registration, Registrations, Settings, SkillMeta, CONFIG_VERSION};
pub use group::Group;
pub use project::ProjectBinding;
pub use skill::{skill_id_for, LinkMode, LinkReport, LinkResult, LinkStatus, Skill, SkillOrigin};
