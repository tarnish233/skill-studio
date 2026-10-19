use skill_studio_core::models::group::{Group, UntouchedCopy};
use skill_studio_core::Error;

use crate::state::AppState;

pub fn list_groups(state: &AppState) -> Result<Vec<Group>, String> {
    let mut groups = state.config().groups.clone();
    groups.sort_by_key(|g| g.sort_order);
    Ok(groups)
}

/// 删除分组。任一 Agent 正在使用的分组必须先停用；删除只清理分组记录和项目绑定，
/// 不动任何文件。
pub fn delete_group(state: &AppState, group_id: String) -> Result<(), String> {
    state
        .mutate(|_, config| {
            if !config.agents_using_group(&group_id).is_empty() {
                return Err(Error::invalid("请先停用正在使用的分组，再删除"));
            }
            let before = config.groups.len();
            config.groups.retain(|g| g.id != group_id);
            if config.groups.len() == before {
                return Err(Error::NotFound(format!("分组 {group_id}")));
            }
            for project in &mut config.projects {
                project.group_ids.retain(|id| id != &group_id);
            }
            Ok(())
        })
        .map_err(Into::into)
}

pub fn reorder_groups(state: &AppState, group_ids: Vec<String>) -> Result<Vec<Group>, String> {
    state
        .mutate(|_, config| {
            for (i, id) in group_ids.iter().enumerate() {
                if let Some(g) = config.group_mut(id) {
                    g.sort_order = i as i32;
                }
            }
            let mut groups = config.groups.clone();
            groups.sort_by_key(|g| g.sort_order);
            Ok(groups)
        })
        .map_err(Into::into)
}

fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn save_group(
    state: &AppState,
    group_id: Option<String>,
    name: String,
    skill_ids: Vec<String>,
) -> Result<Group, String> {
    state
        .mutate(|studio, config| {
            studio.save_group(config, group_id.unwrap_or_else(uuid_v4), &name, skill_ids)
        })
        .map_err(Into::into)
}

/// 设置这个 agent 当前叠加的全部分组；空列表表示全部停用。返回部署后被改过、原样留下的副本
pub fn activate_agent_groups(
    state: &AppState,
    agent_id: String,
    group_ids: Vec<String>,
) -> Result<Vec<UntouchedCopy>, String> {
    state
        .activate_agent_groups(&agent_id, &group_ids)
        .map_err(Into::into)
}
