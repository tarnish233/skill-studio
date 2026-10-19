use serde::{Deserialize, Serialize};

/// A saved skill combination, shared across agents. Each agent independently
/// decides whether this group is its current active combination (see
/// `AppConfig::active_groups`), so the same group can be active for several
/// agents at once without being duplicated per agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// lucide 图标名，前端按名字取图标
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub sort_order: i32,
    /// 成员 skill 的 ID，顺序可由用户拖拽调整
    #[serde(default)]
    pub skill_ids: Vec<String>,
}

impl Group {
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            description: None,
            icon: None,
            sort_order: 0,
            skill_ids: Vec::new(),
        }
    }

    pub fn contains(&self, skill_id: &str) -> bool {
        self.skill_ids.iter().any(|s| s == skill_id)
    }

    /// 幂等添加，重复调用不会产生重复成员
    pub fn add_skill(&mut self, skill_id: String) -> bool {
        if self.contains(&skill_id) {
            return false;
        }
        self.skill_ids.push(skill_id);
        true
    }

    pub fn remove_skill(&mut self, skill_id: &str) -> bool {
        let before = self.skill_ids.len();
        self.skill_ids.retain(|s| s != skill_id);
        self.skill_ids.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_skill_is_idempotent() {
        let mut g = Group::new("g1".into(), "前端".into());
        assert!(g.add_skill("s1".into()));
        assert!(!g.add_skill("s1".into()));
        assert_eq!(g.skill_ids.len(), 1);
    }

    #[test]
    fn remove_skill_reports_whether_it_changed() {
        let mut g = Group::new("g1".into(), "前端".into());
        g.add_skill("s1".into());
        assert!(g.remove_skill("s1"));
        assert!(!g.remove_skill("s1"));
        assert!(g.skill_ids.is_empty());
    }

    #[test]
    fn optional_fields_are_omitted_when_empty() {
        let g = Group::new("g1".into(), "前端".into());
        let json = serde_json::to_string(&g).unwrap();
        assert!(!json.contains("description"), "{json}");
        assert!(!json.contains("icon"), "{json}");
    }
}

/// Exact entries installed by a group, separate from manual registrations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupEntry {
    pub skill_id: String,
    pub source_path: std::path::PathBuf,
    pub target_path: std::path::PathBuf,
}

/// 切换分组时 Studio 没有动的副本：部署后被修改、被替换，或在 Agent 里被手动停用。
/// 不删、不覆盖——那已经不是 Studio 放进去的样子——只在结果里报告。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UntouchedCopy {
    pub skill_id: String,
    pub name: String,
    pub path: std::path::PathBuf,
    /// 「副本已被修改」「已被替换」「已被手动停用」
    pub reason: String,
    /// 仍在用它的分组继续管理它（卡片上会显示需要检查）；false 表示分组已不再管理，
    /// 留在 Agent 目录里由用户处理
    pub tracked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveGroup {
    pub group_id: String,
    pub skill_ids: Vec<String>,
    pub entries: Vec<GroupEntry>,
    #[serde(default = "default_preserve_manual")]
    pub preserve_manual_skills: bool,
}

fn default_preserve_manual() -> bool {
    true
}
