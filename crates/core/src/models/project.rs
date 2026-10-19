use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::skill::LinkMode;

/// 项目级 skill 绑定：这些 skill 只对这个项目生效。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBinding {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    /// 要为哪些 agent 写入项目 skill 目录
    #[serde(default)]
    pub agent_ids: Vec<String>,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    /// 也可以整组绑到项目
    #[serde(default)]
    pub group_ids: Vec<String>,
    /// 项目级**默认 Copy**：项目 skill 通常要进 git 给团队共享，
    /// 而 symlink 进 git 只是一个指向本机绝对路径的文本文件，别人拉下来就是断链。
    #[serde(default = "default_project_link_mode")]
    pub link_mode: LinkMode,
    /// Exact deployments owned by this project; never infer ownership from a name alone.
    #[serde(default)]
    pub managed_entries: Vec<ProjectEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    pub skill_id: String,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSelection {
    pub agent_ids: Vec<String>,
    pub skill_ids: Vec<String>,
    pub group_ids: Vec<String>,
    pub link_mode: LinkMode,
}

fn default_project_link_mode() -> LinkMode {
    LinkMode::Copy
}

/// 项目在某个 Agent 的项目目录里的部署：卡片上按 Agent 显示是否启用、是否需要应用修改或检查
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDeployment {
    pub agent_id: String,
    /// Studio 记录的、写进这个 Agent 项目目录的 skill
    pub skill_ids: Vec<String>,
    /// 有副本落后于 Hub 里的源：重新写入即可
    pub stale: bool,
    /// 部署出了问题（副本被修改、被替换或已不存在），需要用户处理；Studio 不会覆盖
    pub issue: Option<String>,
}

impl ProjectBinding {
    pub fn new(id: String, name: String, root: PathBuf) -> Self {
        Self {
            id,
            name,
            root,
            agent_ids: Vec::new(),
            skill_ids: Vec::new(),
            group_ids: Vec::new(),
            link_mode: LinkMode::Copy,
            managed_entries: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_defaults_to_copy_mode() {
        let p = ProjectBinding::new("p1".into(), "web".into(), PathBuf::from("/tmp/web"));
        assert_eq!(p.link_mode, LinkMode::Copy);
    }

    #[test]
    fn missing_link_mode_deserializes_to_copy() {
        // 老配置里没有 linkMode 字段时，必须回落到 Copy 而不是 Auto，
        // 否则项目 skill 会变成不可提交的 symlink
        let json = r#"{"id":"p1","name":"web","root":"/tmp/web"}"#;
        let p: ProjectBinding = serde_json::from_str(json).unwrap();
        assert_eq!(p.link_mode, LinkMode::Copy);
    }
}
