//! Shared MCP combinations, activated independently per agent. Metadata and all
//! native edits share the management journal.
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
pub fn save_group(dir: &Path, mut group: Group, imports: Vec<Entry>) -> Result<()> {
    group.name = group.name.trim().into();
    if group.name.is_empty() || group.id.is_empty() {
        bail!("请填写分组名称");
    }
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    if catalog
        .groups
        .iter()
        .any(|g| g.name == group.name && g.id != group.id)
    {
        bail!("已存在同名分组");
    }
    // Ignore references submitted in the group payload; only server-rescanned imports
    // or previously saved references can supply filesystem locations.
    group.references = catalog
        .groups
        .iter()
        .find(|g| g.id == group.id)
        .map(|g| g.references.clone())
        .unwrap_or_default();
    group.references.retain(|e| group.entry_ids.contains(&e.id));
    for mut entry in imports {
        if !group.entry_ids.contains(&entry.id) || catalog.entries.iter().any(|e| e.id == entry.id)
        {
            continue;
        }
        native::validate(&entry.definition)?;
        entry.bindings.retain(|b| {
            !catalog.active_groups.values().flatten().any(|g| {
                g.bindings.iter().any(|owned| {
                    owned.agent == b.agent
                        && owned.path == b.path
                        && owned.key == b.key
                        && owned.project == b.project
                })
            })
        });
        if entry.bindings.is_empty() {
            bail!("分组部署不能作为原始来源，请先在 Hub 添加独立配置");
        }
        group.references.retain(|e| e.id != entry.id);
        group.references.push(entry);
    }
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
/// 设置这个 agent 当前叠加的全部分组（空列表表示全部停用）。
///
/// 部署内容是这些分组成员的并集：多个分组共用的成员只写一份，只在所有用到它的分组
/// 都停用后才撤回。
/// 切换分组时 Studio 没有动的接入：部署后已被其他程序改写（包括被手动停用）。
/// 不覆盖、不删除——那已经不是 Studio 写的内容——只在结果里报告。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Untouched {
    pub agent: String,
    pub path: PathBuf,
    pub project: Option<String>,
    pub key: String,
    /// 仍在用它的分组继续记着它（卡片上会显示需要检查）；false 表示分组已不再管理它，
    /// 之后作为原有配置出现在扫描结果里
    pub tracked: bool,
}
/// 设置这个 Agent 当前叠加的全部分组（空列表表示全部停用），返回原样留下的接入。
///
/// 部署后被其他程序改写的接入不会拦住切换：要撤下的不再管理，要保留的不覆盖，都原样留着。
/// 启用新成员时同名位置上已有别人的配置，仍然拒绝，不覆盖。
pub fn activate(
    dir: &Path,
    agent_id: &str,
    ids: &[String],
    path: &Path,
    executable: &Path,
) -> Result<Vec<Untouched>> {
    crate::agents::validate(agent_id)?;
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
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
    let mut entries = vec![];
    let mut names = HashSet::new();
    let mut member_ids = HashSet::new();
    for group in &groups {
        for entry_id in &group.entry_ids {
            if !member_ids.insert(entry_id.clone()) {
                continue;
            }
            let entry = catalog
                .entries
                .iter()
                .find(|e| &e.id == entry_id)
                .cloned()
                .or_else(|| group.references.iter().find(|e| &e.id == entry_id).cloned())
                .context("分组包含已缺失的 MCP，请编辑分组")?;
            if !catalog.entries.iter().any(|e| e.id == entry.id) {
                // A reference is valid only while its source still exists and matches.
                // Never silently deploy a stale snapshot after the source was edited.
                let valid = entry.bindings.iter().any(|b| {
                    read_text(&b.path)
                        .ok()
                        .flatten()
                        .and_then(|text| {
                            native::entry(&text, &b.agent, b.project.as_deref(), &b.key)
                                .ok()
                                .flatten()
                        })
                        .is_some_and(|value| {
                            native::canonical(&value, &b.agent) == entry.definition
                        })
                });
                if !valid {
                    bail!("MCP「{}」的来源已变化，请重新编辑分组选择成员", entry.name);
                }
            }
            if !names.insert(entry.name.trim().to_string()) {
                bail!("启用的分组里 MCP 名称重复，请先在 Hub 修改名称");
            }
            entries.push(entry);
        }
    }
    let name_of = |entry: &Entry| {
        catalog
            .entries
            .iter()
            .find(|e| e.id == entry.id)
            .or(Some(entry))
            .and_then(|e| {
                e.bindings
                    .iter()
                    .find(|b| b.agent == agent_id && b.path == path && b.project.is_none())
            })
            .map(|b| b.key.clone())
            .unwrap_or_else(|| entry.name.trim().to_string())
    };
    // 仍然要用的接入留着，其余还原成原始配置
    let wanted: HashSet<String> = entries.iter().map(name_of).collect();
    let mut files = BTreeMap::new();
    let previous = catalog.active_groups.remove(agent_id).unwrap_or_default();
    let owned_before: Vec<Binding> = previous.iter().flat_map(|g| g.bindings.clone()).collect();
    let mut released = Removal::default();
    for binding in &owned_before {
        if binding.path == path && wanted.contains(&binding.key) {
            continue;
        }
        // 与删除托管同一规则：还是 Studio 写的就还原，被其他程序改写的不碰、不再管理
        release(&mut files, binding, &binding.original, &mut released)?;
    }
    let mut untouched: Vec<Untouched> = released
        .foreign
        .into_iter()
        .map(|f| Untouched {
            agent: f.agent,
            path: f.path,
            project: f.project,
            key: f.key,
            tracked: false,
        })
        .collect();
    let mut bindings = vec![];
    let mut keys = HashSet::new();
    for entry in &entries {
        // Reuse the managed native key even when the Hub display name was changed.
        let key = name_of(entry);
        if !keys.insert(key.clone()) {
            bail!("组内 MCP 使用了相同的配置名称，请先在 Hub 核对");
        }
        let text = match files.get(path) {
            Some(c) => c.after.clone().unwrap_or_default(),
            None => read_text(path)?.unwrap_or_default(),
        };
        native::check_activation(&text, agent_id, &key)?;
        let original = native::entry(&text, agent_id, None, &key)?;
        // 之前由分组写入、之后被其他程序改写（或停用）的接入：不覆盖，仍记在分组名下
        let owned = owned_before
            .iter()
            .find(|b| b.path == path && b.project.is_none() && b.key == key);
        if let (Some(value), Some(owned)) = (&original, owned) {
            if *value != owned.installed {
                untouched.push(Untouched {
                    agent: agent_id.into(),
                    path: path.into(),
                    project: None,
                    key: key.clone(),
                    tracked: true,
                });
                bindings.push(owned.clone());
                continue;
            }
        }
        if let Some(value) = &original {
            if !native::enabled(value, agent_id) {
                bail!("MCP「{key}」已被手动停用，请先恢复启用");
            }
            // Match Skill groups: content Studio已记录在案的接入继续归它管，
            // 否则内容一致的视为用户自己的配置，保留原样。
            let tracked = catalog
                .entries
                .iter()
                .find(|e| e.id == entry.id)
                .is_some_and(|e| {
                    e.bindings.iter().any(|b| {
                        b.path == path
                            && b.project.is_none()
                            && b.key == key
                            && b.installed == *value
                    })
                })
                || owned_before.iter().any(|b| {
                    b.path == path && b.project.is_none() && b.key == key && b.installed == *value
                });
            if !tracked
                && entry.mode == "direct"
                && native::canonical(value, agent_id) == entry.definition
            {
                continue;
            }
            if !tracked && native::canonical(value, agent_id) != entry.definition {
                bail!("MCP「{key}」存在不同的同名配置，请先在 Hub 核对");
            }
        }
        let mut installed = if entry.mode == "gateway" {
            gateway_server(entry)?;
            native::for_agent(
                &json!({"type":"stdio","command":executable,"args":["--mcp-client",entry.id,dir]}),
                agent_id,
            )?
        } else {
            native::for_agent(&entry.definition, agent_id)?
        };
        // Group activation enables its new bindings, not the Hub template. The
        // checks above still reject an existing manually disabled target.
        match agent_id {
            "codex" | "grok" | "pi" => installed["enabled"] = json!(true),
            "opencode" => installed["disabled"] = json!(false),
            _ => {}
        }
        let target = Target {
            agent: agent_id.into(),
            path: path.into(),
            project: None,
            key: key.clone(),
            expected: original.clone(),
        };
        // 重新启用同一个分组时，原生文件里已经是 Studio 自己写进去的值：
        // 这时要沿用上一次记录里的 original，否则停用时就"还原"成自己的输出、撤不掉。
        let record_original = owned_before
            .iter()
            .find(|b| {
                b.path == path
                    && b.project.is_none()
                    && b.key == key
                    && original.as_ref() == Some(&b.installed)
            })
            .map(|b| b.original.clone())
            .unwrap_or_else(|| original.clone());
        stage(&mut files, &target, &original, &Some(installed.clone()))?;
        bindings.push(Binding {
            id: entry.id.clone(),
            agent: agent_id.into(),
            path: path.into(),
            project: None,
            key,
            original: record_original,
            installed,
        });
    }
    if groups.is_empty() {
        catalog.active_groups.remove(agent_id);
    } else {
        // 每个分组记录自己引入的入口；共用的成员归前一个分组，
        // 停用它时只要还有别的分组需要就会被保留（见上面的 wanted 判定）
        let mut claimed_entries = HashSet::new();
        let mut claimed_bindings = HashSet::new();
        let records = groups
            .iter()
            .map(|group| ActiveGroup {
                group_id: group.id.clone(),
                entries: entries
                    .iter()
                    .filter(|e| {
                        group.entry_ids.contains(&e.id) && claimed_entries.insert(e.id.clone())
                    })
                    .map(|e| {
                        let mut e = e.clone();
                        e.bindings.clear();
                        e
                    })
                    .collect(),
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
    let mut texts: std::collections::HashMap<PathBuf, Option<String>> = Default::default();
    let mut out = BTreeMap::new();
    for (agent, records) in &catalog.active_groups {
        let mut by_group = BTreeMap::new();
        for record in records {
            for b in &record.bindings {
                let text = texts
                    .entry(b.path.clone())
                    .or_insert_with(|| read_text(&b.path).ok().flatten());
                let intact = text.as_deref().is_some_and(|text| {
                    native::entry(text, &b.agent, b.project.as_deref(), &b.key)
                        .is_ok_and(|value| value.as_ref() == Some(&b.installed))
                });
                if intact {
                    continue;
                }
                let issue = format!(
                    "MCP「{}」的配置已在外部修改或移除，请先核对 {}",
                    b.key,
                    b.path.display()
                );
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
