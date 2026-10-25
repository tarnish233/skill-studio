//! Search the public skills.sh catalog and stage GitHub skills without executing repository code.
use crate::{
    models::{
        config::{AppConfig, SkillInstallation},
        skill::{skill_id_for, Skill},
    },
    services::{linker, scanner, studio::Studio, transaction::Transaction},
    Error, Result,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
    time::Duration,
};

const MAX_DOWNLOAD: u64 = 64 * 1024 * 1024;
const MAX_EXPANDED: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSkill {
    pub skill_id: String,
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub installs: u64,
}
#[derive(Debug, Deserialize)]
struct SearchResponse {
    skills: Vec<CatalogSkill>,
}

pub fn validate_coordinates(source: &str, skill_id: &str) -> Result<()> {
    let parts: Vec<_> = source.split('/').collect();
    let valid = parts.len() == 2
        && !parts[0].is_empty()
        && parts[0].len() <= 100
        && parts[0]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        && !parts[1].is_empty()
        && parts[1].len() <= 100
        && parts[1] != "."
        && parts[1] != ".."
        && parts[1]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c));
    if !valid
        || skill_id.is_empty()
        || skill_id.len() > 100
        || !skill_id.as_bytes()[0].is_ascii_alphanumeric()
        || !skill_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
    {
        return Err(Error::invalid("仓库或 skill 标识无效"));
    }
    Ok(())
}
fn download(url: reqwest::Url, limit: u64) -> Result<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Skill-Studio/0.1")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| Error::Other(e.to_string()))?;
    let response = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| Error::Other(format!("下载失败：{e}")))?;
    if response.status().is_redirection() {
        return Err(Error::invalid("下载地址发生重定向，请稍后重试"));
    }
    if response.content_length().is_some_and(|n| n > limit) {
        return Err(Error::invalid("下载内容过大"));
    }
    let mut bytes = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::Other(format!("读取下载内容失败：{e}")))?;
    if bytes.len() as u64 > limit {
        return Err(Error::invalid("下载内容过大"));
    }
    Ok(bytes)
}
pub fn search(query: &str) -> Result<Vec<CatalogSkill>> {
    let query = query.trim();
    if query.chars().count() < 2 || query.chars().count() > 200 {
        return Err(Error::invalid("请输入 2 至 200 个字符搜索"));
    }
    let mut url = reqwest::Url::parse("https://skills.sh/api/search").unwrap();
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("limit", "100");
    let bytes = download(url, 2 * 1024 * 1024)?;
    let data: SearchResponse = serde_json::from_slice(&bytes)
        .map_err(|e| Error::Other(format!("搜索服务响应无效：{e}")))?;
    let mut seen = std::collections::HashSet::new();
    Ok(data
        .skills
        .into_iter()
        .filter(|s| {
            validate_coordinates(&s.source, &s.skill_id).is_ok()
                && seen.insert((s.source.clone(), s.skill_id.clone()))
        })
        .take(100)
        .collect())
}

pub struct PreparedSkill {
    _temp: tempfile::TempDir,
    pub directory: PathBuf,
    pub source: String,
    pub skill_id: String,
    pub repository_path: String,
    pub variants: Vec<crate::models::config::SkillVariant>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCandidate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub repository_path: String,
    pub variant_key: Option<String>,
    pub description: Option<String>,
    pub content_hash: String,
}

pub enum CatalogPreparation {
    Ready(PreparedSkill),
    SelectionRequired(Vec<CatalogCandidate>),
}

pub fn prepare(source: &str, skill_id: &str) -> Result<PreparedSkill> {
    require_ready(prepare_catalog(source, skill_id, None)?)
}

fn require_ready(result: CatalogPreparation) -> Result<PreparedSkill> {
    match result {
        CatalogPreparation::Ready(skill) => Ok(skill),
        CatalogPreparation::SelectionRequired(_) => {
            Err(Error::invalid("仓库内有多个同名 skill，请选择安装目录"))
        }
    }
}

pub fn prepare_catalog(
    source: &str,
    skill_id: &str,
    repository_path: Option<&str>,
) -> Result<CatalogPreparation> {
    prepare_catalog_variants(
        source,
        skill_id,
        &repository_path
            .map(str::to_owned)
            .into_iter()
            .collect::<Vec<_>>(),
    )
}

pub fn prepare_catalog_variants(
    source: &str,
    skill_id: &str,
    selected: &[String],
) -> Result<CatalogPreparation> {
    validate_coordinates(source, skill_id)?;
    let url = reqwest::Url::parse(&format!("https://codeload.github.com/{source}/zip/HEAD"))
        .map_err(|e| Error::Other(e.to_string()))?;
    prepare_catalog_archive_variants(source, skill_id, &download(url, MAX_DOWNLOAD)?, selected)
}

/// Public for deterministic integration testing with repository archives.
pub fn prepare_archive(source: &str, skill_id: &str, bytes: &[u8]) -> Result<PreparedSkill> {
    require_ready(prepare_catalog_archive(source, skill_id, bytes, None)?)
}

pub fn prepare_catalog_archive(
    source: &str,
    skill_id: &str,
    bytes: &[u8],
    repository_path: Option<&str>,
) -> Result<CatalogPreparation> {
    prepare_catalog_archive_variants(
        source,
        skill_id,
        bytes,
        &repository_path
            .map(str::to_owned)
            .into_iter()
            .collect::<Vec<_>>(),
    )
}

pub fn prepare_catalog_archive_variants(
    source: &str,
    skill_id: &str,
    bytes: &[u8],
    selected: &[String],
) -> Result<CatalogPreparation> {
    validate_coordinates(source, skill_id)?;
    if bytes.len() as u64 > MAX_DOWNLOAD {
        return Err(Error::invalid("下载内容过大"));
    }
    let temp = tempfile::tempdir().map_err(|e| Error::Other(e.to_string()))?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| Error::Other(format!("无效仓库归档：{e}")))?;
    if archive.len() > MAX_ENTRIES {
        return Err(Error::invalid("仓库文件数量超过限制"));
    }
    let mut expanded = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| Error::Other(e.to_string()))?;
        let path = entry
            .enclosed_name()
            .ok_or_else(|| Error::invalid("归档包含越界路径"))?;
        if entry.name().contains('\\')
            || entry.name().contains(':')
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(Error::invalid("归档包含无效路径"));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(Error::invalid("仓库包含符号链接，暂不支持自动安装"));
        }
        expanded = expanded.saturating_add(if entry.is_dir() { 4096 } else { entry.size() });
        if expanded > MAX_EXPANDED {
            return Err(Error::invalid("解压内容超过限制"));
        }
        let target = temp.path().join(path);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| Error::io(&target, e))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let declared = entry.size();
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| Error::io(&target, e))?;
        let copied = std::io::copy(&mut (&mut entry).take(declared + 1), &mut output)
            .map_err(|e| Error::io(&target, e))?;
        if copied != declared {
            return Err(Error::invalid("归档文件大小校验失败"));
        }
        output.flush().map_err(|e| Error::io(&target, e))?;
        #[cfg(unix)]
        if entry.unix_mode().is_some_and(|m| m & 0o111 != 0) {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
                .map_err(|e| Error::io(&target, e))?;
        }
    }
    // GitHub archives have one repository envelope. Only inspect registered
    // root conventions and their immediate Skill children, never walk examples,
    // plugin trees or arbitrary Agent directories looking for matching names.
    let envelopes = fs::read_dir(temp.path())
        .map_err(|e| Error::io(temp.path(), e))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| Error::io(temp.path(), e))?;
    if envelopes.len() != 1 || !envelopes[0].path().is_dir() {
        return Err(Error::invalid("仓库归档必须包含一个根目录"));
    }
    let repository = envelopes[0].path();
    let mut found = Vec::new();
    let mut consider = |directory: PathBuf| {
        if directory.join("SKILL.md").is_file() {
            let fm = scanner::parse_frontmatter(&directory.join("SKILL.md"));
            if directory.file_name().is_some_and(|name| name == skill_id)
                || fm.name.as_deref() == Some(skill_id)
            {
                found.push(directory);
            }
        }
    };
    consider(repository.clone());
    for relative in super::variants::repository_roots().keys() {
        let root = repository.join(relative);
        if !root.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&root).map_err(|e| Error::io(&root, e))? {
            let entry = entry.map_err(|e| Error::io(&root, e))?;
            if entry
                .file_type()
                .map_err(|e| Error::io(entry.path(), e))?
                .is_dir()
                && !entry.file_name().to_string_lossy().starts_with('.')
            {
                consider(entry.path());
            }
        }
    }
    if found.is_empty() {
        return Err(Error::invalid(
            "仓库中未找到支持的对应 skill；仅扫描已支持 Agent 的标准目录、.agents/skills、skills 和仓库根目录",
        ));
    }
    let relative = |directory: &Path| {
        directory
            .strip_prefix(temp.path())
            .unwrap()
            .components()
            .skip(1)
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    };
    found.sort_by_key(|directory| relative(directory));
    use super::variants::{self, GENERIC};
    let candidates = found
        .iter()
        .map(|directory| {
            let fm = scanner::parse_frontmatter(&directory.join("SKILL.md"));
            let validation = (|| -> Result<String> {
                scanner::validate_sync_source(directory)?;
                if fm.malformed {
                    return Err(Error::invalid("Skill 的 YAML 格式无效"));
                }
                scanner::dir_content_hash(directory)
            })();
            if directory.join(variants::DIRECTORY).exists() {
                return Err(Error::invalid("仓库包含保留的变体目录"));
            }
            let repository_path = relative(directory);
            Ok(CatalogCandidate {
                variant_key: Some(
                    variants::repository_key(&repository_path)
                        .ok_or_else(|| Error::invalid("Skill 来源不在支持的目录中"))?,
                ),
                error: validation.as_ref().err().map(|e| {
                    format!(
                        "{}：{e}",
                        if repository_path.is_empty() {
                            "仓库根目录"
                        } else {
                            &repository_path
                        }
                    )
                }),
                repository_path,
                description: fm.description,
                content_hash: validation.unwrap_or_default(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if selected
        .iter()
        .any(|p| !candidates.iter().any(|c| &c.repository_path == p))
    {
        return Err(Error::invalid("所选 skill 目录已失效，请重新选择安装来源"));
    }
    for candidate in &candidates {
        if selected.contains(&candidate.repository_path) {
            if let Some(error) = &candidate.error {
                return Err(Error::invalid(error));
            }
        }
    }
    if candidates.iter().all(|c| c.error.is_some()) {
        return Err(Error::invalid(
            candidates
                .iter()
                .filter_map(|c| c.error.as_deref())
                .collect::<Vec<_>>()
                .join("；"),
        ));
    }
    // Require acknowledgement before installing a partial, healthy collection.
    if selected.is_empty() && candidates.iter().any(|c| c.error.is_some()) {
        return Ok(CatalogPreparation::SelectionRequired(candidates));
    }
    let mut chosen = Vec::new();
    let keys: std::collections::BTreeSet<_> = candidates
        .iter()
        .filter(|c| c.error.is_none())
        .filter_map(|c| c.variant_key.clone())
        .collect();
    for key in keys {
        let bucket: Vec<_> = candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| c.error.is_none() && c.variant_key.as_deref() == Some(&key))
            .collect();
        let explicit: Vec<_> = bucket
            .iter()
            .filter(|(_, c)| selected.contains(&c.repository_path))
            .collect();
        if explicit.len() > 1 {
            return Err(Error::invalid("每个 Agent 只能选择一个 Skill 变体"));
        }
        if let Some((index, _)) = explicit.first() {
            chosen.push(*index);
        } else if bucket.len() == 1 {
            chosen.push(bucket[0].0);
        } else {
            return Ok(CatalogPreparation::SelectionRequired(candidates));
        }
    }
    chosen.sort_by_key(|i| (candidates[*i].variant_key.as_deref() != Some(GENERIC), *i));
    let primary = chosen[0];
    let repository_path = candidates[primary].repository_path.clone();
    let bundle = chosen.len() > 1
        || candidates[primary]
            .variant_key
            .as_deref()
            .is_some_and(|k| k != GENERIC);
    let mut variant_records = Vec::new();
    let directory = if bundle {
        let root = temp.path().join(".assembled");
        linker::copy_tree(&found[primary], &root)?;
        for index in chosen {
            let c = &candidates[index];
            let key = c.variant_key.clone().unwrap_or_else(|| GENERIC.into());
            let path = variants::payload(&root, &key)?;
            fs::create_dir_all(path.parent().unwrap()).map_err(|e| Error::io(&path, e))?;
            linker::copy_tree(&found[index], &path)?;
            variant_records.push(crate::models::config::SkillVariant {
                key,
                repository_path: c.repository_path.clone(),
                content_hash: c.content_hash.clone(),
            });
        }
        root
    } else {
        found[primary].clone()
    };
    Ok(CatalogPreparation::Ready(PreparedSkill {
        _temp: temp,
        directory,
        source: source.into(),
        skill_id: skill_id.into(),
        repository_path,
        variants: variant_records,
    }))
}

impl Studio {
    /// 本地目录导入：内容没变时返回未变化；Hub 副本没被用户改过时跟着更新；
    /// 用户改过就拒绝覆盖。
    pub fn import_local_skill(
        &self,
        config: &mut AppConfig,
        prepared: &PreparedSkill,
    ) -> Result<ImportOutcome> {
        validate_prepared(prepared)?;
        if !prepared.source.starts_with("local:") {
            return Err(Error::invalid("不是本地 skill 来源"));
        }
        let hub = self.store().hub_dir(config);
        let roots: Vec<_> = crate::models::agent::AGENTS
            .iter()
            .flat_map(|a| a.resolved_global_roots(&config.settings.agent_dir_overrides))
            .collect();
        linker::ensure_distinct_roots(&hub, &roots)?;
        let target = hub.join(&prepared.skill_id);
        let id = skill_id_for(&target);
        scanner::validate_sync_source(&prepared.directory)?;
        let hash = scanner::dir_content_hash(&prepared.directory)?;

        if target.symlink_metadata().is_ok() {
            let record = config
                .skill_installations
                .get(&id)
                .filter(|record| same_installation(record, prepared))
                .ok_or_else(|| Error::invalid("Hub 已有同名 skill，未覆盖现有内容"))?
                .clone();
            let current = scanner::dir_content_hash(&target)?;
            if current == hash {
                return Ok(ImportOutcome {
                    skill: self.installed_view(config, &id)?,
                    changed: false,
                });
            }
            if current != record.content_hash {
                return Err(Error::invalid(format!(
                    "Hub 里的「{}」已被修改，未覆盖；请先移出 Hub 再重新导入",
                    prepared.skill_id
                )));
            }
            let mut next = config.clone();
            next.skill_installations
                .insert(id.clone(), installation(prepared, &hash));
            let skill =
                self.install_prepared(config, prepared, &id, &target, &hub, &hash, next, true)?;
            return Ok(ImportOutcome {
                skill,
                changed: true,
            });
        }

        let mut next = config.clone();
        next.skill_installations
            .insert(id.clone(), installation(prepared, &hash));
        let skill =
            self.install_prepared(config, prepared, &id, &target, &hub, &hash, next, false)?;
        Ok(ImportOutcome {
            skill,
            changed: true,
        })
    }

    pub fn install_catalog_skill(
        &self,
        config: &mut AppConfig,
        prepared: &PreparedSkill,
    ) -> Result<Skill> {
        validate_prepared(prepared)?;
        if prepared.source.starts_with("local:") {
            return Ok(self.import_local_skill(config, prepared)?.skill);
        }
        let hub = self.store().hub_dir(config);
        let roots: Vec<_> = crate::models::agent::AGENTS
            .iter()
            .flat_map(|a| a.resolved_global_roots(&config.settings.agent_dir_overrides))
            .collect();
        linker::ensure_distinct_roots(&hub, &roots)?;
        let target = hub.join(&prepared.skill_id);
        let id = skill_id_for(&target);
        if target.symlink_metadata().is_ok() {
            if config
                .skill_installations
                .get(&id)
                .is_some_and(|record| same_installation(record, prepared))
            {
                return self.installed_view(config, &id);
            }
            return Err(Error::invalid("Hub 已有同名 skill，未覆盖现有内容"));
        }
        scanner::validate_sync_source(&prepared.directory)?;
        let hash = scanner::dir_content_hash(&prepared.directory)?;
        let mut next = config.clone();
        next.skill_installations
            .insert(id.clone(), installation(prepared, &hash));
        self.install_prepared(config, prepared, &id, &target, &hub, &hash, next, false)
    }

    fn installed_view(&self, config: &AppConfig, id: &str) -> Result<Skill> {
        self.scan_skills(config)?
            .into_iter()
            .find(|view| view.skill.id == id && view.diagnostics.is_empty())
            .map(|view| view.skill)
            .ok_or_else(|| Error::invalid("Hub 已有同名 skill，未覆盖现有内容"))
    }

    #[allow(clippy::too_many_arguments)]
    fn install_prepared(
        &self,
        config: &mut AppConfig,
        prepared: &PreparedSkill,
        id: &str,
        target: &Path,
        hub: &Path,
        hash: &str,
        next: AppConfig,
        replace: bool,
    ) -> Result<Skill> {
        let mut tx = Transaction::begin(self.store().dir().join("migration.json"))?;
        let operation = (|| -> Result<Skill> {
            fs::create_dir_all(hub).map_err(|e| Error::io(hub, e))?;
            if !replace && target.symlink_metadata().is_ok() {
                return Err(Error::invalid("Hub 已有同名 skill，未覆盖现有内容"));
            }
            tx.reserve(target)?;
            linker::copy_tree(&prepared.directory, target)?;
            if scanner::dir_content_hash(target)? != hash {
                return Err(Error::invalid("安装内容校验失败"));
            }
            let skill = self
                .scan_skills(&next)?
                .into_iter()
                .find(|view| view.skill.id == id)
                .ok_or_else(|| Error::invalid("安装后无法识别 skill"))?
                .skill;
            self.store().reserve_config(&mut tx, &next)?;
            self.save_config(&next)?;
            Ok(skill)
        })();
        match operation {
            Ok(skill) => {
                tx.commit()?;
                *config = next;
                Ok(skill)
            }
            Err(error) => {
                tx.rollback()
                    .map_err(|e| Error::Other(format!("{error}；回滚未完成：{e}")))?;
                Err(error)
            }
        }
    }
}

fn same_installation(record: &SkillInstallation, prepared: &PreparedSkill) -> bool {
    record.source == prepared.source
        && record.skill_id == prepared.skill_id
        && record.repository_path == prepared.repository_path
        && record
            .variants
            .iter()
            .map(|v| (&v.key, &v.repository_path))
            .eq(prepared
                .variants
                .iter()
                .map(|v| (&v.key, &v.repository_path)))
}

fn installation(prepared: &PreparedSkill, hash: &str) -> SkillInstallation {
    SkillInstallation {
        source: prepared.source.clone(),
        skill_id: prepared.skill_id.clone(),
        repository_path: prepared.repository_path.clone(),
        installed_at: linker::now_secs(),
        content_hash: hash.to_string(),
        variants: prepared.variants.clone(),
    }
}

fn validate_prepared(prepared: &PreparedSkill) -> Result<()> {
    if let Some(source) = prepared.source.strip_prefix("local:") {
        if !Path::new(source).is_absolute()
            || prepared.skill_id.is_empty()
            || prepared.skill_id.starts_with('.')
            || prepared.skill_id.contains(['/', '\\', ':'])
            || Path::new(&prepared.skill_id).components().count() != 1
        {
            return Err(Error::invalid("本地 skill 路径无效"));
        }
    } else {
        validate_coordinates(&prepared.source, &prepared.skill_id)?;
    }
    let mut keys = std::collections::HashSet::new();
    for variant in &prepared.variants {
        if !keys.insert(&variant.key)
            || super::variants::repository_key(&variant.repository_path).as_deref()
                != Some(&variant.key)
        {
            return Err(Error::invalid("Skill 变体来源或标识无效"));
        }
        let payload = super::variants::payload(&prepared.directory, &variant.key)?;
        scanner::validate_sync_source(&payload)?;
        if scanner::parse_frontmatter(&payload.join("SKILL.md")).malformed
            || scanner::dir_content_hash(&payload)? != variant.content_hash
        {
            return Err(Error::invalid("Skill 变体内容校验失败"));
        }
    }
    if prepared.variants.is_empty() && prepared.directory.join(super::variants::DIRECTORY).exists()
    {
        return Err(Error::invalid("变体安装包缺少来源记录，请从仓库重新安装"));
    }
    Ok(())
}

/// 本地导入的结果：`changed` 为 false 说明 Hub 里已经是同一份内容。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub skill: Skill,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSkill {
    pub path: PathBuf,
    pub name: String,
    pub description: Option<String>,
    pub error: Option<String>,
}

/// Discover skills in a selected directory without following directory links.
pub fn discover_local(root: &Path) -> Result<Vec<LocalSkill>> {
    let root = fs::canonicalize(root).map_err(|e| Error::io(root, e))?;
    let mut found = Vec::new();
    let mut budget = MAX_ENTRIES;
    fn walk(dir: &Path, depth: usize, budget: &mut usize, out: &mut Vec<LocalSkill>) -> Result<()> {
        if depth > scanner::MAX_SCAN_DEPTH {
            return Err(Error::invalid("目录层级过深，请选择更具体的目录"));
        }
        if dir.join("SKILL.md").is_file() {
            let fm = scanner::parse_frontmatter(&dir.join("SKILL.md"));
            out.push(LocalSkill {
                path: dir.to_path_buf(),
                name: fm.name.unwrap_or_else(|| {
                    dir.file_name().unwrap_or_default().to_string_lossy().into()
                }),
                description: fm.description,
                error: fm.malformed.then(|| "SKILL.md 的 YAML 格式无效".into()),
            });
            return Ok(());
        }
        for entry in fs::read_dir(dir).map_err(|e| Error::io(dir, e))? {
            let entry = entry.map_err(|e| Error::io(dir, e))?;
            if *budget == 0 {
                return Err(Error::invalid("目录文件数量过多，请选择更具体的目录"));
            }
            *budget -= 1;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') || name == "node_modules" || name == "target"
            {
                continue;
            }
            if entry
                .file_type()
                .map_err(|e| Error::io(entry.path(), e))?
                .is_dir()
            {
                walk(&entry.path(), depth + 1, budget, out)?;
            }
        }
        Ok(())
    }
    walk(&root, 0, &mut budget, &mut found)?;
    found.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(found)
}

/// Stage an independent copy, retaining the user's original files.
pub fn prepare_local(path: &Path) -> Result<PreparedSkill> {
    let source = fs::canonicalize(path).map_err(|e| Error::io(path, e))?;
    scanner::validate_sync_source(&source)?;
    if scanner::parse_frontmatter(&source.join("SKILL.md")).malformed {
        return Err(Error::invalid("SKILL.md 的 YAML 格式无效"));
    }
    let skill_id = source
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && !n.starts_with('.') && !n.contains(['/', '\\', ':']))
        .ok_or_else(|| Error::invalid("Skill 目录名称无效"))?
        .to_owned();
    // Limit the copied tree and reject links so the Hub copy is self-contained.
    fn check(dir: &Path, depth: usize, count: &mut usize, size: &mut u64) -> Result<()> {
        if depth > 32 {
            return Err(Error::invalid("Skill 目录层级过深"));
        }
        for entry in fs::read_dir(dir).map_err(|e| Error::io(dir, e))? {
            let entry = entry.map_err(|e| Error::io(dir, e))?;
            let meta =
                fs::symlink_metadata(entry.path()).map_err(|e| Error::io(entry.path(), e))?;
            *count += 1;
            *size = size.saturating_add(meta.len());
            if *count > MAX_ENTRIES || *size > MAX_EXPANDED {
                return Err(Error::invalid("Skill 文件数量或大小超过限制"));
            }
            if meta.is_symlink() || (!meta.is_dir() && !meta.is_file()) {
                return Err(Error::invalid("Skill 包含链接或特殊文件，暂不支持导入"));
            }
            if meta.is_dir() {
                check(&entry.path(), depth + 1, count, size)?;
            }
        }
        Ok(())
    }
    check(&source, 0, &mut 0, &mut 0)?;
    let temp = tempfile::tempdir().map_err(|e| Error::Other(e.to_string()))?;
    let directory = temp.path().join(&skill_id);
    let before = scanner::dir_content_hash(&source)?;
    linker::copy_tree(&source, &directory)?;
    if scanner::dir_content_hash(&directory)? != before {
        return Err(Error::invalid("本地内容在导入期间发生变化，请重试"));
    }
    Ok(PreparedSkill {
        _temp: temp,
        directory,
        source: format!("local:{}", source.display()),
        skill_id,
        repository_path: String::new(),
        variants: Vec::new(),
    })
}
