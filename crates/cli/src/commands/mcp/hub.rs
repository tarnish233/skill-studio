//! MCP Hub：列出、查看、添加、删除。

use std::fmt::Write as _;

use serde_json::{json, Map, Value};
use skill_studio_mcp::management::{Entry, Removal};
use skill_studio_service::mcp::{self as service, entries::Placement};

use super::{
    executable, kind, load, mcp_agent, notify_gateway, place, redact, resolve, source_json, target,
    written, Mcp, Row,
};
use crate::commands::Ctx;
use crate::error::CliError;
use crate::report::{agent_name, agent_names, arg, home, list as join, Report};
use crate::resolve as names;

/// 分组、项目里用到这个 MCP 的地方，以及它们在哪些 Agent 上启用
struct Usage {
    groups: Vec<(String, Vec<String>)>,
    projects: Vec<(String, Vec<String>)>,
}

fn usage(mcp: &Mcp, id: &str) -> Usage {
    let groups = mcp
        .groups()
        .into_iter()
        .filter(|g| g.entry_ids.iter().any(|e| e == id))
        .map(|g| (g.name.clone(), mcp.group_agents(&g.id)))
        .collect();
    let projects = mcp
        .config
        .projects
        .iter()
        .filter_map(|p| {
            let record = mcp.catalog.projects.get(&p.id)?;
            mcp.project_members(record)
                .iter()
                .any(|e| e == id)
                .then(|| (p.name.clone(), mcp.project_agents(record)))
        })
        .collect();
    Usage { groups, projects }
}

/// 「开发（已启用：Codex）」
fn used(items: &[(String, Vec<String>)]) -> Vec<String> {
    items
        .iter()
        .map(|(name, agents)| {
            if agents.is_empty() {
                name.clone()
            } else {
                format!("{name}（已启用：{}）", agent_names(agents))
            }
        })
        .collect()
}

fn used_json(items: &[(String, Vec<String>)]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|(name, agents)| json!({"name": name, "enabled": agents}))
            .collect(),
    )
}

/// 这一行配在了哪些 Agent 里（已启用的）
fn enabled_agents(row: &Row) -> Vec<String> {
    super::app_agents(
        row.sources
            .iter()
            .filter(|s| s.enabled)
            .map(|s| &s.agent)
            .collect::<Vec<_>>(),
    )
}

pub fn list(ctx: &Ctx, agent: Option<&str>, query: Option<&str>) -> Result<Report, CliError> {
    let agent = agent.map(names::agent).transpose()?.map(|a| a.id);
    let state = ctx.reader()?;
    let mcp = load(&state)?;
    let query = query.map(|q| q.trim().to_lowercase());
    let rows: Vec<&Row> = mcp
        .rows
        .iter()
        .filter(|row| {
            agent.is_none_or(|agent| {
                row.sources
                    .iter()
                    .any(|s| s.enabled && s.agent == mcp_agent(agent))
            })
        })
        .filter(|row| {
            query.as_deref().is_none_or(|q| {
                row.entry.name.to_lowercase().contains(q)
                    || target(&row.entry.definition).to_lowercase().contains(q)
            })
        })
        .collect();
    let hub = rows.iter().filter(|r| r.managed).count();
    let mut text = format!(
        "共 {} 个 MCP（Hub 里 {hub} 个，原有配置 {} 个）",
        rows.len(),
        rows.len() - hub
    );
    if mcp.rows.is_empty() {
        text.push_str("。用 skill-studio mcp add 添加到 Hub");
    }
    let mut items = Vec::new();
    for row in rows {
        let usage = usage(&mcp, &row.entry.id);
        let agents = enabled_agents(row);
        let mut parts = vec![
            row.entry.name.clone(),
            kind(row).to_string(),
            target(&row.entry.definition),
        ];
        if !usage.groups.is_empty() {
            let groups: Vec<&String> = usage.groups.iter().map(|(n, _)| n).collect();
            parts.push(format!("分组：{}", join(&groups)));
        }
        parts.push(if agents.is_empty() {
            "还没有写进任何 Agent".into()
        } else {
            format!("在 {} 里", agent_names(&agents))
        });
        let _ = write!(text, "\n  {}", parts.join(" · "));
        items.push(json!({
            "id": row.entry.id, "name": row.entry.name, "hub": row.managed,
            "mode": row.entry.mode, "target": target(&row.entry.definition),
            "groups": usage.groups.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            "projects": usage.projects.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            "agents": agents,
            "sources": row.sources.iter().map(source_json).collect::<Vec<_>>(),
        }));
    }
    for warning in &mcp.warnings {
        let _ = write!(text, "\n注意：{warning}");
    }
    Ok(Report::new(text, Value::Array(items)))
}

pub fn show(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let state = ctx.reader()?;
    let mcp = load(&state)?;
    let row = resolve(&mcp, input)?;
    let usage = usage(&mcp, &row.entry.id);
    let definition = &row.entry.definition;
    let mut text = format!("{}（{}）", row.entry.name, kind(row));
    if definition.get("url").is_some() {
        let _ = write!(text, "\n网址：{}", target(definition));
    } else {
        let _ = write!(text, "\n启动：{}", target(definition));
    }
    let keys = |field: &str| -> Vec<String> {
        definition
            .get(field)
            .and_then(Value::as_object)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    };
    let env: Vec<String> = keys("env").into_iter().chain(keys("environment")).collect();
    if !env.is_empty() {
        let _ = write!(text, "\n环境变量：{}（值已隐藏）", join(&env));
    }
    let headers: Vec<String> = keys("headers")
        .into_iter()
        .chain(keys("http_headers"))
        .collect();
    if !headers.is_empty() {
        let _ = write!(text, "\n请求头：{}（值已隐藏）", join(&headers));
    }
    let groups = used(&usage.groups);
    let _ = write!(
        text,
        "\n所在分组：{}",
        if groups.is_empty() {
            "无".into()
        } else {
            join(&groups)
        }
    );
    let projects = used(&usage.projects);
    if !projects.is_empty() {
        let _ = write!(text, "\n所在项目：{}", join(&projects));
    }
    if row.sources.is_empty() {
        let _ = write!(text, "\n还没有写进任何 Agent");
    } else {
        let _ = write!(text, "\n配置在：");
        for source in &row.sources {
            let _ = write!(text, "\n  {}", place(source));
        }
    }
    if row.managed && usage.groups.is_empty() && usage.projects.is_empty() {
        let _ = write!(
            text,
            "\n下一步：加入分组 skill-studio mcp groups add <分组> {}",
            arg(&row.entry.name)
        );
    }
    let json = json!({
        "id": row.entry.id, "name": row.entry.name, "hub": row.managed, "mode": row.entry.mode,
        "definition": redact(definition),
        "groups": used_json(&usage.groups), "projects": used_json(&usage.projects),
        "sources": row.sources.iter().map(source_json).collect::<Vec<_>>(),
    });
    Ok(Report::new(text, json))
}

pub struct Input<'a> {
    pub name: Option<&'a str>,
    pub url: Option<&'a str>,
    pub headers: &'a [String],
    pub env: &'a [String],
    pub config: Option<&'a str>,
    pub gateway: bool,
    pub command: &'a [String],
}

/// `KEY=值`，请求头也可以写成 `名称: 值`
fn pairs(items: &[String], what: &str, colon: bool) -> Result<Map<String, Value>, CliError> {
    let mut map = Map::new();
    for item in items {
        let split = if colon {
            item.split_once(':').or_else(|| item.split_once('='))
        } else {
            item.split_once('=')
        };
        let Some((key, value)) = split.filter(|(k, _)| !k.trim().is_empty()) else {
            return Err(CliError::usage(format!("{what}要写成 名称=值：{item}")));
        };
        map.insert(key.trim().to_string(), json!(value.trim()));
    }
    Ok(map)
}

/// 要添加的 MCP：名称和定义
fn candidates(input: &Input) -> Result<Vec<(String, Value)>, CliError> {
    let env = pairs(input.env, "环境变量", false)?;
    let named = |definition: Value| -> Result<Vec<(String, Value)>, CliError> {
        let name = input
            .name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .ok_or_else(|| {
                CliError::usage("请写上 MCP 的名称")
                    .hint("例如 skill-studio mcp add github -- npx -y @modelcontextprotocol/server-github")
            })?;
        Ok(vec![(name.to_string(), definition)])
    };
    if let Some(url) = input.url {
        if !env.is_empty() {
            return Err(CliError::usage(
                "--env 只用于本地启动的 MCP，远程 MCP 用 --header",
            ));
        }
        let mut definition = json!({"type": "http", "url": url.trim()});
        let headers = pairs(input.headers, "请求头", true)?;
        if !headers.is_empty() {
            definition["headers"] = Value::Object(headers);
        }
        return named(definition);
    }
    if let Some((command, args)) = input.command.split_first() {
        let mut definition = json!({"type": "stdio", "command": command});
        if !args.is_empty() {
            definition["args"] = json!(args);
        }
        if !env.is_empty() {
            definition["env"] = Value::Object(env);
        }
        return named(definition);
    }
    let Some(config) = input.config else {
        return Err(CliError::usage(
            "请写明怎么连接：-- 后面写启动命令，或用 --url 写网址，或用 --config 粘贴配置",
        )
        .hint("运行 skill-studio mcp add --help 查看例子"));
    };
    let parsed = service::parse(config).map_err(|e| CliError::usage(format!("配置读不懂：{e}")))?;
    let parsed = parsed.as_array().cloned().unwrap_or_default();
    if input.name.is_some() && parsed.len() > 1 {
        return Err(CliError::usage(format!(
            "配置里有 {} 个 MCP，不能共用一个名称：去掉名称，按配置里的名称逐个添加",
            parsed.len()
        )));
    }
    let mut out = Vec::new();
    for item in parsed {
        let mut definition = item["definition"].clone();
        if !env.is_empty() {
            if definition.get("url").is_some() {
                return Err(CliError::usage("--env 只用于本地启动的 MCP"));
            }
            let mut merged = definition
                .get("env")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            merged.extend(env.clone());
            definition["env"] = Value::Object(merged);
        }
        let name = input
            .name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .or_else(|| item["name"].as_str().filter(|n| !n.trim().is_empty()))
            .ok_or_else(|| CliError::usage("配置里没有 MCP 的名称，请在命令里写上名称"))?;
        out.push((name.trim().to_string(), definition));
    }
    Ok(out)
}

pub fn add(ctx: &Ctx, input: Input) -> Result<Report, CliError> {
    let candidates = candidates(&input)?;
    let mode = if input.gateway { "gateway" } else { "direct" };
    let state = ctx.writer()?;
    let mcp = load(&state)?;
    let mut added: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for (name, definition) in candidates {
        let entry = Entry {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.clone(),
            mode: mode.into(),
            definition,
            ..Default::default()
        };
        if let Some(existing) = mcp
            .catalog
            .entries
            .iter()
            .find(|e| e.name.to_lowercase() == name.to_lowercase())
        {
            if existing.definition == entry.definition && existing.mode == entry.mode {
                skipped.push(name);
                continue;
            }
            return Err(
                CliError::failed(format!("MCP Hub 里已有同名的「{}」", existing.name)).hint(
                    format!(
                        "换个名称，或先用 skill-studio mcp show {} 查看",
                        arg(&existing.name)
                    ),
                ),
            );
        }
        if input.gateway {
            if let Some(issue) = service::gateway_issue(&entry) {
                return Err(
                    CliError::failed(format!("「{name}」不能由网关转发：{issue}"))
                        .hint("去掉 --gateway，让 Agent 直接连接"),
                );
            }
        }
        service::entries::save(
            &state,
            executable().as_deref(),
            entry,
            None,
            &Placement::default(),
        )
        .map_err(|e| CliError::from(e).context(format!("添加「{name}」时失败")))?;
        added.push(name);
    }
    if !added.is_empty() {
        notify_gateway(&service::dir(&state));
    }
    let skipped_note = if skipped.is_empty() {
        String::new()
    } else {
        format!("（{} 已在 Hub 里，配置相同）", join(&skipped))
    };
    if added.is_empty() {
        return Ok(written(
            format!("MCP Hub 没有变化{skipped_note}"),
            false,
            false,
            &[],
            json!({ "added": added, "skipped": skipped }),
        ));
    }
    let first = arg(&added[0]);
    Ok(written(
        format!(
            "已把 {} 添加到 MCP Hub{skipped_note}，还没有写进任何 Agent",
            join(&added)
        ),
        true,
        false,
        &[],
        json!({ "added": added, "skipped": skipped }),
    )
    .next(format!(
        "加入分组 skill-studio mcp groups add <分组> {first}（或 mcp groups create <名称> --mcp {first}），再为 Agent 启用"
    )))
}

pub struct Change<'a> {
    pub name: Option<&'a str>,
    pub env: &'a [String],
    pub unset_env: &'a [String],
    pub headers: &'a [String],
    pub unset_headers: &'a [String],
    pub url: Option<&'a str>,
    pub command: &'a [String],
}

/// 改一张表（环境变量、请求头）：设置的覆盖同名，删掉的去掉；表空了就整项去掉
fn edit_map(
    definition: &mut Value,
    keys: &[&str],
    set: Map<String, Value>,
    unset: &[String],
) -> Result<(), CliError> {
    let key = keys
        .iter()
        .find(|k| definition.get(**k).is_some())
        .copied()
        .unwrap_or(keys[0]);
    let mut map = definition
        .get(key)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for name in unset {
        if map.remove(name.trim()).is_none() {
            return Err(CliError::not_found(format!("没有这一项：{name}"))
                .candidates(map.keys().cloned().collect()));
        }
    }
    map.extend(set);
    let object = definition.as_object_mut().expect("MCP 定义是对象");
    if map.is_empty() {
        object.remove(key);
    } else {
        object.insert(key.into(), Value::Object(map));
    }
    Ok(())
}

pub fn set(ctx: &Ctx, input: &str, change: Change) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    let mcp = load(&state)?;
    let row = resolve(&mcp, input)?;
    if !row.managed {
        return Err(CliError::failed(format!(
            "「{}」是 Agent 配置里原有的 MCP，不在 Hub 里",
            row.entry.name
        ))
        .hint("在 Skill Studio 的 MCP Hub 里把它纳入管理后再改，或直接改 Agent 的配置文件"));
    }
    let current = row.entry.clone();
    let mut entry = current.clone();
    let local = entry.definition.get("url").is_none();
    if let Some(name) = change.name.map(str::trim).filter(|n| !n.is_empty()) {
        if let Some(other) = mcp
            .catalog
            .entries
            .iter()
            .find(|e| e.id != entry.id && e.name.to_lowercase() == name.to_lowercase())
        {
            return Err(CliError::failed(format!(
                "MCP Hub 里已有同名的「{}」",
                other.name
            )));
        }
        entry.name = name.to_string();
    }
    let env = pairs(change.env, "环境变量", false)?;
    let headers = pairs(change.headers, "请求头", true)?;
    if !local && (!env.is_empty() || !change.unset_env.is_empty() || !change.command.is_empty()) {
        return Err(CliError::usage(
            "这是远程 MCP：用 --url 换网址，用 --header 改请求头",
        ));
    }
    if local && (!headers.is_empty() || !change.unset_headers.is_empty() || change.url.is_some()) {
        return Err(CliError::usage(
            "这是本地启动的 MCP：用 --env 改环境变量，-- 后面写新的启动命令",
        ));
    }
    if !env.is_empty() || !change.unset_env.is_empty() {
        edit_map(
            &mut entry.definition,
            &["env", "environment"],
            env,
            change.unset_env,
        )?;
    }
    if !headers.is_empty() || !change.unset_headers.is_empty() {
        edit_map(
            &mut entry.definition,
            &["headers", "http_headers"],
            headers,
            change.unset_headers,
        )?;
    }
    if let Some(url) = change.url {
        entry.definition["url"] = json!(url.trim());
    }
    if let Some((command, args)) = change.command.split_first() {
        entry.definition["command"] = json!(command);
        let object = entry.definition.as_object_mut().expect("MCP 定义是对象");
        if args.is_empty() {
            object.remove("args");
        } else {
            object.insert("args".into(), json!(args));
        }
    }
    if entry.name == current.name && entry.definition == current.definition {
        return Ok(written(
            format!("「{}」没有变化", current.name),
            false,
            false,
            &[],
            json!({ "mcp": current.name }),
        ));
    }
    if entry.mode == "gateway" {
        if let Some(issue) = service::gateway_issue(&entry) {
            return Err(CliError::failed(format!("改动后不能由网关转发：{issue}")));
        }
    }
    // 它自己的接入（纳入管理时保留的原位置）照常保留，按新定义重写
    let placement = Placement {
        binding_ids: current.bindings.iter().map(|b| b.id.clone()).collect(),
        ..Default::default()
    };
    service::entries::save(
        &state,
        executable().as_deref(),
        entry.clone(),
        Some(current.clone()),
        &placement,
    )?;
    notify_gateway(&service::dir(&state));
    // 用到它的分组和项目：部署里的还是旧定义，要应用修改才会写进 Agent
    let mcp = load(&state)?;
    let mut pending = Vec::new();
    for group in mcp.groups() {
        if !mcp.group_agents(&group.id).is_empty() && group.entry_ids.contains(&entry.id) {
            pending.push(format!(
                "skill-studio mcp groups apply {}",
                arg(&group.name)
            ));
        }
    }
    for project in &mcp.config.projects {
        if let Some(record) = mcp.catalog.projects.get(&project.id) {
            if !record.active.is_empty() && mcp.project_members(record).contains(&entry.id) {
                pending.push(format!(
                    "skill-studio mcp projects apply {}",
                    arg(&project.name)
                ));
            }
        }
    }
    let rewrote = !current.bindings.is_empty();
    let mut message = format!("已修改 MCP「{}」", entry.name);
    if rewrote {
        message.push_str(&format!("，并改写了它的 {} 处接入", current.bindings.len()));
    }
    let report = written(
        message,
        true,
        rewrote,
        &[],
        json!({ "mcp": entry.name, "definition": redact(&entry.definition), "apply": pending }),
    );
    Ok(if pending.is_empty() {
        report
    } else {
        report.next(format!(
            "用到它的分组或项目要应用修改才会写进 Agent：{}",
            join(&pending)
        ))
    })
}

/// 删除托管时原样留下的接入：已被其他程序改写，不再是 Studio 写的
fn foreign_text(removal: &Removal) -> Option<String> {
    if removal.foreign.is_empty() {
        return None;
    }
    let mut text = String::from("原样留下（已被其他程序改写过，Studio 不覆盖也不删除）：");
    for item in &removal.foreign {
        let _ = write!(
            text,
            "\n  {} · {} · {}",
            item.key,
            agent_name(&super::app_agent(&item.agent)),
            home(&item.path)
        );
    }
    Some(text)
}

pub fn remove(ctx: &Ctx, input: &str) -> Result<Report, CliError> {
    let state = ctx.writer()?;
    let mcp = load(&state)?;
    let row = resolve(&mcp, input)?;
    let name = row.entry.name.clone();
    if row.managed {
        // 分组、项目正部署着它时，先停用（与界面一致）
        let id = &row.entry.id;
        let groups: Vec<(String, Vec<String>)> = mcp
            .groups()
            .into_iter()
            .filter_map(|g| {
                let agents: Vec<String> = mcp
                    .catalog
                    .active_groups
                    .iter()
                    .filter(|(_, records)| {
                        records
                            .iter()
                            .any(|r| r.group_id == g.id && r.entries.iter().any(|e| e.id == *id))
                    })
                    .map(|(agent, _)| super::app_agent(agent))
                    .collect();
                (!agents.is_empty()).then(|| (g.name.clone(), agents))
            })
            .collect();
        if let Some((group, agents)) = groups.first() {
            return Err(CliError::failed(format!(
                "「{name}」正由分组「{group}」部署在 {} 上，请先停用",
                agent_names(agents)
            ))
            .hint(format!(
                "skill-studio mcp groups disable {} --agent {}",
                arg(group),
                agents.join(",")
            )));
        }
        let projects: Vec<(String, Vec<String>)> = mcp
            .config
            .projects
            .iter()
            .filter_map(|p| {
                let record = mcp.catalog.projects.get(&p.id)?;
                let agents: Vec<String> = record
                    .active
                    .iter()
                    .filter(|(_, d)| d.entries.iter().any(|e| e.id == *id))
                    .map(|(agent, _)| super::app_agent(agent))
                    .collect();
                (!agents.is_empty()).then(|| (p.name.clone(), agents))
            })
            .collect();
        if let Some((project, agents)) = projects.first() {
            return Err(CliError::failed(format!(
                "「{name}」正写在项目「{project}」里（{}），请先停用",
                agent_names(agents)
            ))
            .hint(format!(
                "skill-studio mcp projects disable {} --agent {}",
                arg(project),
                agents.join(",")
            )));
        }
        if !ctx.yes {
            return Err(CliError::confirm(format!(
                "把「{name}」移出 MCP Hub 需要确认：加 --yes 再运行"
            ))
            .hint("移出 Hub，并把它的接入还原成原来的配置；扫描到的原有配置和被其他程序改写过的接入原样保留。配置文件会先备份"));
        }
        let removal = service::entries::remove(&state, id, true)?;
        notify_gateway(&service::dir(&state));
        let message = if removal.removed > 0 {
            format!(
                "已把「{name}」移出 MCP Hub，还原了 {} 处接入（配置文件已备份）",
                removal.removed
            )
        } else {
            format!("已把「{name}」移出 MCP Hub")
        };
        return Ok(Report::written(
            message,
            true,
            (removal.removed > 0).then_some(super::RELOAD),
            json!(removal
                .foreign
                .iter()
                .map(|f| json!({"agent": super::app_agent(&f.agent), "path": f.path, "project": f.project, "key": f.key}))
                .collect::<Vec<_>>()),
            foreign_text(&removal),
            json!({ "mcp": name, "restored": removal.removed }),
        ));
    }
    let places: Vec<String> = row.sources.iter().map(place).collect();
    if !ctx.yes {
        return Err(CliError::confirm(format!(
            "从 Agent 配置里删掉原有的「{name}」需要确认：加 --yes 再运行"
        ))
        .hint(format!(
            "会删掉这 {} 处：{}；配置里的其他内容不动，文件会先备份",
            places.len(),
            join(&places)
        )));
    }
    let sources: Vec<_> = row
        .sources
        .iter()
        .map(|s| service::entries::SourceRef {
            id: s.id.clone(),
            definition: s.definition.clone(),
        })
        .collect();
    service::entries::remove_sources(&state, &sources)?;
    Ok(written(
        format!(
            "已从 Agent 配置里删掉「{name}」的 {} 处原有配置（文件已备份）",
            places.len()
        ),
        true,
        true,
        &[],
        json!({ "mcp": name, "removed": places }),
    ))
}
