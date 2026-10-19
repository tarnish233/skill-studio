//! Configuration management remains available while the gateway is stopped.
use crate::{
    config::{self, Server},
    native,
};
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use skill_studio_core::fs::atomic;
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub mode: String,
    pub definition: Value,
    #[serde(default)]
    pub oauth: bool,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub id: String,
    pub agent: String,
    pub path: PathBuf,
    pub project: Option<String>,
    pub key: String,
    pub original: Option<Value>,
    pub installed: Value,
}
#[derive(Clone)]
pub struct Target {
    pub agent: String,
    pub path: PathBuf,
    pub project: Option<String>,
    pub key: String,
    pub expected: Option<Value>,
}
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    /// Keep Hub records while native Agent connections are restored to their originals.
    #[serde(default)]
    pub suspended: bool,
    #[serde(default)]
    pub groups: Vec<groups::Group>,
    /// agent → 当前叠加的分组记录。旧版是单个对象，见 [`de_active_groups`]。
    #[serde(
        default,
        rename = "activeGroups",
        deserialize_with = "de_active_groups"
    )]
    pub active_groups: BTreeMap<String, Vec<groups::ActiveGroup>>,
    #[serde(default)]
    pub backups: Vec<Backup>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub id: String,
    pub files: Vec<BackupFile>,
    pub created_at: u64,
    pub catalog_backup_path: PathBuf,
    pub before_catalog_hash: String,
    pub after_catalog_hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
}
fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn catalog_state_hash(catalog: &Catalog) -> Result<String> {
    let mut state = catalog.clone();
    state.backups.clear();
    Ok(digest(&serde_json::to_string(&state)?))
}
pub fn restore_backup(dir: &Path, id: &str) -> Result<()> {
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    let backup = catalog
        .backups
        .iter()
        .find(|b| b.id == id)
        .context("备份不存在")?
        .clone();
    if catalog_state_hash(&catalog)? != backup.after_catalog_hash {
        bail!("MCP 托管状态已在备份后修改，停止恢复以保护新配置");
    }
    let old_catalog = read_text(&backup.catalog_backup_path)?.context("托管状态备份不存在")?;
    if digest(&old_catalog) != backup.before_catalog_hash {
        bail!("托管状态备份已变化，停止恢复");
    }
    let old_catalog: Catalog = serde_json::from_str(&old_catalog)?;
    if old_catalog.suspended {
        bail!("此备份包含已关闭的 MCP 管理状态，请通过设置开关恢复接入");
    }
    catalog.backups.retain(|b| b.id != id);
    catalog.entries = old_catalog.entries;
    catalog.groups = old_catalog.groups;
    catalog.active_groups = old_catalog.active_groups;
    let mut files = BTreeMap::new();
    for file in backup.files {
        let current = read_text(&file.path)?;
        if current.as_deref().map(digest) != file.after_hash {
            bail!(
                "配置已在备份后修改，停止恢复以保护新内容：{}",
                file.path.display()
            );
        }
        let original = match file.backup_path {
            Some(path) => {
                let text = read_text(&path)?.context("备份文件不存在")?;
                if Some(digest(&text)) != file.before_hash {
                    bail!("备份文件已变化，停止恢复");
                }
                Some(text)
            }
            None => {
                if file.before_hash.is_some() {
                    bail!("备份记录无效");
                }
                None
            }
        };
        files.insert(
            file.path.clone(),
            Change {
                path: file.path,
                before: current,
                after: original,
            },
        );
    }
    commit(dir, files, &catalog)
}

fn lock(dir: &Path) -> Result<std::fs::File> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("management.lock"))?;
    f.lock_exclusive()?;
    recover(dir)?;
    Ok(f)
}
pub fn read(dir: &Path) -> Result<Catalog> {
    let _lock = lock(dir)?;
    read_unlocked(dir)
}
fn read_unlocked(dir: &Path) -> Result<Catalog> {
    if let Some(catalog) = atomic::read_json_file(&dir.join("catalog.json"))? {
        return Ok(catalog);
    }
    let legacy = config::read(dir)?;
    let mut entries = vec![];
    for server in legacy.servers {
        let mut definition = serde_json::to_value(&server.connection)?;
        let kind = definition
            .as_object_mut()
            .unwrap()
            .remove("transport")
            .unwrap();
        definition["type"] = kind;
        let mut bindings = vec![];
        for b in legacy.bindings.iter().filter(|b| b.id == server.id) {
            let key = format!("studio-{}", server.id);
            let text = read_text(&b.path)?.unwrap_or_default();
            if let Some(installed) = native::entry(&text, &b.agent, None, &key)? {
                // Retain an expected snapshot only for a recognizable legacy adapter.
                if installed["args"] != json!(["--mcp-client", server.id, dir]) {
                    bail!("旧 MCP 接入已修改，请先核对 {}", b.path.display());
                }
                bindings.push(Binding {
                    id: format!("legacy-{}-{}", server.id, bindings.len()),
                    agent: b.agent.clone(),
                    path: b.path.clone(),
                    project: None,
                    key,
                    original: None,
                    installed,
                });
            }
        }
        entries.push(Entry {
            id: server.id,
            name: server.name,
            mode: "gateway".into(),
            definition,
            oauth: server.oauth,
            client_id: server.client_id,
            scopes: server.scopes,
            bindings,
        });
    }
    Ok(Catalog {
        entries,
        ..Default::default()
    })
}

fn ensure_active(catalog: &Catalog) -> Result<()> {
    if catalog.suspended {
        bail!("MCP 管理已关闭，请先在设置中开启");
    }
    Ok(())
}

/// Restore only Studio-owned native MCP entries. Hub metadata remains available
/// for a later opt-in, and the existing journal rolls back every file together.
pub fn set_enabled(dir: &Path, enabled: bool) -> Result<()> {
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    if catalog.suspended == !enabled {
        return Ok(());
    }
    let mut files = BTreeMap::new();
    if enabled {
        for entry in &catalog.entries {
            for binding in &entry.bindings {
                stage_binding(
                    &mut files,
                    binding,
                    &binding.original,
                    &Some(binding.installed.clone()),
                )?;
            }
        }
        for binding in catalog
            .active_groups
            .values()
            .flatten()
            .flat_map(|g| &g.bindings)
        {
            stage_binding(
                &mut files,
                binding,
                &binding.original,
                &Some(binding.installed.clone()),
            )?;
        }
    } else {
        for binding in catalog
            .active_groups
            .values()
            .flatten()
            .flat_map(|g| &g.bindings)
        {
            stage_binding(
                &mut files,
                binding,
                &Some(binding.installed.clone()),
                &binding.original,
            )?;
        }
        for entry in &catalog.entries {
            for binding in &entry.bindings {
                stage_binding(
                    &mut files,
                    binding,
                    &Some(binding.installed.clone()),
                    &binding.original,
                )?;
            }
        }
    }
    catalog.suspended = !enabled;
    commit(dir, files, &catalog)
}

fn stage_binding(
    files: &mut BTreeMap<PathBuf, Change>,
    binding: &Binding,
    expected: &Option<Value>,
    next: &Option<Value>,
) -> Result<()> {
    stage(
        files,
        &Target {
            agent: binding.agent.clone(),
            path: binding.path.clone(),
            project: binding.project.clone(),
            key: binding.key.clone(),
            expected: None,
        },
        expected,
        next,
    )
    .with_context(|| {
        format!(
            "{} 中的 MCP「{}」与托管记录不一致",
            binding.path.display(),
            binding.key
        )
    })
}
pub fn gateway_server(entry: &Entry) -> Result<Server> {
    let mut server =
        crate::discovery::normalize(&entry.name, &entry.id, &entry.definition, "claude")?;
    server.oauth = entry.oauth;
    server.client_id = entry.client_id.clone();
    server.scopes = entry.scopes.clone();
    config::validate(&server)?;
    Ok(server)
}
/// Build a one-shot Studio probe from an Agent-owned definition. Agent-specific
/// extensions stay in the catalog and native config, but are not gateway fields.
pub fn direct_probe_server(entry: &Entry) -> Result<(Server, std::time::Duration)> {
    native::validate(&entry.definition)?;
    let input = entry.definition.as_object().unwrap();
    if input.get("type").and_then(Value::as_str) == Some("sse") {
        bail!("Studio 暂不支持 SSE 连接测试，请在 Agent 中测试此服务");
    }
    let mut definition = serde_json::Map::new();
    let http = input.get("type").and_then(Value::as_str) != Some("stdio");
    let fields: &[&str] = if http {
        &["type", "url", "headers", "http_headers"]
    } else {
        &["type", "command", "args", "env", "cwd"]
    };
    for field in fields {
        if let Some(value) = input.get(*field) {
            definition.insert((*field).into(), value.clone());
        }
    }
    if let Some(headers) = definition.remove("http_headers") {
        definition.insert("headers".into(), headers);
    }
    let mut value = Value::Object(definition);
    let map = value.as_object_mut().unwrap();
    map.remove("type");
    map.insert("id".into(), json!(entry.id));
    map.insert("name".into(), json!(entry.name));
    map.insert(
        "transport".into(),
        json!(if http { "http" } else { "stdio" }),
    );
    let server: Server = serde_json::from_value(value)
        .map_err(|_| anyhow::anyhow!("连接配置字段类型无效，请检查 Agent 配置"))?;
    config::validate(&server)?;
    if let config::Connection::Stdio { cwd: Some(cwd), .. } = &server.connection {
        if !cwd.is_absolute() {
            bail!("相对工作目录依赖 Agent 的启动位置，请在 Agent 中测试此服务");
        }
    }
    let timeout = input
        .get("startup_timeout_sec")
        .and_then(Value::as_u64)
        .filter(|seconds| *seconds > 0)
        .unwrap_or(30);
    Ok((server, std::time::Duration::from_secs(timeout)))
}
pub struct Projection {
    pub servers: Vec<Server>,
    pub suspended: bool,
}

pub fn projection(dir: &Path) -> Result<Option<Projection>> {
    // The catalog is the transaction's final write. Never inspect partial native writes.
    let Some(catalog): Option<Catalog> = atomic::read_json_file(&dir.join("catalog.json"))? else {
        return Ok(None);
    };
    if catalog.suspended {
        return Ok(Some(Projection {
            servers: vec![],
            suspended: true,
        }));
    }
    Ok(Some(Projection {
        servers: catalog
            .entries
            .iter()
            .filter(|e| e.mode == "gateway")
            .map(gateway_server)
            .collect::<Result<_>>()?,
        suspended: false,
    }))
}
pub fn save(
    dir: &Path,
    mut entry: Entry,
    targets: Vec<Target>,
    executable: &Path,
    expected_entry: Option<&Entry>,
) -> Result<()> {
    let _lock = lock(dir)?;
    if entry.id.is_empty()
        || !entry
            .id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("无效的 MCP ID");
    }
    if entry.name.trim().is_empty() {
        bail!("请填写名称");
    }
    native::validate(&entry.definition)?;
    if !matches!(entry.mode.as_str(), "direct" | "gateway") {
        bail!("请选择直连或网关");
    }
    if entry.mode == "gateway" {
        gateway_server(&entry)?;
    }
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    let old = catalog.entries.iter().find(|e| e.id == entry.id).cloned();
    if serde_json::to_value(&old)? != serde_json::to_value(expected_entry)? {
        bail!("MCP 已被其他操作修改，请关闭编辑器后重新打开");
    }
    let mut files = BTreeMap::new();
    let mut installed = vec![];
    let mut seen = HashSet::new();
    for target in targets {
        let identity = (
            target.agent.clone(),
            target.path.clone(),
            target.project.clone(),
            target.key.clone(),
        );
        if !seen.insert(identity) {
            continue;
        }
        if catalog
            .entries
            .iter()
            .filter(|e| e.id != entry.id)
            .flat_map(|e| &e.bindings)
            .any(|b| b.path == target.path && b.project == target.project && b.key == target.key)
        {
            bail!("目标条目已由其他 MCP 管理");
        }
        let prior = old.as_ref().and_then(|e| {
            e.bindings.iter().find(|b| {
                b.path == target.path && b.project == target.project && b.key == target.key
            })
        });
        if let Some(binding) = catalog
            .active_groups
            .values()
            .flatten()
            .flat_map(|g| &g.bindings)
            .find(|b| b.path == target.path && b.project == target.project && b.key == target.key)
        {
            let prior = prior.context("目标正由分组使用，请先停用分组再改变接入")?;
            stage(
                &mut files,
                &target,
                &Some(binding.installed.clone()),
                &Some(binding.installed.clone()),
            )?;
            installed.push(prior.clone());
            continue;
        }
        let text = match files.get(&target.path) {
            Some(change) => change.after.clone().unwrap_or_default(),
            None => read_text(&target.path)?.unwrap_or_default(),
        };
        native::check_activation(&text, &target.agent, &target.key)?;
        let value = if entry.mode == "gateway" {
            let mut adapter =
                json!({"type":"stdio","command":executable,"args":["--mcp-client",entry.id,dir]});
            if entry.definition["enabled"].as_bool() == Some(false) && target.agent != "claude" {
                adapter["enabled"] = json!(false);
            }
            native::for_agent(&adapter, &target.agent)?
        } else {
            native::for_agent(&entry.definition, &target.agent)?
        };
        let expected = prior
            .map(|b| Some(b.installed.clone()))
            .unwrap_or(target.expected.clone());
        stage(&mut files, &target, &expected, &Some(value.clone()))?;
        installed.push(Binding {
            id: prior
                .map(|b| b.id.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            agent: target.agent,
            path: target.path,
            project: target.project,
            key: target.key,
            original: prior.map(|b| b.original.clone()).unwrap_or(expected),
            installed: value,
        });
    }
    if let Some(old) = old {
        for binding in old
            .bindings
            .iter()
            .filter(|b| !installed.iter().any(|next| next.id == b.id))
        {
            if catalog
                .active_groups
                .values()
                .flatten()
                .flat_map(|g| &g.bindings)
                .any(|b| {
                    b.path == binding.path && b.project == binding.project && b.key == binding.key
                })
            {
                bail!("接入正由分组使用，请先停用分组再移除接入");
            }
            let target = Target {
                agent: binding.agent.clone(),
                path: binding.path.clone(),
                project: binding.project.clone(),
                key: binding.key.clone(),
                expected: None,
            };
            stage(&mut files, &target, &Some(binding.installed.clone()), &None)?;
        }
    }
    entry.bindings = installed;
    catalog.entries.retain(|e| e.id != entry.id);
    catalog.entries.push(entry);
    commit(dir, files, &catalog)
}
/// 删除托管、移除项目接入时原样留下的接入：文件里同名的配置已被其他程序改写，不再是 Studio 写的
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Foreign {
    pub agent: String,
    pub path: PathBuf,
    pub project: Option<String>,
    pub key: String,
}
/// 删除托管、移除项目接入的结果
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Removal {
    /// Studio 实际改写的接入数（撤回，或还原成原配置）；为 0 时没有写任何配置文件
    pub removed: usize,
    /// 已被其他程序改写、原样留下的接入。它们交还给用户，之后作为原有配置出现在扫描结果里
    pub foreign: Vec<Foreign>,
}
/// 撤回一条接入，写成 `next`。文件里还是 Studio 写的、或已被删掉，就照常处理；被其他程序
/// 改写过的不碰——那已经不归 Studio，删了会毁掉别人的配置——记进 `removal.foreign`。
/// 过去这里一律要求「仍是 Studio 写的」，被别的工具改写后托管就再也删不掉。
fn release(
    files: &mut BTreeMap<PathBuf, Change>,
    b: &Binding,
    next: &Option<Value>,
    removal: &mut Removal,
) -> Result<()> {
    let text = match files.get(&b.path) {
        Some(change) => change.after.clone(),
        None => read_text(&b.path)?,
    };
    let current = native::entry(
        text.as_deref().unwrap_or_default(),
        &b.agent,
        b.project.as_deref(),
        &b.key,
    )?;
    if current.as_ref().is_some_and(|c| *c != b.installed) {
        removal.foreign.push(Foreign {
            agent: b.agent.clone(),
            path: b.path.clone(),
            project: b.project.clone(),
            key: b.key.clone(),
        });
        return Ok(());
    }
    if current.is_none() && next.is_none() {
        // 已被删掉，也不用还原：没有要写的
        return Ok(());
    }
    let target = Target {
        agent: b.agent.clone(),
        path: b.path.clone(),
        project: b.project.clone(),
        key: b.key.clone(),
        expected: None,
    };
    stage(files, &target, &current, next)?;
    removal.removed += 1;
    Ok(())
}
pub fn remove(dir: &Path, id: &str, restore: bool) -> Result<Removal> {
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    let entry = catalog
        .entries
        .iter()
        .find(|e| e.id == id)
        .context("MCP 不存在")?;
    if catalog
        .active_groups
        .values()
        .flatten()
        .any(|g| g.entries.iter().any(|e| e.id == id))
    {
        bail!("请先停用引用此 MCP 的分组");
    }
    let mut files = BTreeMap::new();
    let mut removal = Removal::default();
    for b in &entry.bindings {
        let next = if restore { b.original.clone() } else { None };
        release(&mut files, b, &next, &mut removal)?;
    }
    catalog.entries.retain(|e| e.id != id);
    commit(dir, files, &catalog)?;
    Ok(removal)
}
/// Remove only Studio-owned bindings for one registered project in a single transaction.
/// Bindings another program has since rewritten are dropped without touching their files.
pub fn remove_project_bindings(dir: &Path, root: &Path) -> Result<Removal> {
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    let local_root = root.to_string_lossy();
    let project_files: Vec<_> = crate::agents::PROJECT_FILES
        .iter()
        .map(|p| root.join(p))
        .collect();
    let bindings: Vec<Binding> = catalog
        .entries
        .iter()
        .flat_map(|e| &e.bindings)
        .filter(|b| {
            b.project.as_deref() == Some(local_root.as_ref()) || project_files.contains(&b.path)
        })
        .cloned()
        .collect();
    if bindings.is_empty() {
        return Ok(Removal::default());
    }
    if bindings.iter().any(|b| {
        catalog
            .active_groups
            .values()
            .flatten()
            .flat_map(|g| &g.bindings)
            .any(|active| {
                active.path == b.path && active.project == b.project && active.key == b.key
            })
    }) {
        bail!("项目接入正由分组使用，请先停用对应分组");
    }
    let mut files = BTreeMap::new();
    let mut removal = Removal::default();
    for b in &bindings {
        release(&mut files, b, &b.original, &mut removal)?;
    }
    let identities: HashSet<_> = bindings
        .iter()
        .map(|b| (b.path.clone(), b.project.clone(), b.key.clone()))
        .collect();
    for entry in &mut catalog.entries {
        entry
            .bindings
            .retain(|b| !identities.contains(&(b.path.clone(), b.project.clone(), b.key.clone())));
    }
    commit(dir, files, &catalog)?;
    Ok(removal)
}
/// Delete scanned, unmanaged entries without disturbing other configuration keys.
pub fn remove_sources(dir: &Path, targets: Vec<Target>) -> Result<()> {
    let _lock = lock(dir)?;
    let catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    if targets.is_empty() {
        bail!("请选择要删除的 MCP 来源");
    }
    let mut files = BTreeMap::new();
    for target in targets {
        if target.expected.is_none() {
            bail!("缺少来源配置快照，请刷新后重试");
        }
        if catalog
            .entries
            .iter()
            .flat_map(|e| &e.bindings)
            .chain(
                catalog
                    .active_groups
                    .values()
                    .flatten()
                    .flat_map(|g| &g.bindings),
            )
            .any(|b| b.path == target.path && b.project == target.project && b.key == target.key)
        {
            bail!("此来源已被托管或由分组使用，请刷新后从对应入口管理");
        }
        stage(&mut files, &target, &target.expected, &None)?;
    }
    commit(dir, files, &catalog)
}

#[derive(Clone, Serialize, Deserialize)]
struct Change {
    path: PathBuf,
    before: Option<String>,
    after: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    changes: Vec<Change>,
    catalog: Change,
}
fn read_text(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
/// 旧版目录文件里每个 agent 只有一个分组对象；现在可以叠加多个，两种形状都读得进来。
fn de_active_groups<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, Vec<groups::ActiveGroup>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        Many(Vec<groups::ActiveGroup>),
        One(groups::ActiveGroup),
    }
    BTreeMap::<String, OneOrMany>::deserialize(deserializer).map(|map| {
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
fn stage(
    files: &mut BTreeMap<PathBuf, Change>,
    target: &Target,
    expected: &Option<Value>,
    next: &Option<Value>,
) -> Result<()> {
    if !files.contains_key(&target.path) {
        let before = read_text(&target.path)?;
        files.insert(
            target.path.clone(),
            Change {
                path: target.path.clone(),
                after: before.clone(),
                before,
            },
        );
    }
    let change = files.get_mut(&target.path).unwrap();
    change.after = Some(native::patch(
        change.after.as_deref().unwrap_or_default(),
        &target.agent,
        target.project.as_deref(),
        &target.key,
        expected,
        next,
    )?);
    Ok(())
}
fn commit(dir: &Path, files: BTreeMap<PathBuf, Change>, catalog: &Catalog) -> Result<()> {
    let mut catalog = catalog.clone();
    let catalog_path = dir.join("catalog.json");
    let mut previous_catalog = read_unlocked(dir)?;
    previous_catalog.backups.clear();
    let previous_catalog_text = serde_json::to_string(&previous_catalog)?;
    let after_catalog_hash = catalog_state_hash(&catalog)?;
    let changes: Vec<Change> = files
        .into_values()
        .filter(|c| c.before != c.after)
        .collect();
    let backup = if changes.is_empty() {
        None
    } else {
        let id = uuid::Uuid::new_v4().to_string();
        // 备份集中放在 Studio 目录，不写进 Agent 配置和项目目录（见 backups.rs）
        let folder = backups::folder(dir, &id);
        Some(Backup {
            files: changes
                .iter()
                .enumerate()
                .map(|(index, change)| BackupFile {
                    path: change.path.clone(),
                    backup_path: change
                        .before
                        .as_ref()
                        .map(|_| folder.join(backups::file_name(index, &change.path))),
                    before_hash: change.before.as_deref().map(digest),
                    after_hash: change.after.as_deref().map(digest),
                })
                .collect(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            catalog_backup_path: folder.join(backups::CATALOG),
            before_catalog_hash: digest(&previous_catalog_text),
            after_catalog_hash,
            id,
        })
    };
    if let Some(backup) = &backup {
        backups::create_folder(dir, &backup.id)?;
        atomic::atomic_write_private(
            &backup.catalog_backup_path,
            previous_catalog_text.as_bytes(),
        )?;
        catalog.backups.push(backup.clone());
    }
    let journal = Journal {
        changes,
        catalog: Change {
            before: read_text(&catalog_path)?,
            path: catalog_path,
            after: Some(serde_json::to_string_pretty(&catalog)?),
        },
    };
    for c in &journal.changes {
        if read_text(&c.path)? != c.before {
            bail!("配置已被修改，请刷新后重试：{}", c.path.display());
        }
        if let Some(before) = &c.before {
            let backup_path = backup
                .as_ref()
                .and_then(|b| b.files.iter().find(|f| f.path == c.path))
                .and_then(|f| f.backup_path.as_ref())
                .context("缺少备份记录")?;
            atomic::atomic_write_private(backup_path, before.as_bytes())?;
        }
    }
    atomic::write_json_file(&dir.join("management-transaction.json"), &journal)?;
    let outcome = (|| -> Result<()> {
        for c in journal
            .changes
            .iter()
            .chain(std::iter::once(&journal.catalog))
        {
            if read_text(&c.path)? != c.before {
                bail!("配置已被修改，请刷新后重试：{}", c.path.display());
            }
            if let Some(after) = &c.after {
                atomic::atomic_write_private(&c.path, after.as_bytes())?;
            } else {
                std::fs::remove_file(&c.path)?;
            }
        }
        Ok(())
    })();
    if let Err(error) = outcome {
        recover(dir).context("保存失败，自动恢复遇到外部修改，请保留事务文件并恢复配置")?;
        return Err(error);
    }
    std::fs::remove_file(dir.join("management-transaction.json"))?;
    Ok(())
}
fn recover(dir: &Path) -> Result<()> {
    let path = dir.join("management-transaction.json");
    let Some(journal): Option<Journal> = atomic::read_json_file(&path)? else {
        return Ok(());
    };
    if read_text(&journal.catalog.path)? == journal.catalog.after {
        std::fs::remove_file(path)?;
        return Ok(());
    }
    // Check every file before rollback; never overwrite edits made after interruption.
    for c in &journal.changes {
        let current = read_text(&c.path)?;
        if current != c.before && current != c.after {
            bail!("事务恢复检测到外部修改：{}", c.path.display());
        }
    }
    for c in journal.changes.iter().rev() {
        if read_text(&c.path)? == c.after {
            if let Some(before) = &c.before {
                atomic::atomic_write_private(&c.path, before.as_bytes())?;
            } else {
                std::fs::remove_file(&c.path)?;
            }
        }
    }
    std::fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
#[path = "management_tests.rs"]
mod tests;

pub mod backups;
pub mod groups;
