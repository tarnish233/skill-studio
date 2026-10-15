//! Agent-local combinations. Metadata and all native edits share the management journal.
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
fn agent(agent: &str) -> Result<()> {
    crate::agents::validate(agent)
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
            !catalog.active_groups.values().any(|g| {
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
        group.sort_order = catalog.groups.len();
        catalog.groups.push(group);
    }
    commit(dir, BTreeMap::new(), &catalog)
}
pub fn remove_group(dir: &Path, id: &str) -> Result<()> {
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    if catalog.active_groups.values().any(|g| g.group_id == id) {
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
pub fn activate(
    dir: &Path,
    agent_id: &str,
    id: Option<&str>,
    path: &Path,
    executable: &Path,
) -> Result<()> {
    agent(agent_id)?;
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    ensure_active(&catalog)?;
    let mut entries = vec![];
    if let Some(id) = id {
        let group = catalog
            .groups
            .iter()
            .find(|g| g.id == id)
            .context("分组不存在")?;
        if group.entry_ids.is_empty() {
            bail!("请先为分组选择 MCP");
        }
        let mut names = HashSet::new();
        for entry_id in &group.entry_ids {
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
                bail!("组内 MCP 名称重复，请先在 Hub 修改名称");
            }
            entries.push(entry);
        }
    }
    let mut files = BTreeMap::new();
    if let Some(previous) = catalog.active_groups.remove(agent_id) {
        for b in previous.bindings {
            let target = Target {
                agent: agent_id.into(),
                path: b.path,
                project: b.project,
                key: b.key,
                expected: None,
            };
            stage(&mut files, &target, &Some(b.installed), &b.original)?;
        }
    }
    let mut bindings = vec![];
    let mut keys = HashSet::new();
    for entry in &entries {
        // Reuse the managed native key even when the Hub display name was changed.
        let key = catalog
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
            .unwrap_or_else(|| entry.name.trim().to_string());
        if !keys.insert(key.clone()) {
            bail!("组内 MCP 使用了相同的配置名称，请先在 Hub 核对");
        }
        let text = match files.get(path) {
            Some(c) => c.after.clone().unwrap_or_default(),
            None => read_text(path)?.unwrap_or_default(),
        };
        native::check_activation(&text, agent_id, &key)?;
        let original = native::entry(&text, agent_id, None, &key)?;
        if let Some(value) = &original {
            if !native::enabled(value, agent_id) {
                bail!("MCP「{key}」已被手动停用，请先恢复启用");
            }
            // Match Skill groups: existing usable content remains externally owned.
            if entry.mode == "direct" && native::canonical(value, agent_id) == entry.definition {
                continue;
            }
            let owned = catalog
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
                });
            if !owned && native::canonical(value, agent_id) != entry.definition {
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
        stage(&mut files, &target, &original, &Some(installed.clone()))?;
        bindings.push(Binding {
            id: entry.id.clone(),
            agent: agent_id.into(),
            path: path.into(),
            project: None,
            key,
            original,
            installed,
        });
    }
    if let Some(id) = id {
        catalog.active_groups.insert(
            agent_id.into(),
            ActiveGroup {
                group_id: id.into(),
                entries: entries
                    .into_iter()
                    .map(|mut e| {
                        e.bindings.clear();
                        e
                    })
                    .collect(),
                bindings,
            },
        );
    }
    commit(dir, files, &catalog)
}

pub fn issues(catalog: &Catalog) -> BTreeMap<String, String> {
    catalog
        .active_groups
        .iter()
        .filter_map(|(agent, group)| {
            let issue = group.bindings.iter().find_map(|b| {
                let check = read_text(&b.path).and_then(|text| {
                    native::entry(
                        &text.unwrap_or_default(),
                        &b.agent,
                        b.project.as_deref(),
                        &b.key,
                    )
                });
                match check {
                    Ok(value) if value == Some(b.installed.clone()) => None,
                    _ => Some(format!(
                        "MCP「{}」的配置已在外部修改或移除，请先核对 {}",
                        b.key,
                        b.path.display()
                    )),
                }
            });
            issue.map(|issue| (agent.clone(), issue))
        })
        .collect()
}
