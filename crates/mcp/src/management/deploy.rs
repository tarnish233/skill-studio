//! 分组与项目共用的部署核心：把一组成员写进某个 Agent 的一份 MCP 配置，撤回不再需要的接入。
//! 分组写到 Agent 的全局配置，项目写到项目里的配置（Claude 可选项目本地），规则完全一致。
use super::*;
use std::collections::HashMap;

/// 启停时 Studio 没有动的接入：部署后已被其他程序改写（包括被手动停用）。
/// 不覆盖、不删除——那已经不是 Studio 写的内容——只在结果里报告。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Untouched {
    pub agent: String,
    pub path: PathBuf,
    pub project: Option<String>,
    pub key: String,
    /// 仍在用它的分组 / 项目继续记着它（卡片上会显示需要检查）；false 表示已不再管理它，
    /// 之后作为原有配置出现在扫描结果里
    pub tracked: bool,
}

/// 写到哪里：某个 Agent 的一份 MCP 配置。Claude 的项目本地配置写在 ~/.claude.json 里，
/// `project` 是项目根目录；其余情况为 None。
pub(super) struct Scope<'a> {
    pub agent: &'a str,
    pub path: &'a Path,
    pub project: Option<&'a str>,
    /// 还要通过启用检查的其他配置（Grok 的全局停用列表在全局配置里）
    pub checks: &'a [PathBuf],
}

/// 按出现顺序收集成员并去重。`subject` 是提示里的叫法：「分组」或「项目」。
pub(super) struct Members<'a> {
    subject: &'a str,
    pub entries: Vec<Entry>,
    ids: HashSet<String>,
    names: HashSet<String>,
}
impl<'a> Members<'a> {
    pub(super) fn new(subject: &'a str) -> Self {
        Self {
            subject,
            entries: vec![],
            ids: HashSet::new(),
            names: HashSet::new(),
        }
    }
    /// 成员可以是 Hub 里的 MCP，或保存时记下的原有配置引用（`references`）
    pub(super) fn add(&mut self, catalog: &Catalog, id: &str, references: &[Entry]) -> Result<()> {
        if !self.ids.insert(id.to_string()) {
            return Ok(());
        }
        let subject = self.subject;
        let entry = catalog
            .entries
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .or_else(|| references.iter().find(|e| e.id == id).cloned())
            .with_context(|| format!("{subject}包含已缺失的 MCP，请编辑{subject}"))?;
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
                    .is_some_and(|value| native::canonical(&value, &b.agent) == entry.definition)
            });
            if !valid {
                bail!(
                    "MCP「{}」的来源已变化，请重新编辑{subject}选择成员",
                    entry.name
                );
            }
        }
        if !self.names.insert(entry.name.trim().to_string()) {
            bail!("启用的{subject}里 MCP 名称重复，请先在 Hub 修改名称");
        }
        self.entries.push(entry);
        Ok(())
    }
}

/// 只记下成员需要的引用：原有配置保存时从扫描结果里取，从不托管或改动它们（同分组）。
/// `previous` 是之前保存的引用，`imports` 是这次由后端重新扫描提交的原有配置。
pub(super) fn references(
    catalog: &Catalog,
    member_ids: &[String],
    previous: Vec<Entry>,
    imports: Vec<Entry>,
    subject: &str,
) -> Result<Vec<Entry>> {
    let mut references = previous;
    references.retain(|e| member_ids.contains(&e.id));
    for mut entry in imports {
        if !member_ids.contains(&entry.id) || catalog.entries.iter().any(|e| e.id == entry.id) {
            continue;
        }
        native::validate(&entry.definition)?;
        entry.bindings.retain(|b| {
            !catalog.owned_bindings().any(|owned| {
                owned.agent == b.agent
                    && owned.path == b.path
                    && owned.key == b.key
                    && owned.project == b.project
            })
        });
        if entry.bindings.is_empty() {
            bail!("{subject}部署不能作为原始来源，请先在 Hub 添加独立配置");
        }
        references.retain(|e| e.id != entry.id);
        references.push(entry);
    }
    Ok(references)
}

/// 把 `entries` 写进 `scope`，撤回 `owned_before` 里不再需要的接入，返回新的接入和原样留下的。
///
/// 部署后被其他程序改写的接入不会拦住启停：要撤下的不再管理，要保留的不覆盖，都原样留着。
/// 启用新成员时同名位置上已有别人的配置，仍然拒绝，不覆盖。
#[allow(clippy::too_many_arguments)]
pub(super) fn deploy(
    dir: &Path,
    catalog: &Catalog,
    files: &mut BTreeMap<PathBuf, Change>,
    scope: &Scope,
    entries: &[Entry],
    owned_before: &[Binding],
    executable: &Path,
    subject: &str,
) -> Result<(Vec<Binding>, Vec<Untouched>)> {
    let (agent_id, path, project) = (scope.agent, scope.path, scope.project);
    let here = |b: &Binding| b.path == path && b.project.as_deref() == project;
    let name_of = |entry: &Entry| {
        catalog
            .entries
            .iter()
            .find(|e| e.id == entry.id)
            .or(Some(entry))
            .and_then(|e| e.bindings.iter().find(|b| b.agent == agent_id && here(b)))
            .map(|b| b.key.clone())
            .unwrap_or_else(|| entry.name.trim().to_string())
    };
    // 仍然要用的接入留着，其余还原成原始配置
    let wanted: HashSet<String> = entries.iter().map(name_of).collect();
    let mut released = Removal::default();
    for binding in owned_before {
        if here(binding) && wanted.contains(&binding.key) {
            continue;
        }
        // 与删除托管同一规则：还是 Studio 写的就还原，被其他程序改写的不碰、不再管理
        release(files, binding, &binding.original, &mut released)?;
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
    let text_of = |files: &BTreeMap<PathBuf, Change>, path: &Path| -> Result<String> {
        Ok(match files.get(path) {
            Some(c) => c.after.clone().unwrap_or_default(),
            None => read_text(path)?.unwrap_or_default(),
        })
    };
    let mut bindings = vec![];
    let mut keys = HashSet::new();
    for entry in entries {
        // Reuse the managed native key even when the Hub display name was changed.
        let key = name_of(entry);
        if !keys.insert(key.clone()) {
            bail!("{subject}内 MCP 使用了相同的配置名称，请先在 Hub 核对");
        }
        let text = text_of(files, path)?;
        native::check_activation(&text, agent_id, &key)?;
        for extra in scope.checks {
            native::check_activation(&text_of(files, extra)?, agent_id, &key)?;
        }
        let original = native::entry(&text, agent_id, project, &key)?;
        // 之前由这里写入、之后被其他程序改写（或停用）的接入：不覆盖，仍记在名下
        let owned = owned_before.iter().find(|b| here(b) && b.key == key);
        if let (Some(value), Some(owned)) = (&original, owned) {
            if *value != owned.installed {
                untouched.push(Untouched {
                    agent: agent_id.into(),
                    path: path.into(),
                    project: project.map(String::from),
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
                    e.bindings
                        .iter()
                        .any(|b| here(b) && b.key == key && b.installed == *value)
                })
                || owned_before
                    .iter()
                    .any(|b| here(b) && b.key == key && b.installed == *value);
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
                &json!({"type":"stdio","command":gateway_command(executable)?,"args":["--mcp-client",entry.id,dir]}),
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
            project: project.map(String::from),
            key: key.clone(),
            expected: original.clone(),
        };
        // 重新启用时，原生文件里已经是 Studio 自己写进去的值：
        // 这时要沿用上一次记录里的 original，否则停用时就"还原"成自己的输出、撤不掉。
        let record_original = owned_before
            .iter()
            .find(|b| here(b) && b.key == key && original.as_ref() == Some(&b.installed))
            .map(|b| b.original.clone())
            .unwrap_or_else(|| original.clone());
        stage(files, &target, &original, &Some(installed.clone()))?;
        bindings.push(Binding {
            id: entry.id.clone(),
            agent: agent_id.into(),
            path: path.into(),
            project: project.map(String::from),
            key,
            original: record_original,
            installed,
        });
    }
    Ok((bindings, untouched))
}

/// 部署记录里的成员快照不带接入：接入单独记在部署记录里
pub(super) fn snapshot(entries: &[Entry]) -> Vec<Entry> {
    entries
        .iter()
        .map(|e| {
            let mut e = e.clone();
            e.bindings.clear();
            e
        })
        .collect()
}

/// 部署写进去的接入是否还是 Studio 写的样子
pub(super) fn intact(texts: &mut HashMap<PathBuf, Option<String>>, b: &Binding) -> bool {
    let text = texts
        .entry(b.path.clone())
        .or_insert_with(|| read_text(&b.path).ok().flatten());
    text.as_deref().is_some_and(|text| {
        native::entry(text, &b.agent, b.project.as_deref(), &b.key)
            .is_ok_and(|value| value.as_ref() == Some(&b.installed))
    })
}

pub(super) fn issue(b: &Binding) -> String {
    format!(
        "MCP「{}」的配置已在外部修改或移除，请先核对 {}",
        b.key,
        b.path.display()
    )
}
