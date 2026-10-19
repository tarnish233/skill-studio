//! 纳入托管、移出 Hub 时保存的完整备份：`<store>/skill-backups/<skill id>-<纳秒>/`，
//! 内含 `content/`（skill 全部文件）、`provenance.json`、`snapshot.json` 和当时的配置。
//!
//! 来源记录（`skill_provenance`）引用的那份在退出应用管理、再次移出 Hub 时用来还原，
//! 不能删；其余是历史快照，可以删除。同目录下删除 skill 时的备份（`record.json` + `payload`）
//! 归 [`super::skill_files`] 管，这里不列也不删。
use super::studio::Studio;
use crate::{
    fs::{atomic, paths},
    models::config::AppConfig,
    Error, Result,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubBackup {
    pub id: String,
    pub name: String,
    /// `adopt`（纳入托管）或 `release`（移出 Hub）；早期版本的备份没有记录
    pub operation: Option<String>,
    pub original_path: PathBuf,
    pub created_at: i64,
    pub size: u64,
    /// 来源记录引用的备份：退出应用管理、移出 Hub 时用它还原
    pub in_use: bool,
}

impl Studio {
    /// 所有完整备份，最新的在前
    pub fn hub_backups(&self, config: &AppConfig) -> Result<Vec<HubBackup>> {
        let root = self.store().dir().join("skill-backups");
        let mut found: Vec<(u128, HubBackup)> = Vec::new();
        for entry in fs::read_dir(&root).into_iter().flatten().flatten() {
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if let Some(backup) = read(&entry.path(), &id, config) {
                let order = stamp(&id).unwrap_or(backup.created_at.max(0) as u128 * 1_000_000_000);
                found.push((order, backup));
            }
        }
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        Ok(found.into_iter().map(|(_, backup)| backup).collect())
    }

    /// 删除未被引用的完整备份。有一份不能删时整批都不删。
    pub fn delete_hub_backups(&self, config: &AppConfig, ids: &[String]) -> Result<usize> {
        let root = self.store().dir().join("skill-backups");
        let mut doomed = Vec::new();
        // 同一份列了两次只删一次：第二次会因为「已经不在了」让整批报错
        for id in ids.iter().map(String::as_str).collect::<BTreeSet<_>>() {
            let plain = matches!(
                Path::new(id).components().collect::<Vec<_>>().as_slice(),
                [Component::Normal(_)]
            );
            if !plain || id.starts_with('.') {
                return Err(Error::invalid("无效的备份 ID"));
            }
            let folder = root.join(id);
            let backup = read(&folder, id, config)
                .ok_or_else(|| Error::NotFound(format!("完整备份 {id}")))?;
            if backup.in_use {
                return Err(Error::invalid(format!(
                    "{} 的这份备份仍在使用：退出应用管理、移出 Hub 时要用它还原",
                    backup.name
                )));
            }
            doomed.push(folder);
        }
        for folder in &doomed {
            fs::remove_dir_all(folder).map_err(|e| Error::io(folder, e))?;
        }
        Ok(doomed.len())
    }
}

/// 只认完整备份的形状；按字段读，旧版本写下的格式不同也能列出
fn read(folder: &Path, id: &str, config: &AppConfig) -> Option<HubBackup> {
    if id.starts_with('.') || !folder.symlink_metadata().ok()?.is_dir() {
        return None;
    }
    let provenance: Value = atomic::read_json_file(&folder.join("provenance.json")).ok()??;
    if !folder.join("content").is_dir() {
        return None;
    }
    let snapshot: Option<Value> = atomic::read_json_file(&folder.join("snapshot.json"))
        .ok()
        .flatten();
    let original_path = PathBuf::from(provenance.get("originalPath")?.as_str()?);
    let created_at = snapshot
        .as_ref()
        .and_then(|s| s.get("createdAt")?.as_i64())
        .or_else(|| stamp(id).map(|n| (n / 1_000_000_000) as i64))
        .or_else(|| provenance.get("collectedAt")?.as_i64())
        .unwrap_or_default();
    Some(HubBackup {
        id: id.into(),
        name: original_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| id.into()),
        operation: snapshot
            .as_ref()
            .and_then(|s| s.get("operation")?.as_str().map(str::to_owned)),
        original_path,
        created_at,
        size: dir_size(folder),
        in_use: config
            .skill_provenance
            .values()
            .any(|p| p.backup_path == folder || paths::paths_alias(&p.backup_path, folder)),
    })
}

/// 目录名末尾的纳秒时间戳：同一秒里先后两次操作也能排对顺序
fn stamp(id: &str) -> Option<u128> {
    id.rsplit_once('-')?.1.parse().ok()
}

fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = path.symlink_metadata() else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| dir_size(&entry.path()))
        .sum()
}
