//! Shared MCP combinations, activated independently per agent. Metadata and all
//! native edits share the management journal.
pub use super::deploy::Untouched;
use super::deploy::{deploy, intact, issue, snapshot, Members, Scope};
use super::*;

/// A saved MCP combination, shared across agents. Each agent independently
/// decides whether this group is its current active combination (see
/// `Catalog::active_groups`), so the same group can be active for several
/// agents at once without being duplicated per agent.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub name: String,
    pub entry_ids: Vec<String>,
    /// Non-owning references captured from scanned sources; saving never adopts them.
    #[serde(default)]
    pub references: Vec<Entry>,
    #[serde(default)]
    pub sort_order: usize,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveGroup {
    pub group_id: String,
    pub entries: Vec<Entry>,
    pub bindings: Vec<Binding>,
}
/// 打开编辑时分组的样子：保存前核对，别处（另一个窗口、命令行）改过就不覆盖
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSnapshot {
    pub name: String,
    pub entry_ids: Vec<String>,
}
impl GroupSnapshot {
    /// 名称前后的空格和成员顺序不算修改
    fn matches(&self, group: &Group) -> bool {
        let set = |ids: &[String]| {
            ids.iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
        };
        self.name.trim() == group.name && set(&self.entry_ids) == set(&group.entry_ids)
    }
}
/// 正在使用这个分组的 Agent，按固定顺序
fn agents_using(catalog: &Catalog, id: &str) -> Vec<&'static str> {
    crate::agents::IDS
        .iter()
        .copied()
        .filter(|agent| {
            catalog
                .active_groups
                .get(*agent)
                .is_some_and(|records| records.iter().any(|g| g.group_id == id))
        })
        .collect()
}
/// 保存分组的名称和成员。`expected` 是打开编辑时的样子，与托管状态里的不同说明别处刚改过，
/// 拒绝保存以免覆盖；新建时为 `None`
pub fn save_group(
    dir: &Path,
    mut group: Group,
    imports: Vec<Entry>,
    expected: Option<&GroupSnapshot>,
) -> Result<()> {
    group.name = group.name.trim().into();
    if group.name.is_empty() || group.id.is_empty() {
        bail!("请填写分组名称");
    }
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    if let Some(expected) = expected {
        let Some(current) = catalog.groups.iter().find(|g| g.id == group.id) else {
            bail!("分组已被别处删除");
        };
        if !expected.matches(current) {
            bail!(
                "分组{}",
                skill_studio_core::services::store::CHANGED_ELSEWHERE
            );
        }
    }
    if catalog
        .groups
        .iter()
        .any(|g| g.name == group.name && g.id != group.id)
    {
        bail!("已存在同名分组");
    }
    // 空分组不能启用：正在用的分组被清空时先让用户停用，避免「存下了、写入又失败」的半截状态
    if group.entry_ids.is_empty() {
        let using = agents_using(&catalog, &group.id);
        if !using.is_empty() {
            let names: Vec<&str> = using.iter().map(|a| crate::agents::name(a)).collect();
            bail!(
                "分组「{}」正在 {} 上使用，清空成员前请先为这些 Agent 停用",
                group.name,
                names.join("、")
            );
        }
    }
    // Ignore references submitted in the group payload; only server-rescanned imports
    // or previously saved references can supply filesystem locations.
    let previous = catalog
        .groups
        .iter()
        .find(|g| g.id == group.id)
        .map(|g| g.references.clone())
        .unwrap_or_default();
    group.references =
        super::deploy::references(&catalog, &group.entry_ids, previous, imports, "分组")?;
    let mut seen = HashSet::new();
    group.entry_ids.retain(|id| seen.insert(id.clone()));
    // Keep missing members visible on edit, as with Skill groups.
    if let Some(i) = catalog.groups.iter().position(|g| g.id == group.id) {
        group.sort_order = catalog.groups[i].sort_order;
        catalog.groups[i] = group;
    } else {
        group.sort_order = catalog
            .groups
            .iter()
            .map(|g| g.sort_order + 1)
            .max()
            .unwrap_or(0);
        catalog.groups.push(group);
    }
    commit(dir, BTreeMap::new(), &catalog)
}
pub fn remove_group(dir: &Path, id: &str) -> Result<()> {
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    if catalog
        .active_groups
        .values()
        .flatten()
        .any(|g| g.group_id == id)
    {
        bail!("请先停用分组");
    }
    if !catalog.groups.iter().any(|g| g.id == id) {
        bail!("分组不存在");
    }
    catalog.groups.retain(|g| g.id != id);
    commit(dir, BTreeMap::new(), &catalog)
}
pub fn reorder(dir: &Path, ids: &[String]) -> Result<()> {
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    let own: HashSet<_> = catalog.groups.iter().map(|g| &g.id).collect();
    if ids.len() != own.len() || ids.iter().collect::<HashSet<_>>() != own {
        bail!("分组列表已变化，请刷新后重试");
    }
    for group in catalog.groups.iter_mut() {
        group.sort_order = ids.iter().position(|id| id == &group.id).unwrap();
    }
    commit(dir, BTreeMap::new(), &catalog)
}
/// 设置这个 Agent 当前叠加的全部分组（空列表表示全部停用），返回原样留下的接入。
///
/// 部署内容是这些分组成员的并集：多个分组共用的成员只写一份，只在所有用到它的分组
/// 都停用后才撤回。部署后被其他程序改写的接入不会拦住切换：要撤下的不再管理，要保留的
/// 不覆盖，都原样留着。启用新成员时同名位置上已有别人的配置，仍然拒绝，不覆盖。
pub fn activate(
    dir: &Path,
    agent_id: &str,
    ids: &[String],
    path: &Path,
    executable: &Path,
) -> Result<Vec<Untouched>> {
    crate::agents::validate(agent_id)?;
    let _lock = lock(dir)?;
    let catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    activate_locked(dir, catalog, agent_id, ids, path, executable)
}

/// 这个 Agent 当前叠加的分组，按启用顺序
pub fn active_ids(catalog: &Catalog, agent_id: &str) -> Vec<String> {
    catalog
        .active_groups
        .get(agent_id)
        .map(|records| records.iter().map(|g| g.group_id.clone()).collect())
        .unwrap_or_default()
}

/// 为一个 Agent 启用或停用一个分组，它叠加的其他分组不变；已经是目标状态时什么也不写。
/// 在锁内按最新的启用记录计算，不会撤掉别处（另一个窗口、命令行）刚做的启用。
/// `path` 只在启用后还有分组要部署时才用到，由 `path` 回调按需解析；回调在管理锁内调用，
/// 只能算路径，不要再读配置文件（加锁顺序见 `AppState::set_mcp_management`）
pub fn set_active(
    dir: &Path,
    agent_id: &str,
    group_id: &str,
    enabled: bool,
    path: impl FnOnce() -> Result<PathBuf>,
    executable: &Path,
) -> Result<Vec<Untouched>> {
    crate::agents::validate(agent_id)?;
    let _lock = lock(dir)?;
    let catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    let mut ids = active_ids(&catalog, agent_id);
    if ids.iter().any(|id| id == group_id) == enabled {
        return Ok(vec![]);
    }
    if enabled {
        ids.push(group_id.to_string());
    } else {
        ids.retain(|id| id != group_id);
    }
    let path = if ids.is_empty() {
        PathBuf::new()
    } else {
        path()?
    };
    activate_locked(dir, catalog, agent_id, &ids, &path, executable)
}

/// 按最新成员重新写入这个 Agent 叠加的全部分组（应用修改）；没有启用的分组时什么也不写
pub fn reapply(
    dir: &Path,
    agent_id: &str,
    path: impl FnOnce() -> Result<PathBuf>,
    executable: &Path,
) -> Result<Vec<Untouched>> {
    crate::agents::validate(agent_id)?;
    let _lock = lock(dir)?;
    let catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    let ids = active_ids(&catalog, agent_id);
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let path = path()?;
    activate_locked(dir, catalog, agent_id, &ids, &path, executable)
}

fn activate_locked(
    dir: &Path,
    mut catalog: Catalog,
    agent_id: &str,
    ids: &[String],
    path: &Path,
    executable: &Path,
) -> Result<Vec<Untouched>> {
    let mut groups = vec![];
    let mut seen_groups = HashSet::new();
    for id in ids {
        if !seen_groups.insert(id.clone()) {
            continue;
        }
        let group = catalog
            .groups
            .iter()
            .find(|g| g.id == *id)
            .context("分组不存在")?
            .clone();
        if group.entry_ids.is_empty() {
            bail!("「{}」还没有成员", group.name);
        }
        groups.push(group);
    }
    let mut members = Members::new("分组");
    for group in &groups {
        for entry_id in &group.entry_ids {
            members.add(&catalog, entry_id, &group.references)?;
        }
    }
    let entries = members.entries;
    let previous = catalog.active_groups.remove(agent_id).unwrap_or_default();
    let owned_before: Vec<Binding> = previous.iter().flat_map(|g| g.bindings.clone()).collect();
    let mut files = BTreeMap::new();
    let scope = Scope {
        agent: agent_id,
        path,
        project: None,
        checks: &[],
    };
    let (bindings, untouched) = deploy(
        dir,
        &catalog,
        &mut files,
        &scope,
        &entries,
        &owned_before,
        executable,
        "分组",
    )?;
    if !groups.is_empty() {
        // 每个分组记录自己引入的入口；共用的成员归前一个分组，
        // 停用它时只要还有别的分组需要就会被保留（见 deploy 里的 wanted 判定）
        let mut claimed_entries = HashSet::new();
        let mut claimed_bindings = HashSet::new();
        let records = groups
            .iter()
            .map(|group| ActiveGroup {
                group_id: group.id.clone(),
                entries: snapshot(
                    &entries
                        .iter()
                        .filter(|e| {
                            group.entry_ids.contains(&e.id) && claimed_entries.insert(e.id.clone())
                        })
                        .cloned()
                        .collect::<Vec<_>>(),
                ),
                bindings: bindings
                    .iter()
                    .filter(|b| {
                        group.entry_ids.contains(&b.id) && claimed_bindings.insert(b.id.clone())
                    })
                    .cloned()
                    .collect(),
            })
            .collect();
        catalog.active_groups.insert(agent_id.into(), records);
    }
    commit(dir, files, &catalog)?;
    Ok(untouched)
}

/// 每个 Agent 下、每个在用分组的部署问题：分组写进 Agent 配置的接入已被外部修改或移除。
/// 共用的成员只记在第一个分组名下（见 `activate`），这里同样标到其他也包含它的在用分组上，
/// 让界面只在受影响的分组卡片上提示「需要检查」。
pub fn issues(catalog: &Catalog) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut texts = Default::default();
    let mut out = BTreeMap::new();
    for (agent, records) in &catalog.active_groups {
        let mut by_group = BTreeMap::new();
        for record in records {
            for b in &record.bindings {
                if intact(&mut texts, b) {
                    continue;
                }
                let issue = issue(b);
                for other in records {
                    let member = other.group_id == record.group_id
                        || catalog
                            .groups
                            .iter()
                            .any(|g| g.id == other.group_id && g.entry_ids.contains(&b.id));
                    if member {
                        by_group
                            .entry(other.group_id.clone())
                            .or_insert_with(|| issue.clone());
                    }
                }
            }
        }
        if !by_group.is_empty() {
            out.insert(agent.clone(), by_group);
        }
    }
    out
}
