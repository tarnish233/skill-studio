//! 编排层：把 scanner / detector / linker / store / native_toggle 串起来，
//! 对上暴露 Tauri 命令直接需要的粗粒度操作。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::models::agent::{find_agent, in_shared_root, AgentInfo, AGENTS};
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

    /// 扫出全部 skill 真身。
    ///
    /// 真身来自两处：各 agent 全局根里的**真目录**（原地模式），以及 Hub 目录。
    /// 目标位置上的链接与复制不是真身，它们只是注册结果。
    pub fn scan_skills(&self, config: &AppConfig) -> Result<Vec<SkillView>> {
        let overrides = &config.settings.agent_dir_overrides;
        // skill_id -> Skill，同一路径只收一次
        let mut sources: HashMap<String, Skill> = HashMap::new();

        for agent in AGENTS {
            for root in agent.resolved_global_roots(overrides) {
                for entry in scanner::scan_root(&root, agent)? {
                    if !matches!(entry.kind, EntryKind::RealSkill) {
                        continue;
                    }
                    if sources
                        .values()
                        .any(|s| crate::fs::paths::paths_alias(&s.source_path, &entry.path))
                    {
                        continue;
                    }
                    let id = skill_id_for(&entry.path);
                    sources.entry(id.clone()).or_insert_with(|| {
                        build_skill(
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
        for entry in scanner::scan_root(&hub, claude)? {
            if !matches!(entry.kind, EntryKind::RealSkill) {
                continue;
            }
            if sources
                .values()
                .any(|s| crate::fs::paths::paths_alias(&s.source_path, &entry.path))
            {
                continue;
            }
            let id = skill_id_for(&entry.path);
            sources.entry(id.clone()).or_insert_with(|| {
                build_skill(
                    id.clone(),
                    &entry.name,
                    entry.path.clone(),
                    hub.clone(),
                    SkillOrigin::Hub,
                    entry.frontmatter.clone(),
                )
            });
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
                        build_skill(
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
                for entry in scanner::scan_root(&root, agent)? {
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
                        build_skill(
                            id,
                            &name,
                            source,
                            root.clone(),
                            SkillOrigin::External,
                            entry.frontmatter,
                        ),
                    );
                }
            }
        }

        // Restored originals remain independent files, but keep their adopted identity
        // while both contents still match the recorded backup.
        let restored: HashSet<String> = sources
            .values()
            .filter(|candidate| {
                sources.values().any(|hub| {
                    hub.id != candidate.id && restored_entry(config, hub, &candidate.source_path)
                })
            })
            .map(|s| s.id.clone())
            .collect();
        sources.retain(|id, _| !restored.contains(id));

        let mut views: Vec<SkillView> = sources
            .into_values()
            .map(|skill| {
                let agents = self.agent_states(config, &skill);
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
                        .unwrap_or_else(|| original_source_ids(config, &skill, &agents))
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
                                && restored_entry(config, skill, path)
                        })
                        .cloned(),
                );
            }
            if let Ok(entries) = scanner::scan_root(&root, agent) {
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
        let overrides = &config.settings.agent_dir_overrides;
        let mut out = HashMap::new();

        for agent in AGENTS {
            let resolved = super::variants::for_agent(config, skill, agent.id);
            let unavailable = resolved.as_ref().err().map(ToString::to_string);
            let identity = super::variants::deployment_source(config, skill, agent.id);
            let skill = identity.as_ref().unwrap_or(skill);
            let roots = agent.resolved_global_roots(overrides);
            let primary = roots.first().cloned().unwrap_or_default();
            let mut best: Option<(LinkStatus, PathBuf)> = None;

            let targets = self.agent_targets(config, skill, agent);
            let entry_paths: Vec<PathBuf> = targets
                .iter()
                .filter(|p| p.symlink_metadata().is_ok())
                .cloned()
                .collect();
            for dest in targets {
                // 真身就在这个 root 里
                let status = if dest.join(scanner::SKILL_FILE).is_file()
                    && !scanner::is_symlink_or_junction(&dest)
                    && (crate::fs::paths::paths_alias(&dest, &skill.source_path)
                        || restored_entry(config, skill, &dest))
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
                && native_toggle::is_skill_disabled_at(agent, overrides, toggle_name, &document);
            let mode = config.registration(&skill.id, agent.id).map(|r| r.mode);

            out.insert(
                agent.id.to_string(),
                AgentSkillState {
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

    /// 按 ID 找一个 skill 真身
    pub fn find_skill(&self, config: &AppConfig, skill_id: &str) -> Result<Skill> {
        self.scan_skills(config)?
            .into_iter()
            .find(|v| v.skill.id == skill_id)
            .map(|v| v.skill)
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
                    if let Err(error) = self
                        .store()
                        .reserve_config(&mut tx, &next)
                        .and_then(|_| self.save_config(&next))
                    {
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
                        && (crate::fs::paths::paths_alias(&dest, &skill.source_path)
                            || restored_entry(config, skill, &dest))
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
            self.store().reserve_config(&mut tx, &next)?;
            self.save_config(&next)?;
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
                && !restored_entry(config, &skill, &target)
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
        self.relocate_hub_skill(config, skill_id, false)
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
        self.relocate_hub_skill(config, skill_id, true)
    }

    fn relocate_hub_skill(
        &self,
        config: &mut AppConfig,
        skill_id: &str,
        restore: bool,
    ) -> Result<Skill> {
        if config
            .active_groups
            .values()
            .flatten()
            .any(|g| g.skill_ids.iter().any(|id| id == skill_id))
            || config
                .policy_suspensions
                .values()
                .flatten()
                .any(|e| e.skill_id == skill_id)
        {
            return Err(Error::invalid(
                "请先停用包含或暂时停用此 skill 的分组，再迁移",
            ));
        }
        for id in &config.settings.disabled_agents {
            let agent = crate::models::agent::require_agent(id)?;
            let skill = self.find_skill(config, skill_id)?;
            let roots = agent.resolved_global_roots(&config.settings.agent_dir_overrides);
            let record = config.skill_provenance.get(skill_id);
            let touches_disabled = std::iter::once(&skill.source_path)
                .chain(record.into_iter().flat_map(|p| {
                    p.entry_paths
                        .iter()
                        .chain(std::iter::once(&p.original_path))
                }))
                .any(|p| {
                    p.parent().is_some_and(|parent| {
                        roots
                            .iter()
                            .any(|r| crate::fs::paths::paths_alias(parent, r))
                    })
                });
            if touches_disabled {
                return Err(Error::invalid(
                    "此 skill 涉及已退出管理的 Agent，请先开启该应用再托管或移出 Hub",
                ));
            }
        }
        if config
            .policy_suspensions
            .values()
            .flatten()
            .any(|e| e.skill_id == skill_id)
        {
            return Err(Error::invalid(
                "请先开启保留手动 skill，再迁移被策略停用的 skill",
            ));
        }
        let skill = self.find_skill(config, skill_id)?;
        if !restore && matches!(skill.origin, SkillOrigin::Hub) {
            return Ok(skill);
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
                "外部来源仅通过链接引用，不能自动迁移或接管源目录",
            ));
        }
        let hub = self.store.hub_dir(config);
        let overrides = config.settings.agent_dir_overrides.clone();
        let all_roots: Vec<PathBuf> = AGENTS
            .iter()
            .flat_map(|a| a.resolved_global_roots(&overrides))
            .collect();
        linker::ensure_distinct_roots(&hub, &all_roots)?;

        // 原件在共享根（~/.agents/skills）时，回链和移出 Hub 都写到所属 agent 自己的根：
        // 共享根里的东西会被每个读它的 agent 加载，分组和开关管不住。
        let owner = match if restore {
            saved.as_ref().map(|p| &p.original_origin)
        } else {
            Some(&skill.origin)
        } {
            Some(SkillOrigin::InPlace { owner_agent }) => find_agent(owner_agent),
            _ => None,
        };
        let private_home = |path: &std::path::Path| -> Option<PathBuf> {
            if !in_shared_root(path, &overrides) {
                return None;
            }
            let root = owner?.primary_global_root(&overrides);
            // 只比较目录本身：条目可能正是指向原件的软链，跟随它会误判成同一位置
            let distinct = !path
                .parent()
                .is_some_and(|parent| crate::fs::paths::paths_alias(parent, &root));
            let name = path.file_name()?;
            distinct.then(|| root.join(name))
        };

        std::fs::create_dir_all(&hub).map_err(|e| Error::io(&hub, e))?;
        let released_to_private = restore
            .then(|| private_home(&saved.as_ref().unwrap().original_path))
            .flatten();
        let target = if restore {
            released_to_private
                .clone()
                .unwrap_or_else(|| saved.as_ref().unwrap().original_path.clone())
        } else {
            hub.join(&skill.name)
        };
        if let Some(parent) = released_to_private.as_ref().and_then(|t| t.parent()) {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        scanner::validate_sync_source(&skill.source_path)?;
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
        } else if target.exists() || scanner::is_symlink_or_junction(&target) {
            return Err(Error::invalid(format!(
                "Hub 里已存在同名 skill: {}",
                target.display()
            )));
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
        // 共享根里指向旧位置的链接不再改指，直接移除
        let mut shared_links = Vec::new();
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
                    if in_shared_root(&dest, &overrides) {
                        shared_links.push(dest);
                    } else {
                        links.push(dest);
                    }
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
        // 纳入托管后留给所属 agent 的入口：原位置，或（原件在共享根时）它自己的根。
        // 自己的根里已有指向原件的链接就沿用；被别的条目占着就不覆盖。
        let (link_back, alias_back) = match (!restore).then(|| private_home(&old_path)) {
            None => (None, None),
            Some(None) => (Some(old_path.clone()), None),
            Some(Some(private))
                if links
                    .iter()
                    .any(|l| crate::fs::paths::is_same_path(l, &private)) =>
            {
                (None, Some(private))
            }
            Some(Some(private)) if std::fs::symlink_metadata(&private).is_ok() => (None, None),
            Some(Some(private)) => (Some(private), None),
        };
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
                original_source_ids(config, &skill, &self.agent_states(config, &skill))
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
                original_hash: scanner::dir_content_hash(&old_path)?,
                collected_at: linker::now_secs(),
                entry_paths: links
                    .iter()
                    .chain(&shared_links)
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
        migrate_skill_id(&mut next, &skill.id, &new_id);
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
        // 旧版本留在共享根里的回链会被移除，对应的登记一并去掉
        for regs in next.registrations.values_mut() {
            regs.retain(|_, r| {
                !shared_links
                    .iter()
                    .any(|l| crate::fs::paths::is_same_path(l, &r.target_path))
            });
        }
        next.registrations.retain(|_, regs| !regs.is_empty());
        let mut tx =
            super::transaction::Transaction::begin(self.store.dir().join("migration.json"))?;
        let operation = (|| -> Result<()> {
            let before = scanner::dir_content_hash(&old_path)?;
            if !restore && before != record.original_hash {
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
                if link_back != &old_path {
                    if let Some(parent) = link_back.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
                    }
                    tx.reserve(link_back)?;
                }
                // Stay inside the outer journal: a nested copy transaction could undo
                // the restored source during startup recovery.
                let mode = config.settings.default_link_mode;
                let linked = if mode == LinkMode::Copy {
                    false
                } else {
                    match linker::create_symlink(&target, link_back) {
                        Ok(()) => true,
                        Err(err) if mode == LinkMode::Symlink => return Err(err),
                        Err(_) => false,
                    }
                };
                let done = if linked {
                    linker::Registered {
                        mode: LinkMode::Symlink,
                        status: LinkStatus::Linked,
                        source_hash: None,
                    }
                } else {
                    linker::copy_tree(&target, link_back)?;
                    if scanner::dir_content_hash(link_back)? != before {
                        return Err(Error::invalid("原位置副本校验失败"));
                    }
                    crate::fs::atomic::write_json_file(
                        &link_back.join(scanner::COPY_SIDECAR),
                        &scanner::CopySidecar {
                            skill_id: new_id.clone(),
                            source_path: target.clone(),
                            source_hash: before.clone(),
                            copied_at: linker::now_secs(),
                        },
                    )?;
                    linker::Registered {
                        mode: LinkMode::Copy,
                        status: LinkStatus::Copied,
                        source_hash: Some(before),
                    }
                };
                if let SkillOrigin::InPlace { ref owner_agent } = skill.origin {
                    next.set_registration(
                        &new_id,
                        owner_agent,
                        Registration {
                            mode: done.mode,
                            target_path: link_back.clone(),
                            registered_at: linker::now_secs(),
                            source_hash_at_copy: done.source_hash,
                        },
                    );
                }
            }
            if let (Some(alias), SkillOrigin::InPlace { owner_agent }) =
                (&alias_back, &skill.origin)
            {
                // 所属 agent 自己的根里本来就有指向原件的链接：下面改指到 Hub，并登记为它的入口
                next.set_registration(
                    &new_id,
                    owner_agent,
                    Registration {
                        mode: LinkMode::Symlink,
                        target_path: alias.clone(),
                        registered_at: linker::now_secs(),
                        source_hash_at_copy: None,
                    },
                );
            }
            for dest in &links {
                tx.reserve(dest)?;
                linker::create_symlink(&target, dest)?;
            }
            for dest in &shared_links {
                tx.reserve(dest)?;
            }
            for (path, sidecar) in &copies {
                tx.reserve(path)?;
                crate::fs::atomic::write_json_file(path, sidecar)?;
            }
            // Configuration is part of the same undo journal as filesystem changes.
            self.store.reserve_config(&mut tx, &next)?;
            self.store.save(&next)?;
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
        adopted.root = match (restore, &released_to_private) {
            (true, Some(private)) => private
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or(record.original_root),
            (true, None) => record.original_root,
            (false, _) => hub,
        };
        adopted.origin = if restore {
            record.original_origin
        } else {
            SkillOrigin::Hub
        };
        Ok(adopted)
    }

    fn resolve_skills(&self, config: &AppConfig, ids: &[String]) -> Result<Vec<Skill>> {
        let all = self.scan_skills(config)?;
        let by_id: HashMap<&str, &Skill> = all
            .iter()
            .map(|v| (v.skill.id.as_str(), &v.skill))
            .collect();
        ids.iter()
            .map(|id| {
                by_id
                    .get(id.as_str())
                    .map(|s| (*s).clone())
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

fn migrate_skill_id(config: &mut AppConfig, old: &str, new: &str) {
    if let Some(provenance) = config.skill_provenance.remove(old) {
        config.skill_provenance.insert(new.into(), provenance);
    }
    for group in &mut config.groups {
        for id in &mut group.skill_ids {
            if id == old {
                *id = new.to_string();
            }
        }
    }
    for project in &mut config.projects {
        for entry in &mut project.managed_entries {
            if entry.skill_id == old {
                entry.skill_id = new.into();
            }
        }
        for id in &mut project.skill_ids {
            if id == old {
                *id = new.to_string();
            }
        }
    }
    if let Some(regs) = config.registrations.remove(old) {
        config.registrations.insert(new.to_string(), regs);
    }
    if let Some(meta) = config.skill_meta.remove(old) {
        config.skill_meta.insert(new.to_string(), meta);
    }
}

fn build_skill(
    id: String,
    name: &str,
    source_path: PathBuf,
    root: PathBuf,
    origin: SkillOrigin,
    frontmatter: Option<Frontmatter>,
) -> Skill {
    let fm = frontmatter.unwrap_or_default();
    let content_hash = scanner::dir_content_hash(&source_path).unwrap_or_default();
    let token_estimate = tokens::estimate_skill(&source_path);
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

fn original_source_ids(
    config: &AppConfig,
    skill: &Skill,
    states: &HashMap<String, AgentSkillState>,
) -> Vec<String> {
    // Historical Hub items without a collection record have unknown origins; storage is not provenance.
    if matches!(skill.origin, SkillOrigin::Hub) {
        return vec!["unknown".into()];
    }
    let shared = |path: &std::path::Path| {
        path.components()
            .any(|c| c.as_os_str() == ".agents" || c.as_os_str() == ".agent")
    };
    let mut ids = HashSet::new();
    if shared(&skill.source_path) || shared(&skill.root) {
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

/// Only known, unchanged restored originals may share an adopted Hub identity.
pub(crate) fn restored_entry(config: &AppConfig, skill: &Skill, target: &std::path::Path) -> bool {
    matches!(skill.origin, SkillOrigin::Hub)
        && !scanner::is_symlink_or_junction(target)
        && config.skill_provenance.get(&skill.id).is_some_and(|p| {
            (p.original_path == target || p.entry_paths.iter().any(|entry| entry == target))
                && scanner::dir_content_hash(target).is_ok_and(|h| h == p.original_hash)
                && scanner::dir_content_hash(&skill.source_path).is_ok_and(|h| h == p.original_hash)
        })
}
