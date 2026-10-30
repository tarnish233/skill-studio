use std::collections::HashMap;
use std::path::PathBuf;

use skill_studio_core::models::config::{AppConfig, Settings};
use skill_studio_core::models::skill::LinkMode;

use crate::state::AppState;

/// 整份配置（前端首屏一次性拿走）
pub fn get_config(state: &AppState) -> Result<AppConfig, String> {
    Ok(state.config().clone())
}

pub fn get_settings(state: &AppState) -> Result<Settings, String> {
    let config = state.config();
    let mut settings = config.settings.clone();
    if !settings.cli_skill_enabled && !config.builtin_skills.is_empty() {
        let remaining = match state.studio().builtin_retained(&config) {
            Ok(remaining) if remaining.is_empty() => None,
            Ok(remaining) => Some(
                remaining
                    .iter()
                    .map(|skill| format!("「{}」{}", skill.name, skill.reason))
                    .collect::<Vec<_>>()
                    .join("；"),
            ),
            Err(error) => Some(format!("无法检查内置 skill：{error}")),
        };
        settings.cli_skill_warning = remaining.map(|text| {
            format!(
                "内置 skill 仍留在 Hub：{text}。CLI 已关闭；请处理上述原因或等待下次启动自动清理。"
            )
        });
    }
    Ok(settings)
}

/// 设置的增量补丁。字段为 `None` 表示不改动。
///
/// 用一个结构体而不是一长串可选参数：Tauri 命令参数超过 7 个 clippy 会报警，
/// 而且前端只想改一项时也不必凑齐全部参数。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ManagedAgentPatch {
    pub agent: String,
    pub enabled: bool,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsPatch {
    pub disabled_agents: Option<Vec<String>>,
    /// 单个 Agent 的管理开关，在锁内按最新状态增量处理。
    pub managed_agent: Option<ManagedAgentPatch>,
    pub show_codex_builtin_mcp: Option<bool>,
    pub manage_mcp: Option<bool>,
    pub cli_skill_enabled: Option<bool>,
    pub default_link_mode: Option<LinkMode>,
    pub preserve_manual_skills: Option<bool>,
    pub language: Option<String>,
    pub theme: Option<String>,
    pub agent_dir_overrides: Option<HashMap<String, PathBuf>>,
    /// 按 Agent 增量修改，None 取消覆盖；在配置锁内合并，避免覆盖其他 Agent 的修改。
    pub agent_dir_updates: Option<HashMap<String, Option<PathBuf>>>,
    pub hub_dir: Option<PathBuf>,
    /// 显式把 Hub 目录恢复为默认值（`hub_dir: None` 只是"不改动"）
    pub clear_hub_dir: Option<bool>,
    pub backup_keep: Option<usize>,
}

/// CLI backup-retention update with the value observed during confirmation preflight.
pub fn update_settings_with_backup_snapshot(
    state: &AppState,
    patch: SettingsPatch,
    expected_backup_keep: Option<usize>,
) -> Result<Settings, String> {
    update_settings_with_builtin_snapshot(state, patch, None, "", expected_backup_keep)
}

pub fn update_settings(state: &AppState, patch: SettingsPatch) -> Result<Settings, String> {
    update_settings_with_builtin(state, patch, None, "")
}

/// `source` is the desktop app's bundled resource directory; the remote helper
/// has no copy and is never expected to install the builtin on its own.
pub fn update_settings_with_builtin(
    state: &AppState,
    patch: SettingsPatch,
    source: Option<&std::path::Path>,
    version: &str,
) -> Result<Settings, String> {
    update_settings_with_builtin_snapshot(state, patch, source, version, None)
}

fn update_settings_with_builtin_snapshot(
    state: &AppState,
    patch: SettingsPatch,
    source: Option<&std::path::Path>,
    version: &str,
    expected_backup_keep: Option<usize>,
) -> Result<Settings, String> {
    let fields = serde_json::to_value(&patch).map_err(|e| e.to_string())?;
    let count = fields
        .as_object()
        .unwrap()
        .values()
        .filter(|v| !v.is_null())
        .count();
    for (present, label) in [
        (
            patch.disabled_agents.is_some() || patch.managed_agent.is_some(),
            "应用管理开关",
        ),
        (
            patch.preserve_manual_skills.is_some(),
            "保留手动 skill 策略",
        ),
        (patch.manage_mcp.is_some(), "MCP 管理开关"),
        (patch.cli_skill_enabled.is_some(), "CLI 与内置 skill 开关"),
    ] {
        if present && count > 1 {
            return Err(format!("{label}需单独保存"));
        }
    }
    if patch.agent_dir_overrides.is_some() && patch.agent_dir_updates.is_some() {
        return Err("目录全量覆盖与逐项修改不能同时保存".into());
    }
    if let Some(agent) = &patch.managed_agent {
        state
            .set_managed_agent(&agent.agent, agent.enabled)
            .map_err(String::from)?;
        return get_settings(state);
    }
    if let Some(ids) = &patch.disabled_agents {
        state.set_managed_agents(ids).map_err(String::from)?;
        return get_settings(state);
    }
    if let Some(preserve) = patch.preserve_manual_skills {
        state
            .set_manual_skill_policy(preserve)
            .map_err(String::from)?;
        return get_settings(state);
    }
    if let Some(enabled) = patch.manage_mcp {
        state.set_mcp_management(enabled).map_err(String::from)?;
        return get_settings(state);
    }
    if let Some(enabled) = patch.cli_skill_enabled {
        state
            .set_cli_skill_enabled(enabled, source, version)
            .map_err(String::from)?;
        return get_settings(state);
    }
    state
        .mutate(|studio, config| {
            if patch.backup_keep.is_some()
                && expected_backup_keep
                    .is_some_and(|observed| observed != config.settings.backup_keep)
            {
                return Err(skill_studio_core::Error::Other(
                    crate::state::CHANGED_ELSEWHERE.into(),
                ));
            }
            let paths_changed = patch.hub_dir.is_some()
                || patch.clear_hub_dir.unwrap_or(false)
                || patch.agent_dir_overrides.is_some()
                || patch.agent_dir_updates.is_some();
            let old_hub = studio.store().hub_dir(config);
            let s = &mut config.settings;
            if let Some(show) = patch.show_codex_builtin_mcp {
                s.show_codex_builtin_mcp = show;
            }
            if let Some(mode) = patch.default_link_mode {
                s.default_link_mode = mode;
            }
            if let Some(language) = patch.language {
                s.language = language;
            }
            if let Some(theme) = patch.theme {
                s.theme = theme;
            }
            let mut overrides = patch.agent_dir_overrides;
            if let Some(updates) = patch.agent_dir_updates {
                let mut next = s.agent_dir_overrides.clone();
                for (agent, path) in updates {
                    skill_studio_core::models::agent::require_agent(&agent)?;
                    match path {
                        Some(path) => {
                            next.insert(agent, path);
                        }
                        None => {
                            next.remove(&agent);
                        }
                    }
                }
                overrides = Some(next);
            }
            if let Some(overrides) = overrides {
                let next: HashMap<_, _> = overrides
                    .into_iter()
                    .filter(|(_, path)| !path.as_os_str().is_empty())
                    .collect();
                if config
                    .active_groups
                    .keys()
                    .chain(config.policy_suspensions.keys())
                    .any(|id| next.get(id) != s.agent_dir_overrides.get(id))
                {
                    return Err(skill_studio_core::Error::invalid(
                        "请先开启保留手动 skill 并停用该 Agent 的分组，再修改目录",
                    ));
                }
                s.agent_dir_overrides = next;
            }
            if patch.clear_hub_dir.unwrap_or(false) {
                s.hub_dir = None;
            } else if let Some(hub) = patch.hub_dir {
                s.hub_dir = Some(hub);
            }
            if let Some(keep) = patch.backup_keep {
                s.backup_keep = keep.min(100);
            }
            if paths_changed {
                validate_hub_change(&old_hub, &studio.store().hub_dir(config), config)?;
            }
            Ok(())
        })
        .map_err(String::from)?;
    get_settings(state)
}

/// 配置备份列表，最新在前
pub fn list_backups(state: &AppState) -> Result<Vec<String>, String> {
    Ok(state
        .studio()
        .store()
        .list_backups()
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

/// 从备份恢复配置。恢复动作本身也会先备份当前状态，可再次回退。
pub fn restore_backup(state: &AppState, path: String) -> Result<AppConfig, String> {
    state
        .restore_backup(std::path::Path::new(&path))
        .map_err(Into::into)
}

/// 删除一份配置备份。恢复模式下不删：那时备份是唯一的退路。
pub fn delete_backup(state: &AppState, path: String) -> Result<(), String> {
    state
        .with_exclusive(|studio, _| studio.store().delete_backup(std::path::Path::new(&path)))
        .map_err(Into::into)
}

/// 配置目录路径，供前端"在 Finder 中显示"用
pub fn get_config_dir(state: &AppState) -> Result<String, String> {
    Ok(state.studio().store().dir().to_string_lossy().to_string())
}

fn validate_hub_change(
    old_hub: &std::path::Path,
    hub: &std::path::Path,
    config: &AppConfig,
) -> skill_studio_core::Result<()> {
    use skill_studio_core::{fs::paths, models::agent::AGENTS, services::linker, Error};
    if !hub.is_absolute() {
        return Err(Error::invalid("请选择绝对路径作为 Hub 目录"));
    }
    let roots: Vec<_> = AGENTS
        .iter()
        .flat_map(|a| a.resolved_global_roots(&config.settings.agent_dir_overrides))
        .collect();
    linker::ensure_distinct_roots(hub, &roots)?;
    if !paths::paths_alias(hub, old_hub)
        && (!config.active_groups.is_empty()
            || (old_hub.exists()
                && std::fs::read_dir(old_hub)
                    .map_err(|e| Error::io(old_hub, e))?
                    .any(|entry| entry.map_or(true, |e| e.file_name() != ".DS_Store"))))
    {
        return Err(Error::invalid(
            "当前 Hub 非空或有启用中的分组，请先移出或剔除已托管的 skill；修改目录不会自动迁移文件",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use skill_studio_core::services::store::Store;
    #[test]
    fn hub_change_rejects_relative_overlapping_and_nonempty_roots() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("old");
        let new = temp.path().join("new");
        std::fs::create_dir(&old).unwrap();
        let mut config = AppConfig::default();
        assert!(validate_hub_change(&old, &new, &config).is_ok());
        assert!(validate_hub_change(&old, std::path::Path::new("relative"), &config).is_err());
        config
            .settings
            .agent_dir_overrides
            .insert("codex".into(), new.clone());
        assert!(validate_hub_change(&old, &new, &config).is_err());
        config.settings.agent_dir_overrides.clear();
        std::fs::write(old.join("SKILL.md"), "keep").unwrap();
        assert!(validate_hub_change(&old, &new, &config).is_err());
        assert!(validate_hub_change(&old, &old, &config).is_ok());
    }

    #[test]
    fn mcp_management_toggle_is_persisted_and_isolated_from_other_settings() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::bootstrap(Store::new(temp.path().to_path_buf())).unwrap();
        assert!(get_settings(&state).unwrap().manage_mcp);
        let off = update_settings(
            &state,
            SettingsPatch {
                manage_mcp: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!off.manage_mcp);
        assert!(
            skill_studio_mcp::management::read(&temp.path().join("mcp"))
                .unwrap()
                .suspended
        );
        assert!(!state.studio().load_config().unwrap().settings.manage_mcp);
        let on = update_settings(
            &state,
            SettingsPatch {
                manage_mcp: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(on.manage_mcp);
        assert!(
            !skill_studio_mcp::management::read(&temp.path().join("mcp"))
                .unwrap()
                .suspended
        );
    }
}

#[cfg(test)]
mod audit_regressions {
    use super::*;
    #[test]
    fn finder_metadata_does_not_block_hub_change() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join(".DS_Store"), "finder metadata").unwrap();
        assert!(validate_hub_change(&old, &tmp.path().join("new"), &AppConfig::default()).is_ok());
        std::fs::write(old.join("important.txt"), "keep").unwrap();
        assert!(validate_hub_change(&old, &tmp.path().join("new"), &AppConfig::default()).is_err());
    }
}
