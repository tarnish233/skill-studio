//! MCP 配置备份：集中放在 Studio 目录，按保留份数轮转，并判断每一份还能不能恢复。
//!
//! 每次写入 Agent 配置前，`commit` 把涉及的配置文件和托管状态存进 `<mcp>/backups/<id>/`。
//! 旧版本把文件备份写在配置文件旁边（`config.studio-backup-<id>-<序号>`），托管状态写在
//! `<mcp>/catalog.studio-backup-<id>.json`；删除这些旧记录时只按当时的命名清理，记录被改成
//! 指向别处时不跟着删。
use super::{catalog_state_hash, digest, lock, read_text, read_unlocked, Backup, Catalog};
use anyhow::{bail, Result};
use serde::Serialize;
use skill_studio_core::fs::atomic;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// `<mcp>/backups`：每次写入一个 `<id>/` 子目录
pub const DIR: &str = "backups";
/// 旧网关接入写入前的备份，每个配置文件只留最近一份
pub const GATEWAY: &str = "gateway";
/// 备份目录里的托管状态
pub(super) const CATALOG: &str = "catalog.json";
const AFTER_NEWER: &str = "需先恢复更新的备份";
const NEWER_LOST: &str = "更新的备份已无法恢复";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// 现在就能恢复
    Restorable,
    /// 之后的写入改过同一份配置：先按顺序恢复更新的备份才能恢复它
    AfterNewer,
    /// 备份已损坏，或配置之后又被改过，沿撤销链回不去了
    Unrestorable,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupView {
    pub id: String,
    pub created_at: u64,
    pub paths: Vec<PathBuf>,
    /// 备份文件占用的字节数
    pub size: u64,
    pub status: Status,
    pub reason: Option<String>,
}

/// 配置文件旁边、不属于任何备份记录的 `*.studio-backup-*`：旧版网关接入写入时留下的，
/// 或记录已经丢失的。无法从界面恢复，只在用户要求时删除。
#[derive(Debug, Serialize)]
pub struct LegacyBackup {
    pub path: PathBuf,
    pub size: u64,
}

pub enum Selection {
    Ids(Vec<String>),
    Unrestorable,
    All,
}

/// `<mcp>/backups/<id>`：一次写入的备份目录；`id` 为 [`GATEWAY`] 时是旧网关接入的备份目录
pub(super) fn folder(dir: &Path, id: &str) -> PathBuf {
    dir.join(DIR).join(id)
}

/// 备份里配置文件的名字。带上序号，两个同名配置（如不同项目的 `mcp.json`）不会相撞。
pub(super) fn file_name(index: usize, path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".into());
    format!("{index}-{name}")
}

/// 建好备份目录。配置里可能有凭据（headers、env），目录只给自己访问。
pub(super) fn create_folder(dir: &Path, id: &str) -> Result<()> {
    private_dir(&dir.join(DIR))?;
    private_dir(&folder(dir, id))
}

fn private_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// 旧网关接入写入 `path` 前的备份位置。同一个配置文件固定一个名字，只留最近一份。
pub fn gateway_backup(dir: &Path, agent: &str, path: &Path) -> Result<PathBuf> {
    create_folder(dir, GATEWAY)?;
    let agent: String = agent
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let key = digest(&path.to_string_lossy());
    Ok(folder(dir, GATEWAY).join(format!("{agent}-{name}-{}", &key[..12])))
}

/// 所有备份，最新的在前
pub fn list(dir: &Path) -> Result<Vec<BackupView>> {
    let _lock = lock(dir)?;
    views(&read_unlocked(dir)?)
}

/// 删除选中的备份，返回删掉的记录数。「无法恢复」按删除这一刻的状态判断。
pub fn delete(dir: &Path, selection: &Selection) -> Result<usize> {
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    let doomed: HashSet<String> = match selection {
        Selection::Ids(ids) => ids.iter().cloned().collect(),
        Selection::All => catalog.backups.iter().map(|b| b.id.clone()).collect(),
        Selection::Unrestorable => views(&catalog)?
            .into_iter()
            .filter(|v| v.status == Status::Unrestorable)
            .map(|v| v.id)
            .collect(),
    };
    let (removed, kept) = std::mem::take(&mut catalog.backups)
        .into_iter()
        .partition(|b| doomed.contains(&b.id));
    catalog.backups = kept;
    discard(dir, &catalog, removed)
}

/// 最多保留 `keep` 份，返回删掉的记录数；顺带清掉没有记录的备份目录。
///
/// 超出时先删已无法恢复的（从旧到新），再删最旧的：只留「最新 N 份」的话，最新的几份
/// 可能恰好都回不去，真正还能撤销的反而被删。无法恢复的备份不在任何一条撤销链上，
/// 删掉它们不会让其余备份变得不可恢复。
pub fn prune(dir: &Path, keep: usize) -> Result<usize> {
    let _lock = lock(dir)?;
    let mut catalog = read_unlocked(dir)?;
    let mut excess = catalog.backups.len().saturating_sub(keep);
    let mut doomed = HashSet::new();
    if excess > 0 {
        let stale = views(&catalog)?
            .into_iter()
            .rev()
            .filter(|v| v.status == Status::Unrestorable)
            .map(|v| v.id);
        let oldest = catalog.backups.iter().map(|b| b.id.clone());
        for id in stale.chain(oldest) {
            if excess == 0 {
                break;
            }
            if doomed.insert(id) {
                excess -= 1;
            }
        }
    }
    let (removed, kept) = std::mem::take(&mut catalog.backups)
        .into_iter()
        .partition(|b| doomed.contains(&b.id));
    catalog.backups = kept;
    discard(dir, &catalog, removed)
}

/// 记录先从托管状态里去掉，再删文件：中途失败只会留下没有记录的文件，下次清理时扫掉。
fn discard(dir: &Path, catalog: &Catalog, removed: Vec<Backup>) -> Result<usize> {
    if !removed.is_empty() {
        atomic::atomic_write_private(
            &dir.join("catalog.json"),
            serde_json::to_string_pretty(catalog)?.as_bytes(),
        )?;
    }
    let mut errors = Vec::new();
    for backup in &removed {
        remove_files(dir, backup, &mut errors);
    }
    sweep(dir, catalog, &mut errors);
    if let Some(first) = errors.first() {
        bail!(
            "备份记录已删除，但有 {} 个文件未能删除：{first}",
            errors.len()
        );
    }
    Ok(removed.len())
}

/// 只删 Studio 为这条记录写下的文件
fn remove_files(dir: &Path, backup: &Backup, errors: &mut Vec<String>) {
    if !is_uuid(&backup.id) {
        return;
    }
    let folder = folder(dir, &backup.id);
    if is_dir(&folder) {
        if let Err(e) = std::fs::remove_dir_all(&folder) {
            errors.push(format!("{}：{e}", folder.display()));
        }
    }
    for (index, file) in backup.files.iter().enumerate() {
        let Some(path) = &file.backup_path else {
            continue;
        };
        let legacy = file
            .path
            .with_extension(format!("studio-backup-{}-{index}", backup.id));
        if *path == legacy {
            remove_file(path, errors);
        }
    }
    if backup.catalog_backup_path == dir.join(format!("catalog.studio-backup-{}.json", backup.id)) {
        remove_file(&backup.catalog_backup_path, errors);
    }
}

/// 写入中途退出、或旧版本留在 Studio 目录里的备份，已经没有记录指向它们
fn sweep(dir: &Path, catalog: &Catalog, errors: &mut Vec<String>) {
    let live: HashSet<&str> = catalog.backups.iter().map(|b| b.id.as_str()).collect();
    for entry in std::fs::read_dir(dir.join(DIR))
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = entry.file_name();
        let path = entry.path();
        if name
            .to_str()
            .is_some_and(|n| is_uuid(n) && !live.contains(n))
            && is_dir(&path)
        {
            if let Err(e) = std::fs::remove_dir_all(&path) {
                errors.push(format!("{}：{e}", path.display()));
            }
        }
    }
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let id = name
            .to_str()
            .and_then(|n| n.strip_prefix("catalog.studio-backup-"))
            .and_then(|n| n.strip_suffix(".json"));
        if id.is_some_and(|id| is_uuid(id) && !live.contains(id)) && is_file(&entry.path()) {
            remove_file(&entry.path(), errors);
        }
    }
}

/// 找出配置文件旁边没有记录的旧备份。`configs` 是各 Agent 的配置文件，托管状态里出现过的
/// 配置路径会一并检查。
pub fn legacy(dir: &Path, configs: &[PathBuf]) -> Result<Vec<LegacyBackup>> {
    let _lock = lock(dir)?;
    Ok(find_legacy(&read_unlocked(dir)?, configs))
}

/// 删除 [`legacy`] 找到的文件，返回删掉的个数
pub fn delete_legacy(dir: &Path, configs: &[PathBuf]) -> Result<usize> {
    let _lock = lock(dir)?;
    let found = find_legacy(&read_unlocked(dir)?, configs);
    let mut errors = Vec::new();
    for backup in &found {
        remove_file(&backup.path, &mut errors);
    }
    if let Some(first) = errors.first() {
        bail!("有 {} 个旧备份文件未能删除：{first}", errors.len());
    }
    Ok(found.len())
}

fn find_legacy(catalog: &Catalog, configs: &[PathBuf]) -> Vec<LegacyBackup> {
    let tracked: HashSet<PathBuf> = catalog
        .backups
        .iter()
        .flat_map(|b| &b.files)
        .filter_map(|f| f.backup_path.as_ref())
        .flat_map(|p| [Some(p.clone()), p.canonicalize().ok()])
        .flatten()
        .collect();
    let known = catalog
        .entries
        .iter()
        .flat_map(|e| &e.bindings)
        .chain(catalog.owned_bindings())
        .map(|b| &b.path)
        .chain(
            catalog
                .backups
                .iter()
                .flat_map(|b| &b.files)
                .map(|f| &f.path),
        );
    let places: BTreeSet<(PathBuf, String)> = configs
        .iter()
        .chain(known)
        .filter_map(|config| {
            let stem = config.file_stem()?.to_string_lossy();
            Some((
                config.parent()?.to_path_buf(),
                format!("{stem}.studio-backup-"),
            ))
        })
        .collect();
    let mut found = BTreeMap::new();
    for (parent, prefix) in places {
        for entry in std::fs::read_dir(&parent).into_iter().flatten().flatten() {
            let name = entry.file_name();
            if !name
                .to_str()
                .and_then(|n| n.strip_prefix(&prefix))
                .is_some_and(is_backup_suffix)
            {
                continue;
            }
            let path = entry.path();
            let Ok(meta) = path.symlink_metadata() else {
                continue;
            };
            if !meta.is_file()
                || tracked.contains(&path)
                || path.canonicalize().is_ok_and(|p| tracked.contains(&p))
            {
                continue;
            }
            found.insert(path, meta.len());
        }
    }
    found
        .into_iter()
        .map(|(path, size)| LegacyBackup { path, size })
        .collect()
}

/// `<uuid>`（旧网关接入）或 `<uuid>-<序号>`（旧版托管写入）
fn is_backup_suffix(rest: &str) -> bool {
    match rest.get(36..) {
        Some("") => is_uuid(rest),
        Some(tail) => {
            is_uuid(&rest[..36])
                && tail
                    .strip_prefix('-')
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        }
        None => false,
    }
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn is_dir(path: &Path) -> bool {
    path.symlink_metadata().is_ok_and(|m| m.is_dir())
}

fn is_file(path: &Path) -> bool {
    path.symlink_metadata().is_ok_and(|m| m.is_file())
}

fn remove_file(path: &Path, errors: &mut Vec<String>) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => errors.push(format!("{}：{e}", path.display())),
    }
}

/// 一次写入改动的对象：各个配置文件，以及每次都会记录的托管状态
#[derive(Clone, PartialEq, Eq, Hash)]
enum Resource {
    Catalog,
    File(PathBuf),
}

/// 从最新的备份往回走，模拟「依次恢复」，判断每一份能否恢复。
///
/// 恢复要求配置文件和托管状态都还是这次写入之后的样子（见 [`super::restore_backup`]）。
/// 现在就满足的是「可恢复」；先恢复更新的备份才满足的是「需先恢复更新的备份」；
/// 备份损坏、或配置之后被改过（包括没留备份的写入）就回不去了。
fn views(catalog: &Catalog) -> Result<Vec<BackupView>> {
    let mut actual = HashMap::from([(Resource::Catalog, Some(catalog_state_hash(catalog)?))]);
    let mut simulated = actual.clone();
    let mut blocked: HashMap<Resource, &str> = HashMap::new();
    let mut out = Vec::new();
    for backup in catalog.backups.iter().rev() {
        let changes: Vec<(Resource, Option<String>, Option<String>)> = backup
            .files
            .iter()
            .map(|f| {
                (
                    Resource::File(f.path.clone()),
                    f.before_hash.clone(),
                    f.after_hash.clone(),
                )
            })
            .chain(std::iter::once((
                Resource::Catalog,
                Some(backup.before_catalog_hash.clone()),
                Some(backup.after_catalog_hash.clone()),
            )))
            .collect();
        for (resource, ..) in &changes {
            if let Resource::File(path) = resource {
                if !actual.contains_key(resource) {
                    let current = current_hash(path);
                    actual.insert(resource.clone(), current.clone());
                    simulated.insert(resource.clone(), current);
                }
            }
        }
        let direct = changes.iter().all(|(r, _, after)| &actual[r] == after);
        let chained = changes.iter().all(|(r, _, after)| &simulated[r] == after);
        let damage = damage(backup);
        let (status, reason) = match (&damage, direct, chained) {
            (Some(damage), ..) => (Status::Unrestorable, Some(damage.clone())),
            (None, true, _) => (Status::Restorable, None),
            (None, false, true) => (Status::AfterNewer, Some(AFTER_NEWER.into())),
            (None, false, false) => {
                let (resource, ..) = changes
                    .iter()
                    .find(|(r, _, after)| &simulated[r] != after)
                    .expect("chained 为 false 时至少有一项不一致");
                let reason = blocked
                    .get(resource)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| match resource {
                        Resource::File(path) => format!("配置已在备份后修改：{}", path.display()),
                        Resource::Catalog => "MCP 托管状态已在备份后修改".into(),
                    });
                (Status::Unrestorable, Some(reason))
            }
        };
        if status == Status::Unrestorable {
            // 它改过的对象停在它写入之后的样子：更早的备份要先越过它，越不过去
            for (resource, before, after) in &changes {
                if before != after && &simulated[resource] == after {
                    blocked.insert(resource.clone(), NEWER_LOST);
                }
            }
        } else {
            for (resource, before, _) in &changes {
                simulated.insert(resource.clone(), before.clone());
                blocked.remove(resource);
            }
        }
        out.push(BackupView {
            id: backup.id.clone(),
            created_at: backup.created_at,
            paths: backup.files.iter().map(|f| f.path.clone()).collect(),
            size: size(backup),
            status,
            reason,
        });
    }
    Ok(out)
}

/// 配置文件现在的哈希；读不出来时给一个不会和任何记录相等的值
fn current_hash(path: &Path) -> Option<String> {
    match read_text(path) {
        Ok(text) => text.as_deref().map(digest),
        Err(_) => Some("unreadable".into()),
    }
}

/// 恢复要用到的备份文件是否完好，与 [`super::restore_backup`] 的检查一致
fn damage(backup: &Backup) -> Option<String> {
    match read_text(&backup.catalog_backup_path) {
        Ok(Some(text)) if digest(&text) != backup.before_catalog_hash => {
            return Some("托管状态备份已变化".into())
        }
        Ok(Some(text)) => match serde_json::from_str::<Catalog>(&text) {
            Ok(old) if old.suspended => {
                return Some("备份时 MCP 管理已关闭，请通过设置中的开关恢复".into())
            }
            Ok(_) => {}
            Err(_) => return Some("托管状态备份已损坏".into()),
        },
        Ok(None) => return Some("托管状态备份已不存在".into()),
        Err(_) => return Some("托管状态备份无法读取".into()),
    }
    for file in &backup.files {
        match (&file.backup_path, &file.before_hash) {
            (Some(path), Some(before)) => match read_text(path) {
                Ok(Some(text)) if &digest(&text) == before => {}
                Ok(Some(_)) => return Some("备份文件已变化".into()),
                Ok(None) => return Some("备份文件已不存在".into()),
                Err(_) => return Some("备份文件无法读取".into()),
            },
            (None, None) => {}
            _ => return Some("备份记录无效".into()),
        }
    }
    None
}

fn size(backup: &Backup) -> u64 {
    backup
        .files
        .iter()
        .filter_map(|f| f.backup_path.as_ref())
        .chain(std::iter::once(&backup.catalog_backup_path))
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum()
}

#[cfg(test)]
#[path = "backups_tests.rs"]
mod tests;
