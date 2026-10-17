//! 关掉 Agent 对别家目录的兼容读取，分组才管得住它。
//!
//! Grok 默认还会扫描 `~/.claude/skills` 和项目里的 `.claude/skills`（Claude Code 兼容）：
//! 给 Claude Code 部署的 skill，Grok 也会加载。管理 Grok 期间在它的 `~/.grok/config.toml`
//! 写 `[compat.claude] skills = false`，退出管理时撤回。
//!
//! 只改一次：Studio 关掉之后用户又自己打开，就不再覆盖；用户本来就关着的，退出时也不碰。
//! Grok 的 `[compat.codex]` 目前是保留字段、不扫描任何目录（见 Grok 配置文档
//! 「Harness compatibility」），所以不需要处理。

use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table, TableLike};

use crate::error::{Error, Result};
use crate::fs::atomic;
use crate::models::agent::find_agent;
use crate::models::config::AppConfig;

pub struct Switch {
    pub agent: &'static str,
    /// 记在 `AppConfig::native_isolation` 里的标识
    pub id: &'static str,
    /// 开关所在的表，如 `["compat", "claude"]`
    pub table: &'static [&'static str],
    pub key: &'static str,
}

pub const SWITCHES: &[Switch] = &[Switch {
    agent: "grok",
    id: "compat.claude.skills",
    table: &["compat", "claude"],
    key: "skills",
}];

pub struct Change {
    pub path: PathBuf,
    switch: &'static Switch,
    /// true：写入 `false`；false：撤回 Studio 写入的 `false`
    isolate: bool,
}

/// 规划要改的开关，并把记录同步到 `next`。读不懂的配置一律不碰。
pub fn plan(config: &AppConfig, next: &mut AppConfig) -> Vec<Change> {
    let overrides = &config.settings.agent_dir_overrides;
    let mut changes = Vec::new();
    for switch in SWITCHES {
        let Some(agent) = find_agent(switch.agent) else {
            continue;
        };
        let managed = !config
            .settings
            .disabled_agents
            .iter()
            .any(|id| id == agent.id);
        let recorded = config
            .native_isolation
            .get(agent.id)
            .is_some_and(|ids| ids.iter().any(|id| id == switch.id));
        let dir = agent.resolved_config_dir(overrides);
        let path = dir.join("config.toml");
        let change = Change {
            path,
            switch,
            isolate: managed,
        };
        if managed && !recorded {
            // 没装（没有配置目录）就不替它创建配置
            if !dir.is_dir() {
                continue;
            }
            match read(&change.path) {
                Some(doc) if value(&doc, switch) != Some(false) => {
                    if edit(&mut doc.clone(), &change).is_err() {
                        continue;
                    }
                }
                // 用户自己已经关了，或者文件读不懂
                _ => continue,
            }
            next.native_isolation
                .entry(agent.id.into())
                .or_default()
                .push(switch.id.into());
            changes.push(change);
        } else if !managed && recorded {
            if let Some(ids) = next.native_isolation.get_mut(agent.id) {
                ids.retain(|id| id != switch.id);
                if ids.is_empty() {
                    next.native_isolation.remove(agent.id);
                }
            }
            // 用户之后自己改成别的值，或文件读不懂：只清记录，不动文件
            if read(&change.path).is_some_and(|doc| value(&doc, switch) == Some(false)) {
                changes.push(change);
            }
        }
    }
    changes
}

/// 写到 `path`（调用方传入事务日志返回的物理路径）
pub fn apply(change: &Change, path: &Path) -> Result<()> {
    let mut doc =
        read(path).ok_or_else(|| Error::config(format!("无法解析 {}", path.display())))?;
    edit(&mut doc, change)?;
    atomic::write_text_file(path, &doc.to_string())
}

fn read(path: &Path) -> Option<DocumentMut> {
    match std::fs::read_to_string(path) {
        Ok(text) => text.parse().ok(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(DocumentMut::new()),
        Err(_) => None,
    }
}

fn value(doc: &DocumentMut, switch: &Switch) -> Option<bool> {
    let mut item = doc.as_item();
    for key in switch.table {
        item = item.get(key)?;
    }
    item.get(switch.key)?.as_bool()
}

fn edit(doc: &mut DocumentMut, change: &Change) -> Result<()> {
    fn walk(table: &mut dyn TableLike, path: &[&str], key: &str, isolate: bool) -> Result<()> {
        let Some((first, rest)) = path.split_first() else {
            if isolate {
                table.insert(key, toml_edit::value(false));
            } else {
                table.remove(key);
            }
            return Ok(());
        };
        if table.get(first).is_none() {
            if !isolate {
                return Ok(());
            }
            let mut child = Table::new();
            // `[compat]` 只作为父级，不单独输出一个空表头
            child.set_implicit(!rest.is_empty());
            table.insert(first, Item::Table(child));
        }
        let child = table
            .get_mut(first)
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| Error::config(format!("config.toml 里的 {first} 不是表")))?;
        walk(child, rest, key, isolate)?;
        // 撤回时把 Studio 留下的空表一并去掉
        let empty = child.is_empty();
        if !isolate && empty {
            table.remove(first);
        }
        Ok(())
    }
    walk(
        doc.as_table_mut(),
        change.switch.table,
        change.switch.key,
        change.isolate,
    )
}
