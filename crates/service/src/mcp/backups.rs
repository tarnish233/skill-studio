//! MCP 写入前留下的备份：列出、删除、恢复。

use std::path::PathBuf;

use serde::Serialize;
use skill_studio_mcp::{
    discovery,
    management::{
        self,
        backups::{self, BackupView, LegacyBackup, Selection},
    },
};

use super::{after_write, dir, ensure_enabled, ensure_writable};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct Backups {
    pub backups: Vec<BackupView>,
    /// 旧版本写在配置文件旁边、不在任何备份记录里的文件
    pub legacy: Vec<LegacyBackup>,
}

/// 找旧备份文件时要看的 Agent 配置文件
fn configs(state: &AppState) -> Vec<PathBuf> {
    discovery::scan_files(&state.config())
        .0
        .into_iter()
        .map(|file| file.path)
        .collect()
}

pub fn list(state: &AppState) -> Result<Backups, String> {
    ensure_enabled(state)?;
    let dir = dir(state);
    Ok(Backups {
        backups: if state.is_read_only() {
            backups::list_read_only(&dir)
        } else {
            backups::list(&dir)
        }
        .map_err(|e| e.to_string())?,
        legacy: if state.is_read_only() {
            backups::legacy_read_only(&dir, &configs(state))
        } else {
            backups::legacy(&dir, &configs(state))
        }
        .map_err(|e| e.to_string())?,
    })
}

/// 文件写入持有配置锁后再拿 MCP 锁，复核最新的 CLI/MCP 开关和目录快照。
fn write<T>(
    state: &AppState,
    operation: impl FnOnce(
        &std::path::Path,
        &skill_studio_core::models::config::AppConfig,
    ) -> anyhow::Result<T>,
) -> Result<T, String> {
    super::with_write(state, operation)
}

/// 删除备份：`which` 为 unrestorable（已无法恢复的）、all（全部），否则按 `ids`
pub fn delete(
    state: &AppState,
    which: Option<&str>,
    ids: Option<Vec<String>>,
) -> Result<usize, String> {
    ensure_writable(state)?;
    let selection = match which {
        Some("unrestorable") => Selection::Unrestorable,
        Some("all") => Selection::All,
        Some(other) => return Err(format!("未知的删除范围：{other}")),
        None => Selection::Ids(ids.ok_or("缺少备份 ID")?),
    };
    write(state, |dir, _| backups::delete(dir, &selection))
}

/// 删除旧版本留在配置文件旁边的备份文件
pub fn delete_legacy(state: &AppState) -> Result<usize, String> {
    ensure_writable(state)?;
    write(state, |dir, config| {
        let files = discovery::scan_files(config)
            .0
            .into_iter()
            .map(|file| file.path)
            .collect::<Vec<_>>();
        backups::delete_legacy(dir, &files)
    })
}

/// 恢复一次写入前的样子；之后又改过的配置不覆盖
pub fn restore(state: &AppState, id: &str) -> Result<(), String> {
    ensure_writable(state)?;
    write(state, |dir, _| management::restore_backup(dir, id))?;
    after_write(state);
    Ok(())
}
