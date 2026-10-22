//! Remote MCP inventory: names and owners only, with no management or runtime IO.
use serde::Serialize;
use skill_studio_core::{
    fs::atomic,
    models::config::{AppConfig, CONFIG_VERSION},
    services::store::Store,
};
use skill_studio_mcp::discovery;

#[derive(Serialize)]
pub struct InventoryEntry {
    id: String,
    name: String,
    agents: Vec<String>,
}

#[derive(Serialize)]
pub struct Inventory {
    servers: Vec<InventoryEntry>,
    warnings: Vec<String>,
}

pub fn scan_inventory(store: &Store) -> Result<Inventory, String> {
    // AppState::bootstrap and Store::load can recover transactions and reconcile
    // policies. Bypass both, even in a writable Skill-management session.
    let config: AppConfig = atomic::read_json_file(&store.config_path())
        .map_err(|_| "无法读取 Studio 配置，请检查文件权限与格式".to_string())?
        .unwrap_or_default();
    if config.version != CONFIG_VERSION {
        return Err("配置版本不匹配，请更新远程组件".into());
    }
    if !config.settings.manage_mcp {
        return Err("MCP 展示已关闭".into());
    }
    let (mut files, mut warnings) = discovery::scan_files(&config);
    files.retain(|file| {
        let app = if file.agent == "claude" {
            "claude-code"
        } else {
            &file.agent
        };
        !config.settings.disabled_agents.iter().any(|id| id == app)
    });
    let scanned = discovery::scan(&files, &[], &store.dir().join("mcp"));
    warnings.extend(scanned.warnings);
    let mut servers: Vec<_> = scanned
        .discovered
        .into_iter()
        .map(|entry| {
            let mut agents: Vec<_> = entry.sources.into_iter().map(|s| s.agent).collect();
            agents.sort();
            agents.dedup();
            // Never serialize definitions, commands, headers, URLs or OAuth data.
            InventoryEntry {
                id: entry.id,
                name: entry.name,
                agents,
            }
        })
        .collect();
    if config.settings.show_codex_builtin_mcp {
        servers.extend(scanned.builtins.into_iter().map(|entry| InventoryEntry {
            id: format!(
                "builtin:{}:{}:{}",
                entry.path.display(),
                entry.scope,
                entry.name
            ),
            name: entry.name,
            agents: vec![entry.agent],
        }));
    }
    servers.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(Inventory { servers, warnings })
}
