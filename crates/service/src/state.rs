use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use skill_studio_core::models::config::AppConfig;
use skill_studio_core::services::store::Store;
use skill_studio_core::services::studio::Studio;
use skill_studio_core::Result;

/// 全局状态。内部全 `Arc`，本身 `Clone`；锁只包住 `AppConfig`，
/// 不做粗粒度全局锁（cc-switch 的教训：粗锁跨 IO 会连锁阻塞）。
#[derive(Clone)]
pub struct AppState {
    studio: Arc<Studio>,
    config: Arc<RwLock<AppConfig>>,
    recovery_only: bool,
}

impl AppState {
    /// 启动时加载配置。加载失败**不静默回落默认值**，把错误交给上层决定怎么提示。
    pub fn bootstrap(store: Store) -> Result<Self> {
        let studio = Studio::new(store);
        let mut config = studio.load_config()?;
        let mcp_dir = studio.store().dir().join("mcp");
        if mcp_dir.join("catalog.json").exists()
            || mcp_dir.join("management-transaction.json").exists()
        {
            // Native configurations and suspended are committed together. Recover
            // that transaction before reconciling the separately saved UI setting.
            // Never reapply native entries here: they may have been edited since.
            let enabled = !skill_studio_mcp::management::read(&mcp_dir)
                .map_err(|error| {
                    skill_studio_core::Error::invalid(format!("恢复 MCP 管理状态失败：{error:#}"))
                })?
                .suspended;
            if config.settings.manage_mcp != enabled {
                config.settings.manage_mcp = enabled;
                studio.save_config(&config)?;
            }
        }
        Ok(Self {
            studio: Arc::new(studio),
            config: Arc::new(RwLock::new(config)),
            recovery_only: false,
        })
    }

    pub fn recovery(store: Store) -> Self {
        Self {
            studio: Arc::new(Studio::new(store)),
            config: Arc::new(RwLock::new(AppConfig::default())),
            recovery_only: true,
        }
    }

    pub fn ensure_writable(&self) -> Result<()> {
        if self.recovery_only {
            return Err(skill_studio_core::Error::invalid(
                "配置尚未正常加载，请先恢复配置备份并重启应用",
            ));
        }
        Ok(())
    }

    pub fn studio(&self) -> &Studio {
        &self.studio
    }

    /// 读配置。锁被毒化时恢复而不 panic（cc-switch 的惯例）。
    pub fn config(&self) -> RwLockReadGuard<'_, AppConfig> {
        self.config.read().unwrap_or_else(|poisoned| {
            log::warn!("配置读锁被毒化，已恢复");
            poisoned.into_inner()
        })
    }

    fn config_mut(&self) -> RwLockWriteGuard<'_, AppConfig> {
        self.config.write().unwrap_or_else(|poisoned| {
            log::warn!("配置写锁被毒化，已恢复");
            poisoned.into_inner()
        })
    }

    /// 在写锁内改配置，成功后立刻落盘。
    ///
    /// 落盘失败会把内存里的改动一起报错返回 —— 让用户知道这次改动没有持久化，
    /// 而不是界面看起来成功、重启后消失。
    pub fn mutate<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Studio, &mut AppConfig) -> Result<T>,
    {
        self.ensure_writable()?;
        let mut guard = self.config_mut();
        let mut next = guard.clone();
        let outcome = f(&self.studio, &mut next)?;
        self.studio.save_config(&next)?;
        *guard = next;
        Ok(outcome)
    }

    /// Core operation owns the filesystem/config transaction and updates memory
    /// only after commit. Do not follow it with a second, fallible config save.
    pub(crate) fn transactional<T>(
        &self,
        f: impl FnOnce(&Studio, &mut AppConfig) -> Result<T>,
    ) -> Result<T> {
        self.ensure_writable()?;
        f(&self.studio, &mut self.config_mut())
    }

    pub fn register_skills(
        &self,
        ids: &[String],
        agents: &[String],
        mode: Option<skill_studio_core::models::skill::LinkMode>,
        force: bool,
    ) -> Result<skill_studio_core::models::skill::LinkReport> {
        self.ensure_writable()?;
        self.studio
            .register(&mut self.config_mut(), ids, agents, mode, force)
    }

    pub fn write_project(
        &self,
        id: &str,
        selection: Option<skill_studio_core::models::project::ProjectSelection>,
    ) -> Result<skill_studio_core::models::skill::LinkReport> {
        self.ensure_writable()?;
        let mut guard = self.config_mut();
        self.studio.write_project(&mut guard, id, selection)
    }

    pub fn set_project_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<skill_studio_core::models::skill::LinkReport> {
        self.ensure_writable()?;
        self.studio
            .set_project_enabled(&mut self.config_mut(), id, enabled)
    }

    /// Restore under the same write lock as every other configuration mutation.
    pub fn restore_backup(&self, path: &std::path::Path) -> Result<AppConfig> {
        let mut guard = self.config_mut();
        if !guard.active_groups.is_empty() || !guard.policy_suspensions.is_empty() {
            return Err(skill_studio_core::Error::invalid(
                "请先开启保留手动 skill 并停用所有分组，再恢复配置备份",
            ));
        }
        let restored = self.studio.store().read_backup(path)?;
        let mcp_enabled = if self.recovery_only {
            !skill_studio_mcp::management::read(&self.studio.store().dir().join("mcp"))
                .map_err(|error| skill_studio_core::Error::invalid(error.to_string()))?
                .suspended
        } else {
            guard.settings.manage_mcp
        };
        if restored.settings.manage_mcp != mcp_enabled {
            return Err(skill_studio_core::Error::invalid(
                "备份中的 MCP 管理状态与当前接入不一致，请先通过设置开关切换，再恢复此备份",
            ));
        }
        if !restored.active_groups.is_empty() || !restored.policy_suspensions.is_empty() {
            return Err(skill_studio_core::Error::invalid(
                "此备份包含运行中的分组，不能只恢复配置；请选择停用分组后生成的备份",
            ));
        }
        self.studio.save_config(&restored)?;
        *guard = restored.clone();
        Ok(restored)
    }

    /// Hub adoption persists its own filesystem/config transaction under the write lock.
    pub fn adopt_to_hub(&self, skill_id: &str) -> Result<skill_studio_core::models::skill::Skill> {
        self.ensure_writable()?;
        let mut guard = self.config_mut();
        self.studio.adopt_to_hub(&mut guard, skill_id)
    }

    pub fn install_catalog_skill(
        &self,
        prepared: &skill_studio_core::services::marketplace::PreparedSkill,
    ) -> Result<skill_studio_core::models::skill::Skill> {
        self.ensure_writable()?;
        let mut guard = self.config_mut();
        self.studio.install_catalog_skill(&mut guard, prepared)
    }

    pub fn release_from_hub(
        &self,
        skill_id: &str,
    ) -> Result<skill_studio_core::models::skill::Skill> {
        self.ensure_writable()?;
        let mut guard = self.config_mut();
        self.studio.release_from_hub(&mut guard, skill_id)
    }

    pub fn set_manual_skill_policy(&self, preserve: bool) -> Result<()> {
        self.ensure_writable()?;
        let mut guard = self.config_mut();
        let mut next = guard.clone();
        next.settings.preserve_manual_skills = preserve;
        self.studio.reconcile_manual_policy(&mut next, true)?;
        *guard = next;
        Ok(())
    }

    pub fn set_mcp_management(&self, enabled: bool) -> Result<()> {
        self.ensure_writable()?;
        let mut guard = self.config_mut();
        if guard.settings.manage_mcp == enabled {
            return Ok(());
        }
        let dir = self.studio.store().dir().join("mcp");
        skill_studio_mcp::management::set_enabled(&dir, enabled).map_err(|error| {
            skill_studio_core::Error::invalid(format!(
                "无法{} MCP 管理：{error:#}。开关保持原状态，请核对配置文件后重试",
                if enabled { "开启" } else { "关闭" }
            ))
        })?;
        let mut next = guard.clone();
        next.settings.manage_mcp = enabled;
        if let Err(error) = self.studio.save_config(&next) {
            if let Err(rollback) = skill_studio_mcp::management::set_enabled(&dir, !enabled) {
                return Err(skill_studio_core::Error::invalid(format!(
                    "保存 MCP 管理开关失败：{error}；回滚 MCP 配置也失败：{rollback}"
                )));
            }
            return Err(error);
        }
        *guard = next;
        Ok(())
    }

    pub fn scan_with_policy(&self) -> Result<Vec<skill_studio_core::services::studio::SkillView>> {
        self.ensure_writable()?;
        let mut guard = self.config_mut();
        self.studio.reconcile_manual_policy(&mut guard, false)?;
        self.studio.scan_skills(&guard)
    }

    pub fn set_managed_agents(&self, disabled: &[String]) -> Result<()> {
        self.ensure_writable()?;
        use std::collections::HashSet;
        let mut guard = self.config_mut();
        for id in disabled {
            skill_studio_core::models::agent::require_agent(id)?;
        }
        let old: HashSet<_> = guard.settings.disabled_agents.iter().cloned().collect();
        let new: HashSet<_> = disabled.iter().cloned().collect();
        let changed: Vec<_> = old.symmetric_difference(&new).cloned().collect();
        if changed.len() > 1 {
            return Err(skill_studio_core::Error::invalid("请逐个切换应用管理状态"));
        }
        if let Some(id) = changed.first() {
            self.studio
                .set_agent_management(&mut guard, id, !new.contains(id))?;
        }
        Ok(())
    }

    /// 设置这个 agent 当前叠加的全部分组；空列表表示全部停用
    pub fn activate_agent_groups(&self, agent_id: &str, group_ids: &[String]) -> Result<()> {
        self.ensure_writable()?;
        let mut guard = self.config_mut();
        self.studio
            .activate_agent_groups(&mut guard, agent_id, group_ids)
    }

    /// Native settings writes share the configuration write lock to prevent two
    /// toggles from replacing each other's read/modify/write results.
    pub fn with_exclusive<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Studio, &AppConfig) -> Result<T>,
    {
        self.ensure_writable()?;
        let guard = self.config_mut();
        f(&self.studio, &guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skill_studio_core::models::group::ActiveGroup;

    fn interrupted_mcp_toggle(enabled: bool) {
        use serde_json::json;
        use skill_studio_mcp::management::{self, Entry, Target};

        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path().join("studio"));
        let state = AppState::bootstrap(store.clone()).unwrap();
        state
            .mutate(|_, config| {
                config.settings.language = "en".into();
                Ok(())
            })
            .unwrap();
        let dir = store.dir().join("mcp");
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            "model='keep'\n[mcp_servers.sample]\ncommand='original'\n",
        )
        .unwrap();
        management::save(
            &dir,
            Entry {
                id: "sample".into(),
                name: "sample".into(),
                mode: "direct".into(),
                definition: json!({"type":"stdio","command":"managed"}),
                oauth: false,
                client_id: None,
                scopes: vec![],
                bindings: vec![],
            },
            vec![Target {
                agent: "codex".into(),
                path: path.clone(),
                project: None,
                key: "sample".into(),
                expected: Some(json!({"command":"original"})),
            }],
            std::path::Path::new("/app"),
            None,
        )
        .unwrap();
        if enabled {
            state.set_mcp_management(false).unwrap();
        }
        // Simulate termination after the MCP transaction commits but before
        // set_mcp_management saves the application setting.
        management::set_enabled(&dir, enabled).unwrap();
        assert_ne!(
            state.studio().load_config().unwrap().settings.manage_mcp,
            enabled
        );
        let native_after = std::fs::read(&path).unwrap();
        drop(state);

        let restarted = AppState::bootstrap(store.clone()).unwrap();
        assert_eq!(restarted.config().settings.manage_mcp, enabled);
        assert_eq!(store.load().unwrap().settings.manage_mcp, enabled);
        assert_eq!(restarted.config().settings.language, "en");
        assert_eq!(std::fs::read(&path).unwrap(), native_after);
        // The recovered state must still allow the opposite operation.
        restarted.set_mcp_management(!enabled).unwrap();
        assert_eq!(management::read(&dir).unwrap().suspended, enabled);
    }

    #[test]
    fn bootstrap_recovers_interrupted_mcp_disable() {
        interrupted_mcp_toggle(false);
    }

    #[test]
    fn bootstrap_recovers_interrupted_mcp_enable() {
        interrupted_mcp_toggle(true);
    }

    #[test]
    fn bootstrap_recovers_mcp_journal_before_reading_toggle_state() {
        use serde_json::json;
        use skill_studio_mcp::management::Catalog;

        for committed in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let store = Store::new(temp.path().join("studio"));
            store.save(&AppConfig::default()).unwrap();
            let dir = store.dir().join("mcp");
            std::fs::create_dir_all(&dir).unwrap();
            let catalog_path = dir.join("catalog.json");
            let before = serde_json::to_string(&Catalog::default()).unwrap();
            let after = serde_json::to_string(&Catalog {
                suspended: true,
                ..Default::default()
            })
            .unwrap();
            let native = temp.path().join("config.toml");
            std::fs::write(&native, "# original").unwrap();
            std::fs::write(&catalog_path, if committed { &after } else { &before }).unwrap();
            let journal = dir.join("management-transaction.json");
            std::fs::write(
                &journal,
                serde_json::to_vec(&json!({
                    "changes": [{"path":native, "before":"# managed", "after":"# original"}],
                    "catalog": {"path":catalog_path, "before":before, "after":after}
                }))
                .unwrap(),
            )
            .unwrap();

            let state = AppState::bootstrap(store.clone()).unwrap();
            assert_eq!(state.config().settings.manage_mcp, !committed);
            assert_eq!(store.load().unwrap().settings.manage_mcp, !committed);
            assert_eq!(
                std::fs::read_to_string(native).unwrap(),
                if committed { "# original" } else { "# managed" }
            );
            assert!(!journal.exists());
        }
    }

    #[test]
    fn bootstrap_reconciles_mcp_state_without_overwriting_external_edits() {
        use serde_json::json;
        use skill_studio_mcp::management;

        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path().join("studio"));
        store.save(&AppConfig::default()).unwrap();
        let dir = store.dir().join("mcp");
        management::set_enabled(&dir, false).unwrap();
        let path = temp.path().join("config.toml");
        let external = "[mcp_servers.sample]\ncommand='edited-while-stopped'\n";
        std::fs::write(&path, external).unwrap();
        let mut catalog = management::read(&dir).unwrap();
        catalog.entries.push(management::Entry {
            id: "sample".into(),
            name: "sample".into(),
            mode: "direct".into(),
            definition: json!({"type":"stdio","command":"managed"}),
            oauth: false,
            client_id: None,
            scopes: vec![],
            bindings: vec![management::Binding {
                id: "binding".into(),
                agent: "codex".into(),
                path: path.clone(),
                project: None,
                key: "sample".into(),
                original: Some(json!({"command":"original"})),
                installed: json!({"command":"managed"}),
            }],
        });
        std::fs::write(
            dir.join("catalog.json"),
            serde_json::to_vec(&catalog).unwrap(),
        )
        .unwrap();

        let state = AppState::bootstrap(store).unwrap();
        assert!(!state.config().settings.manage_mcp);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), external);
        assert!(state.set_mcp_management(true).is_err());
        assert!(!state.config().settings.manage_mcp);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), external);
    }

    #[test]
    fn backup_validation_preserves_memory_and_disk_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let state = AppState::bootstrap(store).unwrap();
        state
            .mutate(|_, c| {
                c.settings.language = "original".into();
                Ok(())
            })
            .unwrap();
        let before = std::fs::read(state.studio().store().config_path()).unwrap();
        let backup = dir.path().join("restore.json");
        let mut restored = state.config().clone();
        restored.version = u32::MAX;
        std::fs::write(&backup, serde_json::to_vec(&restored).unwrap()).unwrap();
        assert!(state.restore_backup(&backup).is_err());
        restored.version = state.config().version;
        restored.active_groups.insert(
            "codex".into(),
            vec![ActiveGroup {
                group_id: "group".into(),
                skill_ids: vec![],
                entries: vec![],
                preserve_manual_skills: true,
            }],
        );
        std::fs::write(&backup, serde_json::to_vec(&restored).unwrap()).unwrap();
        assert!(state.restore_backup(&backup).is_err());
        assert_eq!(
            std::fs::read(state.studio().store().config_path()).unwrap(),
            before
        );
        assert_eq!(state.config().settings.language, "original");
        restored.active_groups.clear();
        restored.settings.language = "restored".into();
        std::fs::write(&backup, serde_json::to_vec(&restored).unwrap()).unwrap();
        state.restore_backup(&backup).unwrap();
        assert_eq!(state.config().settings.language, "restored");
        assert_eq!(
            state.studio().load_config().unwrap().settings.language,
            "restored"
        );
    }
}

#[cfg(test)]
mod audit_regressions {
    use super::*;
    #[test]
    fn recovery_keeps_real_backups_and_blocks_management() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().into());
        store.save(&AppConfig::default()).unwrap();
        store.save(&AppConfig::default()).unwrap();
        std::fs::write(store.config_path(), "broken").unwrap();
        assert!(AppState::bootstrap(store.clone()).is_err());
        let state = AppState::recovery(store.clone());
        assert_eq!(state.studio().store().dir(), store.dir());
        let backups = state.studio().store().list_backups();
        assert!(!backups.is_empty());
        assert!(state.mutate(|_, _| Ok(())).is_err());
        assert!(state.scan_with_policy().is_err());
        state.restore_backup(&backups[0]).unwrap();
        assert!(AppState::bootstrap(store).is_ok());
        assert!(!dir.path().join(".recovery-scratch").exists());
    }
}
