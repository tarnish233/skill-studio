use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// skill 真身的所在
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SkillOrigin {
    /// 真身在某个 agent 的全局目录里（默认模式，零迁移）
    #[serde(rename_all = "camelCase")]
    InPlace { owner_agent: String },
    /// 已托管到 `~/.skill-studio/skills/`
    Hub,
    /// 通过链接发现的外部来源；发现不等于接管，禁止自动迁移真身。
    External,
}

/// 链接方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkMode {
    /// 优先 symlink，失败自动回退 copy（Windows 无开发者模式时会走到这里）
    #[default]
    Auto,
    Symlink,
    Copy,
}

/// 某个 skill 在某个 agent 上的实际状态。
/// 比 cc-switch 的"装了 / 没装"细，因为原地模式下目标可能是用户自己放的东西。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkStatus {
    /// 目标不存在
    NotLinked,
    /// 真身就在这个 agent 的目录里 —— 它天然可用，但不能作为"注册"被移除
    Source,
    /// symlink / junction，且指向正确
    Linked,
    /// 复制且与源一致
    Copied,
    /// 复制但源已变更（哈希不匹配）→ 提示重新复制
    CopyStale,
    /// 副本有本地修改，普通更新/移除必须保护。
    CopyModified,
    /// 源和副本都已变化。
    CopyConflict,
    /// 副本缺少必要文件或无法读取。
    CopyDamaged,
    /// 目标存在但非本工具管理 → **绝不覆盖**，UI 标红
    Foreign,
    /// symlink 悬空
    BrokenLink,
    /// 是链接但指向别处
    Conflict,
}

impl LinkStatus {
    /// 是否由本工具管理（决定能否安全移除）。
    /// `Source` 不算 —— 移除它等于删用户的 skill 真身。
    pub fn is_managed(self) -> bool {
        matches!(
            self,
            Self::Linked | Self::Copied | Self::CopyStale | Self::BrokenLink
        )
    }

    /// 该 agent 是否能用上这个 skill（UI 计数用）
    pub fn is_registered(self) -> bool {
        matches!(
            self,
            Self::Source
                | Self::Linked
                | Self::Copied
                | Self::CopyStale
                | Self::CopyModified
                | Self::CopyConflict
        )
    }

    /// 是否需要用户关注
    pub fn needs_attention(self) -> bool {
        matches!(
            self,
            Self::CopyStale
                | Self::CopyModified
                | Self::CopyConflict
                | Self::CopyDamaged
                | Self::Foreign
                | Self::BrokenLink
                | Self::Conflict
        )
    }
}

/// 一个 skill。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    /// 稳定 ID = 真身绝对路径的 SHA-256 前 16 位十六进制。
    /// 重命名目录会换 ID，因此重命名走专门流程同步更新分组 / 项目里的引用。
    pub id: String,
    /// 目录名，也是在 agent 里的调用名（`/my-skill`）
    pub name: String,
    /// frontmatter 的 `name` 字段
    pub display_name: Option<String>,
    pub description: Option<String>,
    /// 真身所在目录
    pub source_path: PathBuf,
    pub origin: SkillOrigin,
    /// 目录内容哈希，复制模式的 drift 检测用
    pub content_hash: String,
    /// frontmatter 的扩展字段，仅用于提示，不等于目标 Agent 不兼容。
    #[serde(default)]
    pub frontmatter_extra: Vec<String>,
    /// 所在的全局根（用于区分 `~/.codex/skills` 与共享的 `~/.agents/skills`）
    pub root: PathBuf,
    /// token 体量估算（见 `services::tokens`）。UI 用它算"这一组 / 这个项目
    /// 大概多重"。估算值，不是精确分词结果。
    #[serde(default)]
    pub tokens: TokenEstimate,
}

/// 一个 skill 的 token 体量，分成两半 —— 它们进上下文的时机不同：
/// `SKILL.md` 被调用时必然进去，附带文件是 agent 按需打开的。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenEstimate {
    /// `SKILL.md`（含 frontmatter）
    pub skill_md: u32,
    /// `references/`、`scripts/` 等文本附带文件；二进制与隐藏文件不计
    pub extras: u32,
}

impl TokenEstimate {
    pub fn total(&self) -> u32 {
        self.skill_md.saturating_add(self.extras)
    }
}

/// 计算 skill 的稳定 ID
pub fn skill_id_for(path: &Path) -> String {
    let key = crate::fs::paths::comparable_path_key(path);
    let digest = Sha256::digest(key.as_bytes());
    hex16(&digest)
}

fn hex16(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

/// 批量操作的单条结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkResult {
    pub skill_id: String,
    pub skill_name: String,
    pub agent_id: String,
    pub status: LinkStatus,
    pub message: Option<String>,
}

/// 批量操作报告。不用 `Result<(), E>` 一失败全断 —— UI 要逐条展示。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkReport {
    pub success: Vec<LinkResult>,
    pub failed: Vec<LinkResult>,
    /// 写入项目时原样留下的副本：部署后被修改或替换，Studio 不覆盖、不删除
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub untouched: Vec<super::group::UntouchedCopy>,
}

impl LinkReport {
    pub fn push_ok(&mut self, r: LinkResult) {
        self.success.push(r);
    }

    pub fn push_err(&mut self, r: LinkResult) {
        self.failed.push(r);
    }

    pub fn is_all_ok(&self) -> bool {
        self.failed.is_empty()
    }

    pub fn total(&self) -> usize {
        self.success.len() + self.failed.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_id_is_stable_and_path_sensitive() {
        let a = skill_id_for(Path::new("/home/u/.claude/skills/pdf"));
        let b = skill_id_for(Path::new("/home/u/.claude/skills/pdf"));
        let c = skill_id_for(Path::new("/home/u/.claude/skills/other"));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn skill_id_ignores_trailing_separator() {
        assert_eq!(
            skill_id_for(Path::new("/a/b/")),
            skill_id_for(Path::new("/a/b"))
        );
    }

    #[test]
    fn status_classification_is_consistent() {
        assert!(LinkStatus::Linked.is_managed() && LinkStatus::Linked.is_registered());
        // 真身所在的 agent 天然可用，但不能被当作注册移除
        assert!(LinkStatus::Source.is_registered());
        assert!(!LinkStatus::Source.is_managed());
        assert!(!LinkStatus::Source.needs_attention());
        assert!(LinkStatus::Copied.is_registered());
        assert!(LinkStatus::CopyModified.is_registered());
        assert!(LinkStatus::CopyConflict.is_registered());
        assert!(!LinkStatus::CopyDamaged.is_registered());
        assert!(!LinkStatus::CopyModified.is_managed());
        assert!(LinkStatus::CopyStale.is_registered() && LinkStatus::CopyStale.needs_attention());
        // Foreign 是用户自己放的，既不算本工具管理，也不算注册
        assert!(!LinkStatus::Foreign.is_managed());
        assert!(!LinkStatus::Foreign.is_registered());
        assert!(LinkStatus::Foreign.needs_attention());
        // 悬空链接是我们建的，可以清理，但不算注册成功
        assert!(LinkStatus::BrokenLink.is_managed());
        assert!(!LinkStatus::BrokenLink.is_registered());
        assert!(!LinkStatus::NotLinked.needs_attention());
    }

    #[test]
    fn report_tracks_totals() {
        let mut report = LinkReport::default();
        assert!(report.is_all_ok());
        report.push_ok(LinkResult {
            skill_id: "a".into(),
            skill_name: "a".into(),
            agent_id: "codex".into(),
            status: LinkStatus::Linked,
            message: None,
        });
        report.push_err(LinkResult {
            skill_id: "b".into(),
            skill_name: "b".into(),
            agent_id: "codex".into(),
            status: LinkStatus::Foreign,
            message: Some("目标已被占用".into()),
        });
        assert_eq!(report.total(), 2);
        assert!(!report.is_all_ok());
    }

    #[test]
    fn link_mode_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&LinkMode::Auto).unwrap(), "\"auto\"");
        assert_eq!(
            serde_json::to_string(&LinkMode::Symlink).unwrap(),
            "\"symlink\""
        );
    }

    #[test]
    fn origin_roundtrips_with_tag() {
        let o = SkillOrigin::InPlace {
            owner_agent: "claude-code".into(),
        };
        let json = serde_json::to_string(&o).unwrap();
        assert!(json.contains("\"kind\":\"inPlace\""), "{json}");
        assert!(json.contains("ownerAgent"), "{json}");
        let back: SkillOrigin = serde_json::from_str(&json).unwrap();
        assert_eq!(back, o);
    }
}
