//! MCP：Hub 里的 MCP、MCP 分组与项目里的 MCP。口径与界面一致：Hub 的一行对应前端
//! `rows()` 的一行，「有待应用修改」「需要检查」对应 MCP 分组页、MCP 项目页的卡片状态。
//!
//! 输出里的 MCP 定义一律隐藏密钥（环境变量、请求头、网址参数、像令牌的参数）：命令行的
//! 输出会进到 Agent 的上下文里。

mod groups;
mod hub;
mod projects;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use skill_studio_core::models::agent::AGENTS;
use skill_studio_core::models::config::AppConfig;
use skill_studio_mcp::agents as mcp_agents;
use skill_studio_mcp::discovery::{Discovered, Source};
use skill_studio_mcp::management::{groups::Group, projects::Project, Binding, Catalog, Entry};
use skill_studio_mcp::native;
use skill_studio_service::state::AppState;

use super::Ctx;
use crate::args::McpCommand;
use crate::error::CliError;
use crate::redact::{secret_name, HIDDEN};
use crate::report::{agent_name, home};
use crate::view::AgentState;

pub fn run(command: &McpCommand, ctx: &Ctx) -> Result<crate::report::Report, CliError> {
    match command {
        McpCommand::List { agent, query } => hub::list(ctx, agent.as_deref(), query.as_deref()),
        McpCommand::Show { mcp } => hub::show(ctx, mcp),
        McpCommand::Add {
            name,
            url,
            headers,
            env,
            config,
            gateway,
            command,
        } => hub::add(
            ctx,
            hub::Input {
                name: name.as_deref(),
                url: url.as_deref(),
                headers,
                env,
                config: config.as_deref(),
                gateway: *gateway,
                command,
            },
        ),
        McpCommand::Set {
            mcp,
            name,
            env,
            unset_env,
            headers,
            unset_headers,
            url,
            command,
        } => hub::set(
            ctx,
            mcp,
            hub::Change {
                name: name.as_deref(),
                env,
                unset_env,
                headers,
                unset_headers,
                url: url.as_deref(),
                command,
            },
        ),
        McpCommand::Remove { mcp } => hub::remove(ctx, mcp),
        McpCommand::Groups(command) => groups::run(command, ctx),
        McpCommand::Projects(command) => projects::run(command, ctx),
    }
}

/// Hub 里的一行：托管的 MCP，或扫描到的原有配置（`managed` 为 false）。
/// 同一个 MCP 配在几个地方时合成一行，`sources` 是它在各 Agent 配置里的位置
pub struct Row {
    pub entry: Entry,
    pub managed: bool,
    pub sources: Vec<Source>,
}

/// MCP 的一览：托管状态、Hub 的各行，以及 Skill 配置（项目、设置）
pub struct Mcp {
    pub config: AppConfig,
    pub catalog: Catalog,
    pub rows: Vec<Row>,
    pub warnings: Vec<String>,
}

pub fn load(state: &AppState) -> Result<Mcp, CliError> {
    let overview = skill_studio_service::mcp::overview(state, None)?;
    let rows = rows(&overview.catalog, &overview.scanned.discovered);
    Ok(Mcp {
        config: state.config().clone(),
        catalog: overview.catalog,
        rows,
        warnings: overview.warnings,
    })
}

fn same(binding: &Binding, source: &Source) -> bool {
    binding.agent == source.agent
        && binding.path == source.path
        && binding.key == source.key
        && binding.project == source.project
}

/// 与前端 `rows()` 相同的合并规则：扫描到的配置先认分组、项目里记下的引用，再并进
/// 对应的托管条目（接入位置相同或定义相同），剩下的作为原有配置单独成行
fn rows(catalog: &Catalog, discovered: &[Discovered]) -> Vec<Row> {
    let owned: Vec<&Binding> = catalog.owned_bindings().collect();
    let references: Vec<&Entry> = catalog
        .groups
        .iter()
        .flat_map(|g| &g.references)
        .chain(catalog.projects.values().flat_map(|p| &p.references))
        .collect();
    let mut result: Vec<Row> = catalog
        .entries
        .iter()
        .map(|entry| Row {
            entry: entry.clone(),
            managed: true,
            sources: vec![],
        })
        .collect();
    for found in discovered {
        let reference = references.iter().find(|entry| {
            entry.id == found.id
                || entry
                    .bindings
                    .iter()
                    .any(|b| found.sources.iter().any(|s| same(b, s)))
                || owned
                    .iter()
                    .any(|b| b.id == entry.id && found.sources.iter().any(|s| same(b, s)))
        });
        let source = found
            .sources
            .iter()
            .find(|s| reference.is_some_and(|r| r.bindings.iter().any(|b| same(b, s))))
            .or(found.sources.first());
        let definition = match source {
            Some(source) => native::canonical(&source.definition, &source.agent),
            None => native::canonical(&json!({}), "claude"),
        };
        let matched = result.iter_mut().find(|row| {
            found.managed_id.as_deref() == Some(row.entry.id.as_str())
                || row
                    .entry
                    .bindings
                    .iter()
                    .chain(owned.iter().copied().filter(|b| b.id == row.entry.id))
                    .any(|b| found.sources.iter().any(|s| same(b, s)))
                || native::canonical(&row.entry.definition, "claude") == definition
        });
        if let Some(row) = matched {
            row.sources.extend(found.sources.iter().cloned());
            continue;
        }
        result.push(Row {
            entry: Entry {
                id: reference
                    .map(|r| r.id.clone())
                    .unwrap_or_else(|| found.id.clone()),
                name: found.name.clone(),
                mode: "direct".into(),
                definition,
                ..Default::default()
            },
            managed: false,
            sources: found.sources.clone(),
        });
    }
    result
}

/// 部署记录里的成员快照与 Hub 里现在的定义比较（与前端 entrySnapshot 一致）
fn same_definition(a: &Entry, b: &Entry) -> bool {
    a.id == b.id
        && a.name == b.name
        && a.mode == b.mode
        && a.definition == b.definition
        && a.oauth == b.oauth
        && a.client_id == b.client_id
        && a.scopes == b.scopes
}

fn same_members(a: &[String], b: &[String]) -> bool {
    a.iter().collect::<BTreeSet<_>>() == b.iter().collect::<BTreeSet<_>>()
}

/// MCP 用 `claude`，命令行和 Skill 一侧用 `claude-code`
pub fn mcp_agent(app: &str) -> &str {
    if app == "claude-code" {
        "claude"
    } else {
        app
    }
}

pub fn app_agent(mcp: &str) -> String {
    skill_studio_service::mcp::app_agent(mcp).to_string()
}

/// 按 Agent 的固定顺序排好、换成命令行里的 Agent id
fn app_agents<'a>(mcp: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut ids: Vec<String> = mcp.into_iter().map(|a| app_agent(a)).collect();
    ids.sort_by_key(|id| AGENTS.iter().position(|a| a.id == id).unwrap_or(usize::MAX));
    ids.dedup();
    ids
}

impl Mcp {
    pub fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.entry.id == id)
    }

    pub fn name(&self, id: &str) -> Option<&str> {
        self.row(id).map(|r| r.entry.name.as_str())
    }

    /// 成员的名字；已不在 Hub、也扫描不到时写成「已缺失：id」
    pub fn member_name(&self, id: &str) -> String {
        self.name(id)
            .map(str::to_string)
            .unwrap_or_else(|| format!("已缺失：{id}"))
    }

    pub fn group(&self, id: &str) -> Option<&Group> {
        self.catalog.groups.iter().find(|g| g.id == id)
    }

    pub fn groups(&self) -> Vec<&Group> {
        let mut groups: Vec<_> = self.catalog.groups.iter().collect();
        groups.sort_by_key(|g| g.sort_order);
        groups
    }

    /// 哪些 Agent 启用了这个分组（命令行的 Agent id）
    pub fn group_agents(&self, group_id: &str) -> Vec<String> {
        app_agents(
            self.catalog
                .active_groups
                .iter()
                .filter(|(_, records)| records.iter().any(|r| r.group_id == group_id))
                .map(|(agent, _)| agent),
        )
    }

    /// 部署快照里的成员跟不上 Hub 里现在的定义时，说明哪里变了
    fn drift(&self, deployed: &[Entry]) -> Vec<String> {
        deployed
            .iter()
            .filter_map(|e| match self.row(&e.id) {
                None => Some(format!("{} 已缺失", e.name)),
                Some(row) if !same_definition(&row.entry, e) => {
                    Some(format!("{} 的配置已修改", row.entry.name))
                }
                Some(_) => None,
            })
            .collect()
    }

    pub fn group_states(&self, group: &Group) -> Vec<AgentState> {
        let issues = skill_studio_mcp::management::groups::issues(&self.catalog);
        let mut states: Vec<AgentState> = self
            .catalog
            .active_groups
            .iter()
            .filter_map(|(agent, records)| {
                let record = records.iter().find(|r| r.group_id == group.id)?;
                let ids: Vec<String> = record.entries.iter().map(|e| e.id.clone()).collect();
                let mut pending = Vec::new();
                if !same_members(&ids, &group.entry_ids) {
                    pending.push("成员已变化".to_string());
                }
                pending.extend(self.drift(&record.entries));
                Some(AgentState {
                    agent: app_agent(agent),
                    pending,
                    issues: issues
                        .get(agent)
                        .and_then(|by_group| by_group.get(&group.id))
                        .cloned()
                        .into_iter()
                        .collect(),
                })
            })
            .collect();
        states.sort_by_key(|s| AGENTS.iter().position(|a| a.id == s.agent));
        states
    }

    /// 项目的 MCP 成员：单独选的在前，再展开绑定的分组，去重
    pub fn project_members(&self, record: &Project) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        let groups = record.group_ids.iter().filter_map(|id| self.group(id));
        for id in record
            .entry_ids
            .iter()
            .chain(groups.flat_map(|g| g.entry_ids.iter()))
        {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids
    }

    pub fn project_agents(&self, record: &Project) -> Vec<String> {
        app_agents(record.active.keys())
    }

    pub fn project_states(&self, project_id: &str, record: &Project) -> Vec<AgentState> {
        let issues = skill_studio_mcp::management::projects::issues(&self.catalog);
        let existing: Vec<String> = self
            .project_members(record)
            .into_iter()
            .filter(|id| self.row(id).is_some())
            .collect();
        let mut states: Vec<AgentState> = record
            .active
            .iter()
            .map(|(agent, deployed)| {
                let ids: Vec<String> = deployed.entries.iter().map(|e| e.id.clone()).collect();
                let mut pending = Vec::new();
                if !same_members(&ids, &existing) {
                    pending.push("成员已变化".to_string());
                }
                pending.extend(self.drift(&deployed.entries));
                AgentState {
                    agent: app_agent(agent),
                    pending,
                    issues: issues
                        .get(project_id)
                        .and_then(|by_agent| by_agent.get(agent))
                        .cloned()
                        .into_iter()
                        .collect(),
                }
            })
            .collect();
        states.sort_by_key(|s| AGENTS.iter().position(|a| a.id == s.agent));
        states
    }

    /// 项目成员里已经找不到的 MCP，和已删除的分组
    pub fn project_missing(&self, record: &Project) -> Vec<String> {
        let mut missing: Vec<String> = self
            .project_members(record)
            .into_iter()
            .filter(|id| self.row(id).is_none())
            .collect();
        missing.extend(
            record
                .group_ids
                .iter()
                .filter(|id| self.group(id).is_none())
                .map(|id| format!("分组 {id}")),
        );
        missing
    }
}

/// 隐藏网址里的参数值（有的服务把令牌放在查询参数里）
fn redact_url(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };
    let query: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, _)) => format!("{name}={HIDDEN}"),
            None => pair.to_string(),
        })
        .collect();
    format!("{base}?{}", query.join("&"))
}

/// 隐藏像令牌的参数：`--api-key xxx`、`--token=xxx`
fn redact_args(args: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut hide_next = false;
    for arg in args {
        let Some(text) = arg.as_str() else {
            out.push(arg.clone());
            continue;
        };
        if hide_next {
            out.push(json!(HIDDEN));
            hide_next = false;
            continue;
        }
        if text.starts_with('-') {
            if let Some((flag, _)) = text.split_once('=') {
                if secret_name(flag) {
                    out.push(json!(format!("{flag}={HIDDEN}")));
                    continue;
                }
            } else if secret_name(text) {
                hide_next = true;
            }
        }
        out.push(arg.clone());
    }
    out
}

/// MCP 定义去掉密钥：环境变量与请求头只留名字，网址参数和像令牌的参数换成 <已隐藏>
pub fn redact(definition: &Value) -> Value {
    let mut value = definition.clone();
    let Some(map) = value.as_object_mut() else {
        return value;
    };
    for key in ["env", "environment", "headers", "http_headers"] {
        if let Some(Value::Object(entries)) = map.get_mut(key) {
            for value in entries.values_mut() {
                *value = json!(HIDDEN);
            }
        }
    }
    if let Some(url) = map.get("url").and_then(Value::as_str) {
        let url = redact_url(url);
        map.insert("url".into(), json!(url));
    }
    if let Some(Value::Array(args)) = map.get("args") {
        let args = redact_args(args);
        map.insert("args".into(), json!(args));
    }
    value
}

/// 一行写清怎么连：启动命令（含参数）或网址，密钥已隐藏
pub fn target(definition: &Value) -> String {
    let value = redact(definition);
    if let Some(url) = value.get("url").and_then(Value::as_str) {
        return url.to_string();
    }
    let mut words: Vec<String> = Vec::new();
    match value.get("command") {
        Some(Value::String(command)) => words.push(command.clone()),
        Some(Value::Array(parts)) => {
            words.extend(parts.iter().filter_map(Value::as_str).map(str::to_string))
        }
        _ => {}
    }
    if let Some(Value::Array(args)) = value.get("args") {
        words.extend(args.iter().filter_map(Value::as_str).map(str::to_string));
    }
    words
        .iter()
        .map(|w| crate::report::arg(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 连接方式：Hub 里的分直连和网关，扫描到的叫原有配置
pub fn kind(row: &Row) -> &'static str {
    if !row.managed {
        "原有配置"
    } else if row.entry.mode == "gateway" {
        "Studio 网关"
    } else {
        "Agent 直连"
    }
}

/// 配在哪里：「Codex ~/.codex/config.toml（用户全局）」
pub fn place(source: &Source) -> String {
    let mut text = format!(
        "{} {}（{}",
        agent_name(&app_agent(&source.agent)),
        home(&source.path),
        source.scope
    );
    if !source.enabled {
        text.push_str("，已停用");
    }
    text.push('）');
    text
}

pub fn source_json(source: &Source) -> Value {
    json!({
        "agent": app_agent(&source.agent), "path": source.path, "scope": source.scope,
        "key": source.key, "enabled": source.enabled, "project": source.project,
    })
}

/// 用名称找 MCP：ID、名称、不分大小写的名称。同名时 Hub 里的优先，仍分不出来就列出候选
pub fn resolve<'a>(mcp: &'a Mcp, input: &str) -> Result<&'a Row, CliError> {
    let input = input.trim();
    if let Some(row) = mcp.rows.iter().find(|r| r.entry.id == input) {
        return Ok(row);
    }
    let named = |exact: bool| -> Vec<&'a Row> {
        mcp.rows
            .iter()
            .filter(|r| {
                if exact {
                    r.entry.name == input
                } else {
                    r.entry.name.to_lowercase() == input.to_lowercase()
                }
            })
            .collect()
    };
    let mut matches = named(true);
    if matches.is_empty() {
        matches = named(false);
    }
    match matches.len() {
        1 => Ok(matches[0]),
        0 => {
            let lower = input.to_lowercase();
            let mut similar: Vec<String> = mcp
                .rows
                .iter()
                .filter(|r| r.entry.name.to_lowercase().contains(&lower))
                .map(|r| r.entry.name.clone())
                .collect();
            similar.sort();
            similar.dedup();
            similar.truncate(10);
            Err(CliError::not_found(format!("找不到 MCP「{input}」"))
                .candidates(similar)
                .hint("用 skill-studio mcp list 查看全部 MCP，或用 mcp add 添加到 Hub"))
        }
        _ => {
            let hub: Vec<_> = matches.iter().filter(|r| r.managed).collect();
            if hub.len() == 1 {
                return Ok(hub[0]);
            }
            Err(
                CliError::not_found(format!("有 {} 个 MCP 都叫「{input}」", matches.len()))
                    .candidates(
                        matches
                            .iter()
                            .map(|r| format!("{} · {} · ID {}", r.entry.name, kind(r), r.entry.id))
                            .collect(),
                    )
                    .hint("改写成它的 ID"),
            )
        }
    }
}

/// 多个 MCP，返回各行，去重并保持顺序
pub fn resolve_all<'a>(mcp: &'a Mcp, inputs: &[String]) -> Result<Vec<&'a Row>, CliError> {
    let mut rows: Vec<&Row> = Vec::new();
    for input in inputs.iter().filter(|i| !i.trim().is_empty()) {
        let row = resolve(mcp, input)?;
        if !rows.iter().any(|r| r.entry.id == row.entry.id) {
            rows.push(row);
        }
    }
    Ok(rows)
}

pub fn resolve_group<'a>(mcp: &'a Mcp, input: &str) -> Result<&'a Group, CliError> {
    let input = input.trim();
    let groups = mcp.groups();
    let found = groups
        .iter()
        .find(|g| g.id == input || g.name == input)
        .or_else(|| {
            let lower = input.to_lowercase();
            let matches: Vec<_> = groups
                .iter()
                .filter(|g| g.name.to_lowercase() == lower)
                .collect();
            (matches.len() == 1).then(|| matches[0])
        });
    found.copied().ok_or_else(|| {
        CliError::not_found(format!("找不到 MCP 分组「{input}」"))
            .candidates(groups.iter().map(|g| g.name.clone()).collect())
            .hint(if groups.is_empty() {
                "还没有 MCP 分组，用 skill-studio mcp groups create <名称> 新建"
            } else {
                "用 skill-studio mcp groups list 查看全部 MCP 分组"
            })
    })
}

/// 选中的原有配置只记下引用：交给后端重新扫描确认
pub fn import_ref(row: &Row) -> skill_studio_service::mcp::ImportRef {
    skill_studio_service::mcp::ImportRef {
        id: row.entry.id.clone(),
        definition: row.entry.definition.clone(),
        source_ids: row.sources.iter().map(|s| s.id.clone()).collect(),
    }
}

/// 网关接入要写进 Agent 配置的命令：桌面应用的可执行文件。命令行装在应用旁边（macOS 的
/// .app 里、Linux 的 /usr/bin、Windows 的安装目录）；找不到时只能管理 Agent 直连的 MCP
pub fn executable() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SKILL_STUDIO_APP_EXECUTABLE") {
        return Some(PathBuf::from(path));
    }
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let app = exe.with_file_name(format!("skill-studio{}", std::env::consts::EXE_SUFFIX));
    // 服务器上 ~/.skill-studio/bin/skill-studio 指向命令行自己，不是桌面应用
    let real = app.canonicalize().ok()?;
    (real != exe && real.is_file()).then_some(app)
}

/// 写入后通知运行中的网关重读托管状态；网关没在运行时替它清掉已删除服务的接入令牌和
/// OAuth 凭据（与桌面应用一致）。都是尽力而为，失败不影响刚完成的写入
pub fn notify_gateway(dir: &Path) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    let running = runtime
        .block_on(skill_studio_mcp::gateway::request(dir, "list", Value::Null))
        .is_ok();
    if !running {
        let _ = skill_studio_mcp::gateway::forget_removed(dir);
    }
}

/// 项目的 MCP 写在这个 Agent 的哪份配置里
pub fn project_file(agent: &str, record: &Project) -> String {
    if agent == "claude-code" && record.claude_local {
        return "~/.claude.json，只给自己用".into();
    }
    match mcp_agents::PROJECT_FILES
        .iter()
        .find(|f| match mcp_agent(agent) {
            "claude" => f.ends_with(".mcp.json"),
            "codex" => f.starts_with(".codex"),
            "opencode" => f.starts_with("opencode"),
            "pi" => f.starts_with(".pi"),
            "grok" => f.starts_with(".grok"),
            _ => false,
        }) {
        Some(file) => file.to_string(),
        None => "项目配置".into(),
    }
}

/// 改了 MCP 之后的提醒：Agent 要重新加载才生效（与界面一致）
pub const RELOAD: &str = "请在 Agent 中重新加载 MCP";

/// MCP 写操作的结果：做了什么、是否真有改动、要不要重新加载、原样留下了哪些接入
pub fn written(
    message: impl Into<String>,
    changed: bool,
    reload: bool,
    untouched: &[skill_studio_mcp::management::groups::Untouched],
    extra: Value,
) -> crate::report::Report {
    let items: Vec<Value> = untouched
        .iter()
        .map(|u| {
            json!({
                "agent": app_agent(&u.agent), "path": u.path, "project": u.project,
                "key": u.key, "tracked": u.tracked,
            })
        })
        .collect();
    crate::report::Report::written(
        message.into(),
        changed,
        reload.then_some(RELOAD),
        Value::Array(items),
        (!untouched.is_empty()).then(|| untouched_text(untouched)),
        extra,
    )
}

/// 启停、写入时原样留下的接入：部署后被其他程序改写过，Studio 不覆盖也不删除
pub fn untouched_text(untouched: &[skill_studio_mcp::management::groups::Untouched]) -> String {
    let mut text = String::from("原样留下（部署后被其他程序改写过，Studio 不覆盖也不删除）：");
    for item in untouched {
        let state = if item.tracked {
            "仍由分组或项目管理，标为需要检查"
        } else {
            "已不再管理，留给你处理"
        };
        text.push_str(&format!(
            "\n  {} · {} · {}（{state}）",
            item.key,
            agent_name(&app_agent(&item.agent)),
            home(&item.path)
        ));
    }
    text
}
