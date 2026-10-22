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

/// 打开编辑时分组的样子：保存前核对，别处（另一个窗口、命令行）改过就不覆盖
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSnapshot {
    pub name: String,
    pub skill_ids: Vec<String>,
}

impl GroupSnapshot {
    /// 名称前后的空格和成员顺序不算修改
    fn matches(&self, group: &Group) -> bool {
        let set = |ids: &[String]| {
            ids.iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
        };
        self.name.trim() == group.name && set(&self.skill_ids) == set(&group.skill_ids)
    }
}

/// 保存分组的名称和成员。`expected` 是打开编辑时的样子，与配置里的不同说明别处刚改过，
/// 拒绝保存以免覆盖；新建时为 `None`
pub fn save_group(
    state: &AppState,
    group_id: Option<String>,
    name: String,
    skill_ids: Vec<String>,
    expected: Option<GroupSnapshot>,
) -> Result<Group, String> {
    state
        .mutate(|studio, config| {
            if let (Some(id), Some(expected)) = (&group_id, &expected) {
                let current = config
                    .group(id)
                    .ok_or_else(|| Error::Other("分组已被别处删除".into()))?;
                if !expected.matches(current) {
                    // 不加「无效输入」前缀：界面显示「保存失败，修改已保留：分组已被别处修改…」
                    return Err(Error::Other(format!(
                        "分组{}",
                        crate::state::CHANGED_ELSEWHERE
                    )));
                }
            }
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

/// 为一个 Agent 启用或停用一个分组，它叠加的其他分组不变；已经是目标状态时什么也不做
pub fn set_agent_group(
    state: &AppState,
    agent_id: String,
    group_id: String,
    enabled: bool,
) -> Result<Vec<UntouchedCopy>, String> {
    state
        .set_agent_group(&agent_id, &group_id, enabled)
        .map_err(Into::into)
}

/// 按最新的启用记录重新写入这个 Agent 叠加的全部分组（应用修改）
pub fn reapply_agent_groups(
    state: &AppState,
    agent_id: String,
) -> Result<Vec<UntouchedCopy>, String> {
    state.reapply_agent_groups(&agent_id).map_err(Into::into)
}
