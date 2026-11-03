//! 编排层：把 scanner / detector / linker / store / native_toggle 串起来，
//! 对上暴露 Tauri 命令直接需要的粗粒度操作。

use crate::fs::paths;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::models::agent::{AgentInfo, AGENTS};
use crate::models::config::{AppConfig, Registration, SkillProvenance};
use crate::models::skill::{
    skill_id_for, LinkMode, LinkReport, LinkResult, LinkStatus, Skill, SkillOrigin,
};
use crate::services::scanner::{EntryKind, Frontmatter};
use crate::services::{detector, linker, native_toggle, scanner, store::Store, tokens};

/// 某个 skill 在某个 agent 上的状态（给前端）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkillState {
    pub status: LinkStatus,
    /// 实际检查到的位置。未注册时是"将会写入"的位置。
    pub target_path: PathBuf,
    /// 通过 agent 原生配置停用（文件仍在）
    pub disabled: bool,
    #[serde(default)]
    pub manual: bool,
    #[serde(default)]
    pub policy_blocked: bool,
    /// 原生停用由 Studio 的组外策略记录；不是仅凭开关推测。
    #[serde(default)]
    pub policy_suspended: bool,
    pub mode: Option<LinkMode>,
    /// 同一来源在该 agent 下的全部入口（含别名）。
    pub entry_paths: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

/// 前端拿到的 skill 视图
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillView {
    #[serde(flatten)]
    pub skill: Skill,
    /// agent_id -> 状态
    pub agents: HashMap<String, AgentSkillState>,
    pub group_ids: Vec<String>,
    /// frontmatter 解析失败（YAML 坏了）
    pub malformed_frontmatter: bool,
    pub frontmatter_error: Option<String>,
    pub diagnostics: Vec<String>,
    pub source_ids: Vec<String>,
    pub provenance: Option<SkillProvenance>,
    pub installation: Option<crate::models::config::SkillInstallation>,
}

type RootScans = HashMap<(&'static str, PathBuf), RootScan>;

/// These indexes belong to one read-only scan; write-time ownership checks never use them.
struct RootScan {
    entries: Vec<scanner::ScannedEntry>,
    literal: HashMap<String, Vec<usize>>,
    physical: HashMap<PathBuf, Vec<usize>>,
}

impl RootScan {
    fn new(entries: Vec<scanner::ScannedEntry>) -> Self {
        let mut scan = Self {
            entries,
            literal: HashMap::new(),
            physical: HashMap::new(),
        };
        for (index, entry) in scan.entries.iter().enumerate() {
            let target = match &entry.kind {
                EntryKind::Link {
                    target: Some(target),
                } => Some(paths::normalize_path_lexically(&if target.is_absolute() {
                    target.clone()
                } else {
                    entry.path.parent().unwrap().join(target)
                })),
                _ => None,
            };
            let candidates: Vec<&Path> = match &entry.kind {
                EntryKind::RealSkill => vec![entry.path.as_path()],
                EntryKind::ManagedCopy { source_path, .. } => vec![source_path.as_path()],
                EntryKind::Link { .. } => std::iter::once(entry.path.as_path())
                    .chain(target.as_deref())
                    .collect(),
                EntryKind::NotASkill => vec![],
            };
            for path in candidates {
                scan.literal
                    .entry(paths::comparable_path_key(path))
                    .or_default()
                    .push(index);
                if let Ok(path) = path.canonicalize() {
                    scan.physical.entry(path).or_default().push(index);
                }
            }
        }
        scan
    }

    fn targets(&self, source: &Path) -> Vec<PathBuf> {
        let mut matches = self
            .literal
            .get(&paths::comparable_path_key(source))
            .cloned()
            .unwrap_or_default();
        if let Ok(source) = source.canonicalize() {
            if let Some(indices) = self.physical.get(&source) {
                matches.extend(indices);
            }
        }
        // Keep the same scan/name order even when both alias indexes match.
        matches.sort_unstable();
        matches.dedup();
        matches
            .into_iter()
            .map(|index| self.entries[index].path.clone())
            .collect()
    }
}

#[derive(Default)]
struct SourcePaths {
    literal: HashSet<String>,
    physical: HashSet<PathBuf>,
}
impl SourcePaths {
    fn insert(&mut self, path: &Path) -> bool {
        let literal = paths::comparable_path_key(path);
        let physical = path.canonicalize().ok();
        if self.literal.contains(&literal)
            || physical
                .as_ref()
                .is_some_and(|path| self.physical.contains(path))
        {
            return false;
        }
        self.literal.insert(literal);
        if let Some(path) = physical {
            self.physical.insert(path);
        }
        true
    }
}

#[derive(Default)]
struct ScanSnapshot {
    roots: RootScans,
    variants: HashMap<(String, PathBuf), Option<String>>,
    toggles: native_toggle::ToggleSnapshot,
}
impl ScanSnapshot {
    fn unavailable(
        &mut self,
        config: &AppConfig,
        skill: &Skill,
        agent: &str,
        identity: &Result<Skill>,
    ) -> Option<String> {
        let source = match identity {
            Ok(source) => source,
            Err(error) => return Some(error.to_string()),
        };
        self.variants
            .entry((skill.id.clone(), source.source_path.clone()))
            .or_insert_with(|| {
                super::variants::for_agent(config, skill, agent)
                    .err()
                    .map(|e| e.to_string())
            })
            .clone()
    }
}

pub struct Studio {
    store: Store,
}

impl Studio {
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn load_config(&self) -> Result<AppConfig> {
        let config = self.store.load()?;
        let mut roots: Vec<PathBuf> = AGENTS
            .iter()
            .flat_map(|a| a.resolved_global_roots(&config.settings.agent_dir_overrides))
            .collect();
        for project in &config.projects {
            for agent in AGENTS {
                if let Some(root) = agent.project_root(&project.root) {
                    roots.push(root);
                }
            }
        }
        for regs in config.registrations.values() {
            for reg in regs.values() {
                if let Some(parent) = reg.target_path.parent() {
                    roots.push(parent.to_path_buf());
                }
            }
        }
        for root in roots {
            linker::recover_replacements(&root)?;
        }
        Ok(config)
    }

    pub fn save_config(&self, config: &AppConfig) -> Result<()> {
        self.store.save(config)
    }

    pub fn agents(&self, config: &AppConfig, skip_cli_probe: bool) -> Vec<AgentInfo> {
        detector::describe_all(&config.settings.agent_dir_overrides, skip_cli_probe)
    }

    fn scan_snapshot(&self, config: &AppConfig) -> Result<ScanSnapshot> {
        let mut snapshot = ScanSnapshot::default();
        for agent in AGENTS {
            for root in agent.resolved_global_roots(&config.settings.agent_dir_overrides) {
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    snapshot.roots.entry((agent.id, root.clone()))
                {
                    entry.insert(RootScan::new(scanner::scan_root(&root, agent)?));
                }
            }
        }
        let hub = self.store.hub_dir(config);
        if let std::collections::hash_map::Entry::Vacant(entry) =
            snapshot.roots.entry((AGENTS[0].id, hub.clone()))
        {
            entry.insert(RootScan::new(scanner::scan_root(&hub, &AGENTS[0])?));
        }
        Ok(snapshot)
    }

    /// Discover every identity, but only read/hash bodies for requested IDs on a lookup.
    fn scan_sources(
        &self,
        config: &AppConfig,
        scans: &RootScans,
        wanted: Option<&HashSet<&str>>,
    ) -> Result<HashMap<String, Skill>> {
        let overrides = &config.settings.agent_dir_overrides;
        let mut sources: HashMap<String, Skill> = HashMap::new();
        let mut seen = SourcePaths::default();
        let build = |id: String,
                     name: &str,
                     source: PathBuf,
                     root: PathBuf,
                     origin: SkillOrigin,
                     fm: Option<Frontmatter>| {
            let content = wanted.is_none_or(|ids| ids.contains(id.as_str()));
            build_skill_with_content(id, name, source, root, origin, fm, content)
        };

        for agent in AGENTS {
            for root in agent.resolved_global_roots(overrides) {
                let scan = scans
                    .get(&(agent.id, root.clone()))
                    .ok_or_else(|| Error::invalid("扫描目录已变化，请重试"))?;
                for entry in &scan.entries {
                    if !matches!(entry.kind, EntryKind::RealSkill) {
                        continue;
                    }
                    if !seen.insert(&entry.path) {
                        continue;
                    }
                    let id = skill_id_for(&entry.path);
                    sources.entry(id.clone()).or_insert_with(|| {
                        build(
                            id.clone(),
                            &entry.name,
                            entry.path.clone(),
                            root.clone(),
                            SkillOrigin::InPlace {
                                owner_agent: agent.id.to_string(),
                            },
                            entry.frontmatter.clone(),
                        )
                    });
                }
            }
        }

        // Hub 目录：用任一 agent 的保留名规则都不合适，这里用 Claude 的（含 synced）
        // 只是为了复用扫描逻辑；Hub 里本不该有 synced。
        let hub = self.store.hub_dir(config);
        let claude = &AGENTS[0];
        let hub_scan = scans
            .get(&(claude.id, hub.clone()))
            .ok_or_else(|| Error::invalid("Hub 目录已变化，请重试"))?;
        for entry in &hub_scan.entries {
            if !matches!(entry.kind, EntryKind::RealSkill) {
                continue;
            }
            if !seen.insert(&entry.path) {
                continue;
            }
            let id = skill_id_for(&entry.path);
            sources.entry(id.clone()).or_insert_with(|| {
                build(
                    id.clone(),
                    &entry.name,
                    entry.path.clone(),
                    hub.clone(),
                    SkillOrigin::Hub,
                    entry.frontmatter.clone(),
                )
            });
        }

        // Only explicitly recorded Local directories are rediscovered after release.
        for (id, record) in &config.skill_provenance {
            if matches!(record.original_origin, SkillOrigin::Local)
                && *id == skill_id_for(&record.original_path)
                && !scanner::is_symlink_or_junction(&record.original_path)
                && record.original_path.join(scanner::SKILL_FILE).is_file()
            {
                sources.entry(id.clone()).or_insert_with(|| {
                    build(
                        id.clone(),
                        record
                            .original_path
                            .file_name()
                            .unwrap()
                            .to_str()
                            .unwrap_or("skill"),
                        record.original_path.clone(),
                        record.original_root.clone(),
                        SkillOrigin::Local,
                        Some(scanner::parse_frontmatter(
                            &record.original_path.join(scanner::SKILL_FILE),
                        )),
                    )
                });
            }
        }

        // A bundle's preview is not its identity. Recover known installations
        // even when their representative SKILL.md has been removed.
        if hub.is_dir() {
            for entry in std::fs::read_dir(&hub).map_err(|e| Error::io(&hub, e))? {
                let entry = entry.map_err(|e| Error::io(&hub, e))?;
                let path = entry.path();
                if !entry.file_type().map_err(|e| Error::io(&path, e))?.is_dir() {
                    continue;
                }
                let id = skill_id_for(&path);
                if config
                    .skill_installations
                    .get(&id)
                    .is_some_and(|i| !i.variants.is_empty())
                {
                    sources.entry(id.clone()).or_insert_with(|| {
                        build(
                            id,
                            &entry.file_name().to_string_lossy(),
                            path,
                            hub.clone(),
                            SkillOrigin::Hub,
                            None,
                        )
                    });
                }
            }
        }

        // Resolve links only after real sources, so an alias cannot change an
        // existing source's ID or ownership. Never recurse through directory links.
        let mut identities: HashMap<PathBuf, String> = sources
            .values()
            .map(|skill| {
                (
                    skill
                        .source_path
                        .canonicalize()
                        .unwrap_or_else(|_| skill.source_path.clone()),
                    skill.id.clone(),
                )
            })
            .collect();
        // Internal payload links retain their logical Hub identity.
        for skill in sources.values() {
            for variant in super::variants::entries(config, skill) {
                if variant.key != super::variants::GENERIC
                    && !AGENTS.iter().any(|a| a.id == variant.key)
                {
                    continue;
                }
                // Record the expected entry without following a replaced payload
                // symlink into an unrelated external directory.
                for root in [
                    skill.source_path.clone(),
                    skill
                        .source_path
                        .canonicalize()
                        .unwrap_or_else(|_| skill.source_path.clone()),
                ] {
                    identities.insert(
                        root.join(super::variants::DIRECTORY).join(&variant.key),
                        skill.id.clone(),
                    );
                }
                if let Ok(path) = super::variants::payload(&skill.source_path, &variant.key) {
                    identities.insert(path.canonicalize().unwrap_or(path), skill.id.clone());
                }
            }
        }
        for agent in AGENTS {
            for root in agent.resolved_global_roots(overrides) {
                let scan = scans
                    .get(&(agent.id, root.clone()))
                    .ok_or_else(|| Error::invalid("扫描目录已变化，请重试"))?;
                for entry in &scan.entries {
                    if !matches!(entry.kind, EntryKind::Link { .. }) {
                        continue;
                    }
                    if scanner::link_destination(&entry.path)
                        .is_ok_and(|p| identities.contains_key(&p))
                    {
                        continue;
                    }
                    let source = entry.path.canonicalize().unwrap_or_else(|_| {
                        scanner::link_destination(&entry.path)
                            .unwrap_or_else(|_| entry.path.clone())
                    });
                    if identities.contains_key(&source) {
                        continue;
                    }
                    let id = skill_id_for(&source);
                    identities.insert(source.clone(), id.clone());
                    let name = source
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&entry.name)
                        .to_string();
                    sources.insert(
                        id.clone(),
                        build(
                            id,
                            &name,
                            source,
                            root.clone(),
                            SkillOrigin::External,
                            entry.frontmatter.clone(),
                        ),
                    );
                }
            }
        }

        Ok(sources)
    }

    /// Sources come from real Agent directories and the Hub, not deployment copies.
    pub fn scan_skills(&self, config: &AppConfig) -> Result<Vec<SkillView>> {
        let mut snapshot = self.scan_snapshot(config)?;
        let sources = self.scan_sources(config, &snapshot.roots, None)?;
        // 来源标注使用的共享根在一轮扫描里只解析一次，全部 skill 复用同一个切片。
        let roots = shared_roots(config);
        let mut views: Vec<SkillView> = sources
            .into_values()
            .map(|skill| {
                let agents = self.agent_states_using(config, &skill, Some(&mut snapshot));
                let group_ids = config
                    .groups_containing(&skill.id)
                    .into_iter()
                    .map(|g| g.id.clone())
                    .collect();
                let fm = scanner::parse_frontmatter(&skill.source_path.join(scanner::SKILL_FILE));
                let diagnostics = if super::variants::entries(config, &skill).is_empty()
                    && !skill.source_path.join(scanner::SKILL_FILE).is_file()
                {
                    vec![match std::fs::metadata(&skill.source_path) {
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            "目标目录不存在".into()
                        }
                        Err(e) => format!("无法访问链接目标：{e}"),
                        Ok(meta) if !meta.is_dir() => "链接目标不是目录".into(),
                        Ok(_) => {
                            match std::fs::metadata(skill.source_path.join(scanner::SKILL_FILE)) {
                                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                                    "目标目录中缺少 SKILL.md".into()
                                }
                                Err(e) => format!("无法读取 SKILL.md：{e}"),
                                Ok(_) => "目标中的 SKILL.md 不是普通文件".into(),
                            }
                        }
                    }]
                } else {
                    vec![]
                };
                let provenance = config.skill_provenance.get(&skill.id).cloned();
                let installation = config.skill_installations.get(&skill.id).cloned();
                let source_ids = if config.builtin_skills.contains_key(&skill.id) {
                    vec!["builtin".into()]
                } else if installation.is_some() {
                    vec!["studio".into()]
                } else {
                    provenance
                        .as_ref()
                        .map(|p| p.source_ids.clone())
                        .unwrap_or_else(|| original_source_ids(config, &skill, &agents, &roots))
                };
                SkillView {
                    source_ids,
                    provenance,
                    installation,
                    malformed_frontmatter: fm.malformed,
                    frontmatter_error: fm.error,
                    diagnostics,
                    skill,
                    agents,
                    group_ids,
                }
            })
            .collect();

        views.sort_by(|a, b| a.skill.name.cmp(&b.skill.name));
        Ok(views)
    }

    fn agent_targets(
        &self,
        config: &AppConfig,
        skill: &Skill,
        agent: &crate::models::agent::AgentDescriptor,
    ) -> Vec<PathBuf> {
        self.agent_targets_from(config, skill, agent, None)
    }

    fn agent_targets_from(
        &self,
        config: &AppConfig,
        skill: &Skill,
        agent: &crate::models::agent::AgentDescriptor,
        scans: Option<&RootScans>,
    ) -> Vec<PathBuf> {
        let mut targets = Vec::new();
        if let Some(reg) = config.registration(&skill.id, agent.id) {
            targets.push(reg.target_path.clone());
        }
        for root in agent.resolved_global_roots(&config.settings.agent_dir_overrides) {
            targets.push(root.join(&skill.name));
            if let Some(record) = config.skill_provenance.get(&skill.id) {
                targets.extend(
                    record
                        .entry_paths
                        .iter()
                        .filter(|path| {
                            path.parent()
                                .is_some_and(|parent| crate::fs::paths::paths_alias(parent, &root))
                                && linker::link_status(&skill.source_path, path).is_registered()
                        })
                        .cloned(),
                );
            }
            if let Some(scan) = scans.and_then(|scans| scans.get(&(agent.id, root.clone()))) {
                targets.extend(scan.targets(&skill.source_path));
            } else if let Ok(entries) = scanner::scan_root(&root, agent) {
                for entry in entries {
                    let matches = match entry.kind {
                        EntryKind::Link { .. } => {
                            crate::fs::paths::paths_alias(&entry.path, &skill.source_path)
                                || scanner::link_destination(&entry.path).is_ok_and(|p| {
                                    crate::fs::paths::paths_alias(&p, &skill.source_path)
                                })
                        }
                        EntryKind::ManagedCopy { source_path, .. } => {
                            crate::fs::paths::paths_alias(&source_path, &skill.source_path)
                        }
                        EntryKind::RealSkill => {
                            crate::fs::paths::paths_alias(&entry.path, &skill.source_path)
                        }
                        _ => false,
                    };
                    if matches {
                        targets.push(entry.path);
                    }
                }
            }
        }
        let mut seen = HashSet::new();
        targets.retain(|p| {
            seen.insert(
                p.parent()
                    .and_then(|parent| parent.canonicalize().ok())
                    .unwrap_or_default()
                    .join(p.file_name().unwrap_or_default()),
            )
        });
        targets
    }

    /// 计算一个 skill 在所有 agent 上的状态。
    ///
    /// 会遍历该 agent 的**全部**全局根，而不是只看写入根 —— 否则用户手工放到
    /// `~/.agents/skills` 的 skill 会被误报成未注册。
    fn agent_states(&self, config: &AppConfig, skill: &Skill) -> HashMap<String, AgentSkillState> {
        self.agent_states_using(config, skill, None)
    }

    fn agent_states_using(
        &self,
        config: &AppConfig,
        skill: &Skill,
        mut snapshot: Option<&mut ScanSnapshot>,
    ) -> HashMap<String, AgentSkillState> {
        let overrides = &config.settings.agent_dir_overrides;
        let mut out = HashMap::new();

        for agent in AGENTS {
            let identity = super::variants::deployment_source(config, skill, agent.id);
            let unavailable = match snapshot.as_deref_mut() {
                Some(snapshot) => snapshot.unavailable(config, skill, agent.id, &identity),
                None => super::variants::for_agent(config, skill, agent.id)
                    .err()
                    .map(|e| e.to_string()),
            };
            let skill = identity.as_ref().unwrap_or(skill);
            let roots = agent.resolved_global_roots(overrides);
            let primary = roots.first().cloned().unwrap_or_default();
            let mut best: Option<(LinkStatus, PathBuf)> = None;

            let targets = self.agent_targets_from(
                config,
                skill,
                agent,
                snapshot.as_deref().map(|snapshot| &snapshot.roots),
            );
            let entry_paths: Vec<PathBuf> = targets
                .iter()
                .filter(|p| p.symlink_metadata().is_ok())
                .cloned()
                .collect();
            for dest in targets {
                // 真身就在这个 root 里
                let status = if dest.join(scanner::SKILL_FILE).is_file()
                    && !scanner::is_symlink_or_junction(&dest)
                    && crate::fs::paths::paths_alias(&dest, &skill.source_path)
                {
                    LinkStatus::Source
                } else {
                    let status = linker::link_status(&skill.source_path, &dest);
                    if status == LinkStatus::Linked && !dest.join(scanner::SKILL_FILE).is_file() {
                        LinkStatus::BrokenLink
                    } else {
                        status
                    }
                };
                if status == LinkStatus::NotLinked {
                    continue;
                }
                // 优先展示"能用"的状态；其次才是需要关注的异常
                let replace = match &best {
                    None => true,
                    Some((prev, _)) => !prev.is_registered() && status.is_registered(),
                };
                if replace {
                    best = Some((status, dest));
                }
            }

            let (status, target_path) =
                best.unwrap_or((LinkStatus::NotLinked, primary.join(&skill.name)));
            // The representative root is not a deployable fallback for a missing
            // variant, even if someone manually linked it into this Agent.
            let status = if identity.is_err() && status.is_registered() {
                LinkStatus::Conflict
            } else {
                status
            };
            let document = target_path.join(scanner::SKILL_FILE);
            let declared = scanner::parse_frontmatter(&document)
                .name
                .unwrap_or_else(|| skill.name.clone());
            let toggle_name = if agent.id == "codex" {
                &declared
            } else {
                target_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&skill.name)
            };
            let disabled = status.is_registered()
                && match snapshot.as_deref_mut() {
                    Some(snapshot) => {
                        snapshot
                            .toggles
                            .is_disabled(agent, overrides, toggle_name, &document)
                    }
                    None => native_toggle::is_skill_disabled_at(
                        agent,
                        overrides,
                        toggle_name,
                        &document,
                    ),
                };
            let mode = config.registration(&skill.id, agent.id).map(|r| r.mode);

            out.insert(
                agent.id.to_string(),
                AgentSkillState {
                    policy_suspended: disabled
                        && config
                            .policy_suspensions
                            .get(agent.id)
                            .into_iter()
                            .flatten()
                            .any(|entry| {
                                entry.skill_id == skill.id
                                    && crate::fs::paths::paths_alias(
                                        &entry.source_path,
                                        &skill.source_path,
                                    )
                                    && (crate::fs::paths::paths_alias(
                                        &entry.target_path,
                                        &skill.source_path,
                                    ) || linker::link_status(
                                        &skill.source_path,
                                        &entry.target_path,
                                    )
                                    .is_registered())
                            }),
                    unavailable_reason: unavailable,
                    status,
                    target_path,
                    policy_blocked: agent.toggle_config_path(overrides).is_some()
                        && !config.settings.preserve_manual_skills
                        && !config
                            .settings
                            .disabled_agents
                            .iter()
                            .any(|id| id == agent.id)
                        && !config
                            .active_groups_of(agent.id)
                            .iter()
                            .any(|g| g.skill_ids.contains(&skill.id)),
                    manual: status.is_registered()
                        && entry_paths.iter().any(|target| {
                            if config
                                .active_groups_of(agent.id)
                                .iter()
                                .any(|g| g.entries.iter().any(|e| e.target_path == *target))
                            {
                                return false;
                            }
                            if config.skill_provenance.get(&skill.id).is_some_and(|p| {
                                p.original_path == *target || p.entry_paths.contains(target)
                            }) {
                                return true;
                            }
                            !config
                                .registration(&skill.id, agent.id)
                                .is_some_and(|r| r.target_path == *target)
                        }),
                    disabled,
                    mode,
                    entry_paths,
                },
            );
        }
        out
    }

    /// Preserve the preview/open allowlist without hashing bodies or building every state.
    pub fn knows_skill_path(&self, config: &AppConfig, path: &Path) -> Result<bool> {
        let snapshot = self.scan_snapshot(config)?;
        let sources = self.scan_sources(config, &snapshot.roots, Some(&HashSet::new()))?;
        for skill in sources.values() {
            if skill.source_path == path {
                return Ok(true);
            }
            for agent in AGENTS {
                let identity = super::variants::deployment_source(config, skill, agent.id);
                let source = identity.as_ref().unwrap_or(skill);
                let targets = self.agent_targets_from(config, source, agent, Some(&snapshot.roots));
                let mut present = false;
                for target in targets {
                    if target.symlink_metadata().is_ok() {
                        if target == path {
                            return Ok(true);
                        }
                        present = true;
                    }
                }
                let primary = agent
                    .resolved_global_roots(&config.settings.agent_dir_overrides)
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                if !present && primary.join(&source.name) == path {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// 按 ID 找一个 skill 真身
    pub fn find_skill(&self, config: &AppConfig, skill_id: &str) -> Result<Skill> {
        let snapshot = self.scan_snapshot(config)?;
        self.scan_sources(config, &snapshot.roots, Some(&HashSet::from([skill_id])))?
            .remove(skill_id)
            .ok_or_else(|| Error::NotFound(format!("skill {skill_id}")))
    }

    /// 把若干 skill 注册到若干 agent。
    ///
    /// 单条失败不影响其余，全部结果汇总在 `LinkReport` 里。
    pub fn register(
        &self,
        config: &mut AppConfig,
        skill_ids: &[String],
        agent_ids: &[String],
        mode: Option<LinkMode>,
        force: bool,
    ) -> Result<LinkReport> {
        if agent_ids
            .iter()
            .any(|id| config.settings.disabled_agents.contains(id))
        {
            return Err(Error::invalid("请先在设置中开启目标 Agent 的管理"));
        }
        for id in agent_ids {
            crate::models::agent::require_agent(id)?;
        }
        let skills = self.resolve_skills(config, skill_ids)?;
        let mode = mode.unwrap_or(config.settings.default_link_mode);
        let overrides = config.settings.agent_dir_overrides.clone();
        let mut report = LinkReport::default();

        for agent_id in agent_ids {
            let agent = crate::models::agent::require_agent(agent_id)?;
            let root = agent.primary_global_root(&overrides);
            for family in &skills {
                let resolved = match super::variants::for_agent(config, family, agent_id) {
                    Ok(skill) => skill,
                    Err(error) => {
                        report.push_err(result(
                            family,
                            agent_id,
                            LinkStatus::NotLinked,
                            Some(error.to_string()),
                        ));
                        continue;
                    }
                };
                let skill = &resolved;
                if let Err(error) = scanner::validate_agent_sync_source(&skill.source_path, agent) {
                    report.push_err(result(
                        skill,
                        agent_id,
                        LinkStatus::NotLinked,
                        Some(error.to_string()),
                    ));
                    continue;
                }
                let dest = self
                    .agent_targets(config, skill, agent)
                    .into_iter()
                    .find(|p| {
                        crate::fs::paths::paths_alias(p, &skill.source_path)
                            || matches!(
                                linker::link_status(&skill.source_path, p),
                                LinkStatus::Copied
                                    | LinkStatus::CopyStale
                                    | LinkStatus::CopyModified
                                    | LinkStatus::CopyConflict
                                    | LinkStatus::CopyDamaged
                            )
                    })
                    .unwrap_or_else(|| root.join(&skill.name));
                // 真身就在这儿，没什么可注册的
                if crate::fs::paths::is_same_path(&dest, &skill.source_path) {
                    report.push_ok(result(skill, agent_id, LinkStatus::Source, None));
                    continue;
                }
                let registration = (|| {
                    let (done, mut tx) = linker::prepare_registration(
                        &skill.source_path,
                        &dest,
                        mode,
                        &skill.id,
                        force,
                        self.store().dir().join("registration.json"),
                    )?;
                    let mut next = config.clone();
                    next.set_registration(
                        &skill.id,
                        agent_id,
                        Registration {
                            mode: done.mode,
                            target_path: dest.clone(),
                            registered_at: linker::now_secs(),
                            source_hash_at_copy: done.source_hash.clone(),
                        },
                    );
                    if let Err(error) = self.store().save_in_transaction(&mut tx, &next) {
                        if let Err(rollback) = tx.rollback() {
                            return Err(Error::Other(format!("{error}; 回滚失败: {rollback}")));
                        }
                        return Err(error);
                    }
                    tx.commit()?;
                    *config = next;
                    Ok(done)
                })();
                match registration {
                    Ok(done) => {
                        report.push_ok(result(skill, agent_id, done.status, None));
                    }
                    Err(err) => report.push_err(result(
                        skill,
                        agent_id,
                        linker::link_status(&skill.source_path, &dest),
                        Some(err.to_string()),
                    )),
                }
            }
        }
        Ok(report)
    }

    /// 取消注册。只移除本工具管理的内容，`Foreign` 会以失败条目返回。
    pub fn unregister(
        &self,
        config: &mut AppConfig,
        skill_ids: &[String],
        agent_ids: &[String],
        force: bool,
    ) -> Result<LinkReport> {
        if agent_ids
            .iter()
            .any(|id| config.settings.disabled_agents.contains(id))
        {
            return Err(Error::invalid("请先在设置中开启目标 Agent 的管理"));
        }
        let skills = self.resolve_skills(config, skill_ids)?;
        let mut plan = Vec::new();
        let mut group_owned = Vec::new();

        for agent_id in agent_ids {
            let agent = crate::models::agent::require_agent(agent_id)?;
            for family in &skills {
                let resolved = super::variants::deployment_source(config, family, agent_id)?;
                let skill = &resolved;
                // 遍历全部根，把散落在共享根里的注册也清掉
                for dest in self.agent_targets(config, skill, agent) {
                    if !scanner::is_symlink_or_junction(&dest)
                        && crate::fs::paths::paths_alias(&dest, &skill.source_path)
                    {
                        // 不能把真身当注册删掉
                        continue;
                    }
                    // 分组部署的副本归分组管：在这里删掉会让启用记录指向一个不存在的目录。
                    // force 也不放行 —— 停用分组才是撤回它的唯一入口。
                    if config
                        .active_entries(agent_id)
                        .iter()
                        .any(|e| crate::fs::paths::paths_alias(&e.target_path, &dest))
                    {
                        group_owned.push(result(
                            skill,
                            agent_id,
                            linker::link_status(&skill.source_path, &dest),
                            Some("由分组部署，请在分组页停用".into()),
                        ));
                        continue;
                    }
                    plan.push((skill.clone(), agent_id.clone(), dest));
                }
            }
        }
        let mut report = self.remove_deployments(config, plan, force, None)?;
        for refused in group_owned {
            report.push_err(refused);
        }
        Ok(report)
    }

    /// Remove files and their durable ownership records in one undo journal.
    fn remove_deployments(
        &self,
        config: &mut AppConfig,
        plan: Vec<(Skill, String, PathBuf)>,
        force: bool,
        project: Option<&str>,
    ) -> Result<LinkReport> {
        use super::transaction::Transaction;
        let mut next = config.clone();
        let mut report = LinkReport::default();
        let mut tx = Transaction::begin(self.store().dir().join("skill-unregister.json"))?;
        let operation = (|| -> Result<()> {
            let mut removed = HashSet::new();
            for (skill, agent, dest) in plan {
                let status = linker::link_status(&skill.source_path, &dest);
                if !force
                    && matches!(
                        status,
                        LinkStatus::Foreign
                            | LinkStatus::Conflict
                            | LinkStatus::CopyModified
                            | LinkStatus::CopyConflict
                            | LinkStatus::CopyDamaged
                    )
                {
                    report.push_err(result(
                        &skill,
                        &agent,
                        status,
                        Some("部署内容已修改或归属不匹配，未删除".into()),
                    ));
                    continue;
                }
                if status != LinkStatus::NotLinked && removed.insert(dest.clone()) {
                    tx.reserve(&dest)?;
                }
                if let Some(id) = project {
                    if let Some(p) = next.projects.iter_mut().find(|p| p.id == id) {
                        p.managed_entries.retain(|e| e.target_path != dest);
                    }
                } else if next
                    .registration(&skill.id, &agent)
                    .is_some_and(|r| r.target_path == dest)
                {
                    next.remove_registration(&skill.id, &agent);
                }
                if status != LinkStatus::NotLinked {
                    report.push_ok(result(&skill, &agent, LinkStatus::NotLinked, None));
                }
            }
            self.store().save_in_transaction(&mut tx, &next)?;
            Ok(())
        })();
        if let Err(error) = operation {
            tx.rollback()?;
            return Err(error);
        }
        tx.commit()?;
        *config = next;
        Ok(report)
    }

    /// 应用项目绑定：把项目里的 skill（含所绑分组）写进该项目各 agent 的目录。
    ///
    /// 项目级默认 Copy —— symlink 进 git 只是个指向本机绝对路径的死链。
    pub fn apply_project(&self, config: &mut AppConfig, project_id: &str) -> Result<LinkReport> {
        self.write_project(config, project_id, None)
    }

    /// 从项目里移除某些 skill 的注册
    pub fn unapply_project(
        &self,
        config: &mut AppConfig,
        project_id: &str,
        skill_ids: &[String],
        force: bool,
    ) -> Result<LinkReport> {
        let project = config
            .project(project_id)
            .ok_or_else(|| Error::NotFound(format!("项目 {project_id}")))?
            .clone();
        if project
            .agent_ids
            .iter()
            .any(|id| config.settings.disabled_agents.contains(id))
        {
            return Err(Error::invalid("项目包含已退出管理的 Agent"));
        }
        let skills = self.resolve_skills(config, skill_ids)?;
        let mut plan = Vec::new();

        for agent_id in &project.agent_ids {
            let agent = crate::models::agent::require_agent(agent_id)?;
            let Some(root) = agent.project_root(&project.root) else {
                continue;
            };
            for family in &skills {
                let mut skill = super::variants::deployment_source(config, family, agent_id)?;
                let dest = root.join(&skill.name);
                if let Some(entry) = project
                    .managed_entries
                    .iter()
                    .find(|e| e.skill_id == skill.id && e.target_path == dest)
                {
                    skill.source_path = entry.source_path.clone();
                }
                plan.push((skill, agent_id.clone(), dest));
            }
        }
        self.remove_deployments(config, plan, force, Some(project_id))
    }

    /// 通过 agent 原生配置启停某个 skill（不动文件）
    pub fn set_skill_enabled(
        &self,
        config: &AppConfig,
        skill_id: &str,
        agent_id: &str,
        enabled: bool,
    ) -> Result<()> {
        if config
            .settings
            .disabled_agents
            .iter()
            .any(|id| id == agent_id)
        {
            return Err(Error::invalid("请先在设置中开启此 Agent 的管理"));
        }
        if enabled
            && !config.settings.preserve_manual_skills
            && !config
                .active_groups_of(agent_id)
                .iter()
                .any(|g| g.skill_ids.iter().any(|id| id == skill_id))
        {
            return Err(Error::invalid(
                "当前不保留组外 skill，请先开启保留开关或启用包含此 skill 的分组",
            ));
        }
        let skill = self.find_skill(config, skill_id)?;
        let agent = crate::models::agent::require_agent(agent_id)?;
        let state = self
            .agent_states(config, &skill)
            .remove(agent_id)
            .ok_or_else(|| Error::invalid("未知 agent"))?;
        if !state.status.is_registered() {
            return Err(Error::invalid("该 skill 在目标 agent 上不可用"));
        }
        let skill = super::variants::deployment_source(config, &skill, agent_id)?;
        for target in state.entry_paths {
            if !linker::link_status(&skill.source_path, &target).is_registered()
                && !crate::fs::paths::paths_alias(&target, &skill.source_path)
            {
                continue;
            }
            let document = target.join(scanner::SKILL_FILE);
            let name = if agent_id == "codex" {
                scanner::parse_frontmatter(&document)
                    .name
                    .unwrap_or_else(|| skill.name.clone())
            } else {
                target.file_name().unwrap().to_string_lossy().into_owned()
            };
            native_toggle::set_skill_enabled_at(
                agent,
                &config.settings.agent_dir_overrides,
                &name,
                &document,
                enabled,
            )?;
        }
        Ok(())
    }

    /// 把一个原地 skill 托管到 Hub：移动真身，并把原位置改成指向 Hub 的注册。
    ///
    /// 移动前会校验 Hub 与所有 agent 目录不重叠，且目标不存在。
    pub fn adopt_to_hub(&self, config: &mut AppConfig, skill_id: &str) -> Result<Skill> {
        let skill = self.find_skill(config, skill_id)?;
        self.relocate_hub_skill(config, skill, false)
    }

    pub fn release_from_hub(&self, config: &mut AppConfig, skill_id: &str) -> Result<Skill> {
        if config
            .skill_installations
            .get(skill_id)
            .is_some_and(|i| !i.variants.is_empty())
        {
            return Err(Error::invalid(
                "多端安装由 Hub 统一管理，不能还原为单个 Agent 来源",
            ));
        }
        let skill = self.find_skill(config, skill_id)?;
        self.relocate_hub_skill(config, skill, true)
    }

    pub(super) fn relocate_hub_skill(
        &self,
        config: &mut AppConfig,
        skill: Skill,
        restore: bool,
    ) -> Result<Skill> {
        if !restore && matches!(skill.origin, SkillOrigin::Hub) {
            return Ok(skill);
        }
        let skill_id = skill.id.as_str();
        let using: Vec<String> = AGENTS
            .iter()
            .flat_map(|agent| {
                config
                    .active_groups_of(agent.id)
                    .iter()
                    .filter(|group| group.skill_ids.iter().any(|id| id == skill_id))
                    .map(|group| {
                        let name = config
                            .group(&group.group_id)
                            .map(|group| group.name.as_str())
                            .unwrap_or(&group.group_id);
                        format!("{} · {name}", agent.display_name)
                    })
            })
            .collect();
        if !using.is_empty() {
            return Err(Error::invalid(format!(
                "此 skill 正被启用分组使用：{}；请先停用这些分组，再迁移",
                using.join("、")
            )));
        }
        for id in &config.settings.disabled_agents {
            let agent = crate::models::agent::require_agent(id)?;
            let roots = agent.resolved_global_roots(&config.settings.agent_dir_overrides);
            let record = config.skill_provenance.get(skill_id);
            let touches_disabled = std::iter::once(&skill.source_path)
                .chain(record.into_iter().flat_map(|p| {
                    p.entry_paths
                        .iter()
                        .chain(std::iter::once(&p.original_path))
                }))
                .any(|path| {
                    path.parent().is_some_and(|parent| {
                        roots
                            .iter()
                            .any(|root| crate::fs::paths::paths_alias(parent, root))
                    })
                });
            if touches_disabled {
                return Err(Error::invalid(format!(
                    "此 skill 涉及已退出管理的 {}，请先开启该应用再托管或移出 Hub",
                    agent.display_name
                )));
            }
        }
        if config.builtin_skills.contains_key(skill_id) {
            return Err(Error::invalid("内置 skill 不能迁移，请通过设置管理"));
        }
        if restore && !matches!(skill.origin, SkillOrigin::Hub) {
            return Err(Error::invalid("此 skill 不在 Hub 中"));
        }
        let saved = config.skill_provenance.get(skill_id).cloned();
        if restore && saved.is_none() {
            return Err(Error::invalid(
                "缺少原始来源与备份记录，不能自动还原；请先确认历史来源",
            ));
        }
        if matches!(skill.origin, SkillOrigin::External) {
            return Err(Error::invalid(
                "此入口仅引用外部来源；请明确选择真实源目录收录，不会复制或自动接管",
            ));
        }
        if !restore {
            super::collection::validate_source(&skill.source_path)?;
        }
        let hub = self.store.hub_dir(config);
        let overrides = config.settings.agent_dir_overrides.clone();
        let all_roots: Vec<PathBuf> = AGENTS
            .iter()
            .flat_map(|a| a.resolved_global_roots(&overrides))
            .collect();
        linker::ensure_distinct_roots(&hub, &all_roots)?;

        // Collection leaves the same entry in place as a symlink, including shared
        // and project roots. Native path/name switches therefore keep their state.
        std::fs::create_dir_all(&hub).map_err(|e| Error::io(&hub, e))?;
        let target = if restore {
            saved.as_ref().unwrap().original_path.clone()
        } else {
            hub.join(&skill.name)
        };
        scanner::validate_sync_source(&skill.source_path)?;
        let source_hash = scanner::dir_content_hash(&skill.source_path)?;
        let mut replace_imported = false;
        if restore {
            let parent = target
                .parent()
                .ok_or_else(|| Error::invalid("原始目录无效"))?;
            let resolved_parent = parent.canonicalize().map_err(|e| Error::io(parent, e))?;
            let resolved_target = resolved_parent.join(
                target
                    .file_name()
                    .ok_or_else(|| Error::invalid("原始目录无效"))?,
            );
            let resolved_source = skill
                .source_path
                .canonicalize()
                .map_err(|e| Error::io(&skill.source_path, e))?;
            if crate::fs::paths::path_is_within(&resolved_target, &resolved_source)
                || crate::fs::paths::path_is_within(&resolved_source, &resolved_target)
            {
                return Err(Error::invalid("原目录已重定向至 Hub，不能自动还原"));
            }
            if crate::fs::paths::path_is_within(&target, &skill.source_path)
                || crate::fs::paths::path_is_within(&skill.source_path, &target)
            {
                return Err(Error::invalid("还原目录与 Hub 来源重叠"));
            }
            if target.symlink_metadata().is_ok() {
                let status = linker::link_status(&skill.source_path, &target);
                if !matches!(
                    status,
                    LinkStatus::Linked | LinkStatus::Copied | LinkStatus::CopyStale
                ) {
                    return Err(Error::invalid(
                        "原位置已被占用或副本已修改，不能覆盖；请先处理冲突",
                    ));
                }
            }
        } else if target.symlink_metadata().is_ok() {
            // A verified, unedited snapshot previously imported from this exact
            // original can converge to one Hub identity. Never replace unrelated
            // installations, edited copies, or a redirected Hub directory.
            replace_imported = !scanner::is_symlink_or_junction(&target)
                && config
                    .skill_installations
                    .get(&skill_id_for(&target))
                    .is_some_and(|record| {
                        record.variants.is_empty()
                            && record.source.strip_prefix("local:").is_some_and(|source| {
                                crate::fs::paths::paths_alias(
                                    std::path::Path::new(source),
                                    &skill.source_path,
                                )
                            })
                            && record.content_hash == source_hash
                    })
                && scanner::dir_content_hash(&target)? == source_hash;
            if !replace_imported {
                return Err(Error::invalid(format!(
                    "Hub 里已存在同名 skill，未覆盖：{}；请先处理内容或来源冲突",
                    target.display()
                )));
            }
            if let (Some(original), Some(existing)) = (
                config
                    .skill_meta
                    .get(skill_id)
                    .and_then(|meta| meta.note.as_ref()),
                config
                    .skill_meta
                    .get(&skill_id_for(&target))
                    .and_then(|meta| meta.note.as_ref()),
            ) {
                if original != existing {
                    return Err(Error::invalid(
                        "原件与 Hub 条目有不同备注，请先合并备注再托管",
                    ));
                }
            }
        }

        // Discover affected entries before moving the source; shared roots may alias.
        let old_path = skill.source_path.clone();
        let new_id = skill_id_for(&target);
        let mut candidates: Vec<PathBuf> = all_roots.iter().map(|r| r.join(&skill.name)).collect();
        for agent in AGENTS {
            candidates.extend(self.agent_targets(config, &skill, agent));
        }
        if let Some(regs) = config.registrations.get(&skill.id) {
            candidates.extend(regs.values().map(|r| r.target_path.clone()));
        }
        for project in &config.projects {
            if !project.root.is_dir() {
                return Err(Error::invalid(format!(
                    "项目不可访问，无法检查迁移引用: {}",
                    project.root.display()
                )));
            }
            // Include all supported roots, including deployments whose selection was later changed.
            for agent in AGENTS {
                if let Some(root) = agent.project_root(&project.root) {
                    candidates.push(root.join(&skill.name));
                }
            }
        }
        if let Some(record) = &saved {
            candidates.extend(record.entry_paths.clone());
        }
        let mut seen = HashSet::new();
        let mut copies = Vec::new();
        let mut links = Vec::new();
        for dest in candidates {
            let parent = dest.parent().unwrap();
            let key = parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf())
                .join(dest.file_name().unwrap());
            if !seen.insert(key)
                || crate::fs::paths::is_same_path(&dest, &old_path)
                || (restore && crate::fs::paths::is_same_path(&dest, &target))
            {
                continue;
            }
            match std::fs::symlink_metadata(&dest) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(Error::io(&dest, e)),
                Ok(_) => {}
            }
            if scanner::is_symlink_or_junction(&dest) {
                if linker::link_status(&old_path, &dest) == LinkStatus::Linked {
                    links.push(dest);
                }
            } else if let Some(mut sidecar) = scanner::read_copy_sidecar(&dest) {
                if crate::fs::paths::paths_alias(&sidecar.source_path, &old_path) {
                    sidecar.source_path = target.clone();
                    sidecar.skill_id = new_id.clone();
                    copies.push((dest.join(scanner::COPY_SIDECAR), sidecar));
                }
            } else if dest.join(scanner::COPY_SIDECAR).symlink_metadata().is_ok() {
                return Err(Error::invalid(format!(
                    "无法读取副本元数据: {}",
                    dest.display()
                )));
            }
        }
        let link_back = (!restore).then(|| old_path.clone());
        let backup_path = self.store.dir().join("skill-backups").join(format!(
            "{}-{}",
            skill.id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let source_ids = saved
            .as_ref()
            .map(|p| p.source_ids.clone())
            .unwrap_or_else(|| {
                let roots = shared_roots(config);
                original_source_ids(config, &skill, &self.agent_states(config, &skill), &roots)
            });
        let record = if restore {
            saved.clone().unwrap()
        } else {
            SkillProvenance {
                source_ids,
                original_path: old_path.clone(),
                original_root: skill.root.clone(),
                original_origin: skill.origin.clone(),
                backup_path: backup_path.clone(),
                original_hash: source_hash.clone(),
                collected_at: linker::now_secs(),
                entry_paths: links
                    .iter()
                    .cloned()
                    .chain(
                        copies
                            .iter()
                            .filter_map(|(p, _)| p.parent().map(|p| p.to_path_buf())),
                    )
                    .chain(std::iter::once(old_path.clone()))
                    .chain(link_back.iter().filter(|p| **p != old_path).cloned())
                    .collect(),
            }
        };
        let mut next = config.clone();
        migrate_skill_id(&mut next, &skill.id, &new_id, &target);
        if replace_imported {
            next.skill_installations.remove(&new_id);
        }
        for project in &mut next.projects {
            for entry in &mut project.managed_entries {
                if entry.skill_id == new_id {
                    entry.source_path = target.clone();
                }
            }
        }
        next.skill_provenance.insert(new_id.clone(), record.clone());
        if restore {
            if let Some(regs) = next.registrations.get_mut(&new_id) {
                regs.retain(|_, r| r.target_path != target);
            }
        }
        next.registrations.retain(|_, regs| !regs.is_empty());
        let mut tx =
            super::transaction::Transaction::begin(self.store.dir().join("migration.json"))?;
        let operation = (|| -> Result<()> {
            let before = scanner::dir_content_hash(&old_path)?;
            if before != source_hash {
                return Err(Error::invalid("来源在备份前发生变化，请重试"));
            }
            // Keep a permanent verified snapshot separate from the temporary undo journal.
            tx.reserve(&backup_path)?;
            linker::copy_tree(&old_path, &backup_path.join("content"))?;
            if scanner::dir_content_hash(&backup_path.join("content"))? != before {
                return Err(Error::invalid("备份校验失败"));
            }
            crate::fs::atomic::write_json_file(&backup_path.join("provenance.json"), &record)?;
            crate::fs::atomic::write_json_file(&backup_path.join("config.json"), config)?;
            crate::fs::atomic::write_json_file(
                &backup_path.join("snapshot.json"),
                &serde_json::json!({ "operation": if restore { "release" } else { "adopt" }, "source": old_path, "destination": target, "contentHash": before, "createdAt": linker::now_secs() }),
            )?;
            if restore
                && target.symlink_metadata().is_ok()
                && !matches!(
                    linker::link_status(&old_path, &target),
                    LinkStatus::Linked | LinkStatus::Copied | LinkStatus::CopyStale
                )
            {
                return Err(Error::invalid("原位置发生变化，停止还原"));
            }
            if replace_imported && scanner::dir_content_hash(&target)? != before {
                return Err(Error::invalid("Hub 内容在迁移期间发生变化，停止覆盖"));
            }
            if !restore && !replace_imported && target.symlink_metadata().is_ok() {
                return Err(Error::invalid("Hub 目标在迁移期间出现同名内容，停止托管"));
            }
            tx.reserve(&target)?;
            linker::copy_tree(&old_path, &target)?;
            if scanner::dir_content_hash(&target)? != before
                || scanner::dir_content_hash(&old_path)? != before
            {
                return Err(Error::invalid("迁移校验失败或源在迁移期间发生变化"));
            }
            scanner::validate_sync_source(&target)?;
            tx.reserve(&old_path)?;
            if let Some(link_back) = &link_back {
                // Collection is always a relocation plus a symlink. Deployment's
                // Copy/Auto preference must never create a second original here.
                linker::create_symlink(&target, link_back)?;
                if let SkillOrigin::InPlace { ref owner_agent } = skill.origin {
                    next.set_registration(
                        &new_id,
                        owner_agent,
                        Registration {
                            mode: LinkMode::Symlink,
                            target_path: link_back.clone(),
                            registered_at: linker::now_secs(),
                            source_hash_at_copy: None,
                        },
                    );
                }
            }
            for dest in &links {
                tx.reserve(dest)?;
                linker::create_symlink(&target, dest)?;
            }
            for (path, sidecar) in &copies {
                tx.reserve(path)?;
                crate::fs::atomic::write_json_file(path, sidecar)?;
            }
            // Configuration is part of the same undo journal as filesystem changes.
            self.store.save_in_transaction(&mut tx, &next)?;
            Ok(())
        })();
        if let Err(err) = operation {
            if let Err(recovery) = tx.rollback() {
                return Err(Error::Other(format!(
                    "{err}; 自动回滚未完成，请重启恢复: {recovery}"
                )));
            }
            return Err(err);
        }
        tx.commit()?;
        *config = next;

        let mut adopted = skill;
        adopted.id = new_id;
        adopted.source_path = target;
        adopted.root = if restore { record.original_root } else { hub };
        adopted.origin = if restore {
            record.original_origin
        } else {
            SkillOrigin::Hub
        };
        Ok(adopted)
    }

    fn resolve_skills(&self, config: &AppConfig, ids: &[String]) -> Result<Vec<Skill>> {
        let snapshot = self.scan_snapshot(config)?;
        let wanted: HashSet<_> = ids.iter().map(String::as_str).collect();
        let all = self.scan_sources(config, &snapshot.roots, Some(&wanted))?;
        ids.iter()
            .map(|id| {
                all.get(id)
                    .cloned()
                    .ok_or_else(|| Error::NotFound(format!("skill {id}")))
            })
            .collect()
    }

    /// 清理引用了已消失 skill 的分组成员与注册记录
    pub fn prune(&self, config: &mut AppConfig) -> Result<usize> {
        // An explicitly configured root disappearing is not evidence of deletion.
        for root in config
            .settings
            .hub_dir
            .iter()
            .chain(config.settings.agent_dir_overrides.values())
        {
            std::fs::read_dir(root).map_err(|e| Error::io(root, e))?;
        }
        let existing: HashSet<String> = self
            .scan_skills(config)?
            .into_iter()
            .filter(|v| v.skill.source_path.join(scanner::SKILL_FILE).is_file())
            .map(|v| v.skill.id)
            .collect();
        Ok(config.prune_missing_skills(&existing))
    }
}

fn migrate_skill_id(config: &mut AppConfig, old: &str, new: &str, source: &std::path::Path) {
    if old == new {
        return;
    }
    if let Some(provenance) = config.skill_provenance.remove(old) {
        config
            .skill_provenance
            .entry(new.into())
            .or_insert(provenance);
    }
    for group in &mut config.groups {
        for id in &mut group.skill_ids {
            if id == old {
                *id = new.to_string();
            }
        }
        let mut seen = HashSet::new();
        group.skill_ids.retain(|id| seen.insert(id.clone()));
    }
    for records in config.active_groups.values_mut() {
        for group in records {
            for id in &mut group.skill_ids {
                if id == old {
                    *id = new.to_string();
                }
            }
            for entry in &mut group.entries {
                if entry.skill_id == old {
                    entry.skill_id = new.into();
                    entry.source_path = source.into();
                }
            }
        }
    }
    for entries in config.policy_suspensions.values_mut() {
        for entry in entries {
            if entry.skill_id == old {
                entry.skill_id = new.into();
                entry.source_path = source.into();
            }
        }
    }
    for project in &mut config.projects {
        for entry in &mut project.managed_entries {
            if entry.skill_id == old {
                entry.skill_id = new.into();
                entry.source_path = source.into();
            }
        }
        for id in &mut project.skill_ids {
            if id == old {
                *id = new.to_string();
            }
        }
        let mut seen = HashSet::new();
        project.skill_ids.retain(|id| seen.insert(id.clone()));
    }
    if let Some(regs) = config.registrations.remove(old) {
        let current = config.registrations.entry(new.into()).or_default();
        for (agent, registration) in regs {
            current.entry(agent).or_insert(registration);
        }
    }
    if let Some(meta) = config.skill_meta.remove(old) {
        let current = config.skill_meta.entry(new.into()).or_default();
        current.favorite |= meta.favorite;
        current.pinned |= meta.pinned;
        if current.note.is_none() {
            current.note = meta.note;
        }
    }
}

pub(super) fn build_skill(
    id: String,
    name: &str,
    source_path: PathBuf,
    root: PathBuf,
    origin: SkillOrigin,
    frontmatter: Option<Frontmatter>,
) -> Skill {
    build_skill_with_content(id, name, source_path, root, origin, frontmatter, true)
}

fn build_skill_with_content(
    id: String,
    name: &str,
    source_path: PathBuf,
    root: PathBuf,
    origin: SkillOrigin,
    frontmatter: Option<Frontmatter>,
    content: bool,
) -> Skill {
    let fm = frontmatter.unwrap_or_default();
    let content_hash = if content {
        scanner::dir_content_hash(&source_path).unwrap_or_default()
    } else {
        String::new()
    };
    let token_estimate = if content {
        tokens::estimate_skill(&source_path)
    } else {
        Default::default()
    };
    Skill {
        id,
        name: name.to_string(),
        display_name: fm.name.clone(),
        description: fm.description.clone(),
        source_path,
        origin,
        content_hash,
        frontmatter_extra: fm.extra_keys.clone(),
        root,
        tokens: token_estimate,
    }
}

fn result(
    skill: &Skill,
    agent_id: &str,
    status: LinkStatus,
    message: Option<String>,
) -> LinkResult {
    LinkResult {
        skill_id: skill.id.clone(),
        skill_name: skill.name.clone(),
        agent_id: agent_id.to_string(),
        status,
        message,
    }
}

/// 一次扫描/操作内解析全部共享根（精确路径去重），供来源标注同一个切片复用。
///
/// 只在本次调用内复用，不缓存：写时保护（迁移/回链/还原）仍走
/// 根目录的独立校验负责写入安全，两者互不替代。
fn shared_roots(config: &AppConfig) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for agent in AGENTS {
        for root in agent.resolved_shared_roots(&config.settings.agent_dir_overrides) {
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    roots
}

fn original_source_ids(
    config: &AppConfig,
    skill: &Skill,
    states: &HashMap<String, AgentSkillState>,
    roots: &[PathBuf],
) -> Vec<String> {
    // Historical Hub items without a collection record have unknown origins; storage is not provenance.
    if matches!(skill.origin, SkillOrigin::Hub) {
        return vec!["unknown".into()];
    }
    let shared = |path: &std::path::Path| {
        // 根本身或根的直接子项都算共享来源；别名比较保留符号链接指向同一根的情况。
        let parent = path.parent();
        roots.iter().any(|root| {
            crate::fs::paths::paths_alias(root, path)
                || parent.is_some_and(|parent| crate::fs::paths::paths_alias(root, parent))
        }) || path
            .components()
            .any(|c| c.as_os_str() == ".agents" || c.as_os_str() == ".agent")
    };
    let mut ids = HashSet::new();
    if matches!(skill.origin, SkillOrigin::Local) {
        ids.insert("local".into());
    } else if shared(&skill.source_path) || shared(&skill.root) {
        ids.insert("agent".to_string());
    } else if let SkillOrigin::InPlace { owner_agent } = &skill.origin {
        ids.insert(owner_agent.clone());
    }
    for (id, state) in states {
        if !matches!(state.status, LinkStatus::Source | LinkStatus::Linked) {
            continue;
        }
        for path in &state.entry_paths {
            if config
                .registration(&skill.id, id)
                .is_some_and(|r| r.target_path == *path)
            {
                continue;
            }
            ids.insert(if shared(path) {
                "agent".into()
            } else {
                id.clone()
            });
        }
    }
    if ids.is_empty() {
        ids.insert("external".into());
    }
    let mut ids: Vec<_> = ids.into_iter().collect();
    ids.sort();
    ids
}

#[cfg(all(test, unix))]
#[path = "../../tests/support.rs"]
mod scan_test_support;

#[cfg(all(test, unix))]
mod scan_cost_tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn root_scans_are_bounded_and_single_lookup_only_hashes_its_source() {
        let env = scan_test_support::Env::new();
        std::fs::create_dir_all(env.claude_skills()).unwrap();
        for index in 0..50 {
            let name = format!("skill-{index}");
            let source = env.write_simple_skill(&env.hub(), &name);
            std::os::unix::fs::symlink(&source, env.claude_skills().join(name)).unwrap();
        }
        let studio = env.studio();
        let config = studio.load_config().unwrap();
        let mut roots: HashSet<_> = AGENTS
            .iter()
            .flat_map(|agent| {
                agent
                    .resolved_global_roots(&config.settings.agent_dir_overrides)
                    .into_iter()
                    .map(move |root| (agent.id, root))
            })
            .collect();
        roots.insert((AGENTS[0].id, env.hub()));
        scanner::take_scan_counts();
        let skills = studio.scan_skills(&config).unwrap();
        assert_eq!(skills.len(), 50);
        assert_eq!(
            scanner::take_scan_counts().0,
            roots.len(),
            "each (Agent, root) is scanned once, not once per skill"
        );
        let selected = studio.find_skill(&config, &skills[0].skill.id).unwrap();
        assert_eq!(selected.content_hash, skills[0].skill.content_hash);
        assert_eq!(selected.tokens, skills[0].skill.tokens);
        assert_eq!(
            scanner::take_scan_counts(),
            (roots.len(), 1),
            "lookup must not build all Agent states or hash unrelated skills"
        );
        assert!(studio
            .knows_skill_path(&config, &env.claude_skills().join(&selected.name))
            .unwrap());
        assert_eq!(
            scanner::take_scan_counts(),
            (roots.len(), 0),
            "preview authorization must not hash payloads"
        );
        assert!(!studio
            .knows_skill_path(&config, &env.claude_skills().join("unrelated"))
            .unwrap());
    }

    #[test]
    #[ignore = "manual debug-build scaling benchmark"]
    #[serial]
    fn scan_scaling_benchmark() {
        for count in [50, 100, 200] {
            let env = scan_test_support::Env::new();
            std::fs::create_dir_all(env.claude_skills()).unwrap();
            for index in 0..count {
                let name = format!("skill-{index}");
                let source = env.write_simple_skill(&env.hub(), &name);
                std::os::unix::fs::symlink(&source, env.claude_skills().join(name)).unwrap();
            }
            let studio = env.studio();
            let config = studio.load_config().unwrap();
            let started = std::time::Instant::now();
            assert_eq!(studio.scan_skills(&config).unwrap().len(), count);
            println!("{count} skills: {:?}", started.elapsed());
        }
    }
}
