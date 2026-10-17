use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::group::Group;
use super::project::ProjectBinding;
use super::skill::LinkMode;

/// 当前配置结构版本。改结构时 +1 并在 store 里加迁移分支。
/// v4：分组不再归属单个 agent；是否对某 agent 生效完全由 active_groups 决定。
/// v5：一个 agent 可以同时叠加多个分组（active_groups 的值由单个记录变成列表），
/// 部署内容是这些分组成员加手动 skill 的并集。
pub const CONFIG_VERSION: u32 = 5;

/// 某个 skill 在某个 agent 上的注册记录。
///
/// 不用 cc-switch 的扁平 bool（`SkillApps { claude: bool, codex: bool, ... }`）——
/// 那种写法加一个 agent 要改 5 处 `match` 加一次 schema 迁移。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registration {
    pub mode: LinkMode,
    pub target_path: PathBuf,
    pub registered_at: i64,
    /// 仅 Copy 模式：复制那一刻的源哈希，用于判定 CopyStale
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash_at_copy: Option<String>,
}

/// skill_id -> agent_id -> 注册记录
pub type Registrations = HashMap<String, HashMap<String, Registration>>;

/// 用户对单个 skill 的附加信息
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// 关闭管理入口的应用；不修改已安装内容或来源记录。
    #[serde(default)]
    pub disabled_agents: Vec<String>,
    #[serde(default)]
    pub show_codex_builtin_mcp: bool,
    /// Whether Studio manages MCP connections. Existing installations stay enabled.
    #[serde(default = "default_manage_mcp")]
    pub manage_mcp: bool,
    /// 新建注册时的默认链接方式
    #[serde(default)]
    pub default_link_mode: LinkMode,
    #[serde(default = "default_preserve_manual_skills")]
    pub preserve_manual_skills: bool,
    /// 界面语言
    #[serde(default = "default_language")]
    pub language: String,
    /// 主题（前端也存了一份在 localStorage，这里用于托盘等原生侧）
    #[serde(default = "default_theme")]
    pub theme: String,
    /// 各 agent 的配置目录覆盖：agent_id -> 目录。
    /// 优先级高于环境变量，因为 GUI 进程未必继承 shell profile 里的环境变量。
    #[serde(default)]
    pub agent_dir_overrides: HashMap<String, PathBuf>,
    /// Hub 目录覆盖，默认 `~/.skill-studio/skills`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hub_dir: Option<PathBuf>,
    /// 保留多少份 config.json 备份
    #[serde(default = "default_backup_keep")]
    pub backup_keep: usize,
}

fn default_preserve_manual_skills() -> bool {
    true
}

fn default_manage_mcp() -> bool {
    true
}

fn default_language() -> String {
    "zh".to_string()
}

fn default_theme() -> String {
    "system".to_string()
}

fn default_backup_keep() -> usize {
    10
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            disabled_agents: Vec::new(),
            show_codex_builtin_mcp: false,
            manage_mcp: true,
            default_link_mode: LinkMode::default(),
            preserve_manual_skills: true,
            language: default_language(),
            theme: default_theme(),
            agent_dir_overrides: HashMap::new(),
            hub_dir: None,
            backup_keep: default_backup_keep(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillVariant {
    /// "generic" or a registered Agent ID; also the managed storage directory key.
    pub key: String,
    pub repository_path: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInstallation {
    pub source: String,
    pub skill_id: String,
    pub repository_path: String,
    pub installed_at: i64,
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<SkillVariant>,
}

/// Durable provenance and recovery information; independent of the current storage location.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillProvenance {
    pub source_ids: Vec<String>,
    pub original_path: PathBuf,
    pub original_root: PathBuf,
    pub original_origin: super::skill::SkillOrigin,
    pub backup_path: PathBuf,
    pub original_hash: String,
    pub collected_at: i64,
    #[serde(default)]
    pub entry_paths: Vec<PathBuf>,
}

/// 落盘的根结构：`~/.skill-studio/config.json`
///
/// 刻意不用 SQLite。cc-switch 上 SQLite 是被 provider / proxy / usage 的复杂需求
/// 推上去的（schema.rs 3800+ 行、19 个 schema 版本）；纯 skill 管理数据量在几百条
/// 量级，单个 JSON 可读、可 git、可手改、易备份。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub groups: Vec<Group>,
    /// agent → 当前叠加的分组记录（按启用顺序）。改成列表时保留旧版单对象格式的读取，
    /// 见 [`de_active_groups`]。
    #[serde(default, deserialize_with = "de_active_groups")]
    pub active_groups: HashMap<String, Vec<super::group::ActiveGroup>>,
    #[serde(default)]
    pub policy_suspensions: HashMap<String, Vec<super::group::GroupEntry>>,
    #[serde(default)]
    pub projects: Vec<ProjectBinding>,
    #[serde(default)]
    pub registrations: Registrations,
    #[serde(default)]
    pub skill_meta: HashMap<String, SkillMeta>,
    #[serde(default)]
    pub skill_provenance: HashMap<String, SkillProvenance>,
    #[serde(default)]
    pub skill_installations: HashMap<String, SkillInstallation>,
    /// agent → Studio 为了让分组管得住它而关掉的兼容读取开关（如 Grok 的
    /// `compat.claude.skills`）。只记 Studio 自己改动的：退出管理时据此还原，
    /// 用户本来就关着的、或之后自己改回去的都不碰。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub native_isolation: HashMap<String, Vec<String>>,
}

fn default_version() -> u32 {
    CONFIG_VERSION
}

/// v4 及更早的配置里每个 agent 只有一个分组对象；v5 起是列表。
/// 两种形状都读得进来，升级后按列表写回，不需要先在 JSON 上做手术。
fn de_active_groups<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, Vec<super::group::ActiveGroup>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        Many(Vec<super::group::ActiveGroup>),
        One(super::group::ActiveGroup),
    }
    HashMap::<String, OneOrMany>::deserialize(deserializer).map(|map| {
        map.into_iter()
            .map(|(agent, value)| {
                let records = match value {
                    OneOrMany::Many(v) => v,
                    OneOrMany::One(v) => vec![v],
                };
                (agent, records)
            })
            .collect()
    })
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            settings: Settings::default(),
            groups: Vec::new(),
            active_groups: HashMap::new(),
            policy_suspensions: HashMap::new(),
            projects: Vec::new(),
            registrations: HashMap::new(),
            skill_meta: HashMap::new(),
            skill_provenance: HashMap::new(),
            skill_installations: HashMap::new(),
            native_isolation: HashMap::new(),
        }
    }
}

impl AppConfig {
    pub fn group(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn group_mut(&mut self, id: &str) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    pub fn project(&self, id: &str) -> Option<&ProjectBinding> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn project_mut(&mut self, id: &str) -> Option<&mut ProjectBinding> {
        self.projects.iter_mut().find(|p| p.id == id)
    }

    /// 这个 agent 当前叠加的分组记录，按启用顺序
    pub fn active_groups_of(&self, agent_id: &str) -> &[super::group::ActiveGroup] {
        self.active_groups.get(agent_id).map_or(&[], Vec::as_slice)
    }

    /// 这个 agent 当前生效的全部分组成员（叠加后的并集，含顺序与去重）
    pub fn active_skill_ids(&self, agent_id: &str) -> Vec<String> {
        let mut seen = HashSet::new();
        self.active_groups_of(agent_id)
            .iter()
            .flat_map(|g| g.skill_ids.iter())
            .filter(|id| seen.insert((*id).clone()))
            .cloned()
            .collect()
    }

    /// 这个 agent 当前部署的全部入口（分组副本/链接），按分组顺序去重
    pub fn active_entries(&self, agent_id: &str) -> Vec<&super::group::GroupEntry> {
        let mut seen = HashSet::new();
        self.active_groups_of(agent_id)
            .iter()
            .flat_map(|g| g.entries.iter())
            .filter(|e| seen.insert(e.target_path.clone()))
            .collect()
    }

    /// 某个 agent 是否正在叠加这个分组
    pub fn group_is_active(&self, agent_id: &str, group_id: &str) -> bool {
        self.active_groups_of(agent_id)
            .iter()
            .any(|g| g.group_id == group_id)
    }

    /// 正在叠加这个分组的所有 agent
    pub fn agents_using_group(&self, group_id: &str) -> Vec<&str> {
        self.active_groups
            .iter()
            .filter(|(_, records)| records.iter().any(|g| g.group_id == group_id))
            .map(|(agent, _)| agent.as_str())
            .collect()
    }

    pub fn registration(&self, skill_id: &str, agent_id: &str) -> Option<&Registration> {
        self.registrations.get(skill_id)?.get(agent_id)
    }

    pub fn set_registration(&mut self, skill_id: &str, agent_id: &str, reg: Registration) {
        self.registrations
            .entry(skill_id.to_string())
            .or_default()
            .insert(agent_id.to_string(), reg);
    }

    pub fn remove_registration(&mut self, skill_id: &str, agent_id: &str) -> bool {
        let Some(per_agent) = self.registrations.get_mut(skill_id) else {
            return false;
        };
        let removed = per_agent.remove(agent_id).is_some();
        if per_agent.is_empty() {
            self.registrations.remove(skill_id);
        }
        removed
    }

    /// 某个 skill 被哪些分组包含（用于 Remove 时提示"还在别的组里"）
    pub fn groups_containing(&self, skill_id: &str) -> Vec<&Group> {
        self.groups
            .iter()
            .filter(|g| g.contains(skill_id))
            .collect()
    }

    /// 清掉引用了已不存在 skill 的分组成员与注册记录，返回清理条数。
    /// skill 真身被用户在 Studio 之外删掉时会用到。
    pub fn prune_missing_skills(&mut self, existing: &std::collections::HashSet<String>) -> usize {
        let mut pruned = 0;
        for group in &mut self.groups {
            let before = group.skill_ids.len();
            group.skill_ids.retain(|id| existing.contains(id));
            pruned += before - group.skill_ids.len();
        }
        for project in &mut self.projects {
            let before = project.skill_ids.len();
            project.skill_ids.retain(|id| existing.contains(id));
            pruned += before - project.skill_ids.len();
        }
        let before = self.registrations.len();
        self.registrations.retain(|id, _| existing.contains(id));
        pruned += before - self.registrations.len();
        self.skill_meta.retain(|id, _| existing.contains(id));
        pruned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn reg() -> Registration {
        Registration {
            mode: LinkMode::Symlink,
            target_path: PathBuf::from("/tmp/x"),
            registered_at: 0,
            source_hash_at_copy: None,
        }
    }

    #[test]
    fn empty_json_deserializes_to_defaults() {
        // 所有字段都有 #[serde(default)]，空对象也要能读出可用配置
        let cfg: AppConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.version, CONFIG_VERSION);
        assert_eq!(cfg.settings.language, "zh");
        assert_eq!(cfg.settings.default_link_mode, LinkMode::Auto);
        assert_eq!(cfg.settings.backup_keep, 10);
        assert!(cfg.groups.is_empty());
    }

    #[test]
    fn registration_roundtrip_and_removal_cleans_empty_maps() {
        let mut cfg = AppConfig::default();
        cfg.set_registration("s1", "codex", reg());
        assert!(cfg.registration("s1", "codex").is_some());

        assert!(cfg.remove_registration("s1", "codex"));
        // 该 skill 已无任何 agent 注册，外层条目也要清掉，避免留空壳
        assert!(!cfg.registrations.contains_key("s1"));
        assert!(!cfg.remove_registration("s1", "codex"));
    }

    #[test]
    fn groups_containing_finds_all_owners() {
        let mut cfg = AppConfig::default();
        let mut g1 = Group::new("g1".into(), "前端".into());
        g1.add_skill("s1".into());
        let mut g2 = Group::new("g2".into(), "通用".into());
        g2.add_skill("s1".into());
        cfg.groups.push(g1);
        cfg.groups.push(g2);

        let owners = cfg.groups_containing("s1");
        assert_eq!(owners.len(), 2);
        assert!(cfg.groups_containing("nope").is_empty());
    }

    #[test]
    fn prune_drops_references_to_vanished_skills() {
        let mut cfg = AppConfig::default();
        let mut g = Group::new("g1".into(), "前端".into());
        g.add_skill("alive".into());
        g.add_skill("gone".into());
        cfg.groups.push(g);
        cfg.set_registration("gone", "codex", reg());
        cfg.skill_meta.insert("gone".into(), SkillMeta::default());

        let mut existing = HashSet::new();
        existing.insert("alive".to_string());
        let pruned = cfg.prune_missing_skills(&existing);

        assert_eq!(pruned, 2); // 分组成员 1 条 + 注册 1 条
        assert_eq!(cfg.groups[0].skill_ids, vec!["alive"]);
        assert!(cfg.registrations.is_empty());
        assert!(cfg.skill_meta.is_empty());
    }
}
