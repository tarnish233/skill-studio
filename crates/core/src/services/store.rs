//! `AppConfig` 的读写。原子写入 + 备份轮转。
//!
//! 分组和项目绑定是用户手工攒出来的，丢了很痛，所以每次写入前先把旧文件轮转到
//! `backups/`，并且写入本身走 `atomic_write`（tmp + fsync + rename）。

use std::path::{Path, PathBuf};

use super::transaction::Transaction;
use crate::error::{Error, Result};
use crate::fs::{atomic, paths};
use crate::models::config::{AppConfig, CONFIG_VERSION};

/// 备份文件名前缀
const BACKUP_PREFIX: &str = "config-";

/// 配置读写的落点。测试里指向临时目录，运行时指向 `~/.skill-studio`。
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Default for Store {
    fn default() -> Self {
        Self::new(paths::config_dir())
    }
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn config_path(&self) -> PathBuf {
        self.dir.join("config.json")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.dir.join("backups")
    }

    /// Hub 目录：默认 `<dir>/skills`，可被设置覆盖。
    pub fn hub_dir(&self, config: &AppConfig) -> PathBuf {
        config
            .settings
            .hub_dir
            .clone()
            .unwrap_or_else(|| self.dir.join("skills"))
    }

    /// 读配置。文件不存在时返回默认值（首次启动）。
    ///
    /// 解析失败**不**静默回落到默认值 —— 那会让用户的分组凭空消失。
    /// 直接报错，由上层提示用户去 `backups/` 找回。
    pub fn load(&self) -> Result<AppConfig> {
        super::transaction::recover(&self.dir.join("registration.json"))?;
        super::transaction::recover(&self.dir.join("migration.json"))?;
        super::transaction::recover(&self.dir.join("skill-files.json"))?;
        super::transaction::recover(&self.dir.join("group-switch.json"))?;
        super::transaction::recover(&self.dir.join("skill-unregister.json"))?;
        super::transaction::recover(&self.dir.join("manual-policy.json"))?;
        super::transaction::recover(&self.dir.join("project-write.json"))?;
        let path = self.config_path();
        match atomic::read_json_file::<AppConfig>(&path)? {
            Some(mut config) => {
                self.migrate(&mut config)?;
                Ok(config)
            }
            None => Ok(AppConfig::default()),
        }
    }

    /// v2 added optional Agent ownership and active combinations; v4 dropped that
    /// ownership again so a group can be independently active per agent. Both are
    /// plain field changes with sane defaults, so old JSON just loads with the new
    /// shape and the version number is bumped for the record.
    fn migrate(&self, config: &mut AppConfig) -> Result<()> {
        if config.version > CONFIG_VERSION {
            return Err(Error::config(format!(
                "配置版本 {} 高于本程序支持的 {}，请升级 Skill Studio；\
                 强行写入会损坏配置",
                config.version, CONFIG_VERSION
            )));
        }
        if config.version < CONFIG_VERSION {
            log::info!(
                "配置版本 {} -> {}，执行迁移",
                config.version,
                CONFIG_VERSION
            );
            config.version = CONFIG_VERSION;
        }
        Ok(())
    }

    /// 写配置：先轮转备份，再原子写入。
    pub fn save(&self, config: &AppConfig) -> Result<()> {
        self.rotate_backup(config.settings.backup_keep)?;
        atomic::write_json_file(&self.config_path(), config)
    }

    /// 把 config.json 纳入 `tx` 的回滚日志，并在它被移走之前先轮转一份备份。
    ///
    /// [`Self::save`] 的备份是从 config.json 的**当前内容**复制出来的，而
    /// `Transaction::reserve` 会把目标 rename 到兄弟位置。所以"先 reserve 再 save"时
    /// `rotate_backup` 会 stat 不到文件、直接静默跳过 —— 分组切换、Hub 托管/还原、项目
    /// 写入这些最该能回退的操作，恰好一份备份都不留。顺序必须是先备份、再 reserve。
    pub fn reserve_config(&self, tx: &mut Transaction, config: &AppConfig) -> Result<()> {
        if self.config_path().exists() && !self.config_path().is_file() {
            return Err(Error::config("配置路径不是普通文件，拒绝替换"));
        }
        self.rotate_backup(config.settings.backup_keep)?;
        tx.reserve(&self.config_path())
    }

    /// 把当前 config.json 复制一份到 backups/，并只保留最近 `keep` 份。
    /// 备份失败不阻塞主流程 —— 保住写入比保住备份重要。
    fn rotate_backup(&self, keep: usize) -> Result<()> {
        if keep == 0 {
            // 保留 0 份：不再备份，已有的也不留（与 MCP 配置备份共用这个份数）
            self.prune_backups(0);
            return Ok(());
        }
        let current = self.config_path();
        if !current.is_file() {
            return Ok(());
        }
        let backups = self.backups_dir();
        if let Err(e) = std::fs::create_dir_all(&backups) {
            log::warn!("创建备份目录失败，跳过备份: {e}");
            return Ok(());
        }
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
        let target = backups.join(format!("{BACKUP_PREFIX}{stamp}.json"));
        if let Err(e) = std::fs::copy(&current, &target) {
            log::warn!("写备份失败，跳过: {e}");
            return Ok(());
        }
        self.prune_backups(keep);
        Ok(())
    }

    fn prune_backups(&self, keep: usize) {
        let Ok(entries) = std::fs::read_dir(self.backups_dir()) else {
            return;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(BACKUP_PREFIX) && n.ends_with(".json"))
            })
            .collect();
        // 文件名里的时间戳是定长的，字典序等于时间序
        files.sort();
        while files.len() > keep {
            let oldest = files.remove(0);
            let _ = std::fs::remove_file(oldest);
        }
    }

    /// 列出备份，最新在前
    pub fn list_backups(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.backups_dir()) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(BACKUP_PREFIX) && n.ends_with(".json"))
            })
            .collect();
        files.sort();
        files.reverse();
        files
    }

    /// 删除一份配置备份。只接受 `backups/` 下的 `config-*.json`。
    pub fn delete_backup(&self, backup: &Path) -> Result<()> {
        let named = backup
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(BACKUP_PREFIX) && n.ends_with(".json"));
        let inside = backup
            .parent()
            .is_some_and(|p| paths::paths_alias(p, &self.backups_dir()));
        if !named || !inside || backup.symlink_metadata().is_ok_and(|m| !m.is_file()) {
            return Err(Error::invalid("只能删除配置备份"));
        }
        match std::fs::remove_file(backup) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::io(backup, e)),
        }
    }

    pub fn read_backup(&self, backup: &Path) -> Result<AppConfig> {
        let mut restored: AppConfig = atomic::read_json_file(backup)?
            .ok_or_else(|| Error::NotFound(backup.display().to_string()))?;
        self.migrate(&mut restored)?;
        Ok(restored)
    }

    /// 从备份恢复。恢复前会把当前配置再备份一次，避免误操作不可逆。
    pub fn restore_backup(&self, backup: &Path) -> Result<AppConfig> {
        let restored = self.read_backup(backup)?;
        self.save(&restored)?;
        Ok(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::group::Group;

    fn store() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let s = Store::new(tmp.path().to_path_buf());
        (tmp, s)
    }

    #[test]
    fn load_returns_default_when_file_missing() {
        let (_t, s) = store();
        let cfg = s.load().unwrap();
        assert_eq!(cfg.version, CONFIG_VERSION);
        assert!(cfg.groups.is_empty());
    }

    #[test]
    fn save_then_load_roundtrips() {
        let (_t, s) = store();
        let mut cfg = AppConfig::default();
        cfg.groups.push(Group::new("g1".into(), "前端".into()));
        s.save(&cfg).unwrap();

        let back = s.load().unwrap();
        assert_eq!(back.groups.len(), 1);
        assert_eq!(back.groups[0].name, "前端");
    }

    #[test]
    fn corrupt_config_errors_instead_of_silently_resetting() {
        // 静默回落到默认值会让用户的分组凭空消失，必须报错
        let (_t, s) = store();
        std::fs::write(s.config_path(), "{ this is not json").unwrap();
        let err = s.load().unwrap_err().to_string();
        assert!(err.contains("config.json"), "{err}");
    }

    #[test]
    fn newer_config_version_is_refused() {
        let (_t, s) = store();
        std::fs::write(
            s.config_path(),
            format!(r#"{{"version": {}}}"#, CONFIG_VERSION + 5),
        )
        .unwrap();
        let err = s.load().unwrap_err().to_string();
        assert!(err.contains("高于"), "{err}");
    }

    #[test]
    fn save_rotates_backups_and_respects_keep() {
        let (_t, s) = store();
        let mut cfg = AppConfig::default();
        cfg.settings.backup_keep = 3;

        // 第一次写没有旧文件可备份
        s.save(&cfg).unwrap();
        assert!(s.list_backups().is_empty());

        for i in 0..6 {
            cfg.groups = vec![Group::new(format!("g{i}"), format!("组{i}"))];
            s.save(&cfg).unwrap();
        }
        let backups = s.list_backups();
        assert_eq!(backups.len(), 3, "只保留最近 3 份，实际 {}", backups.len());
    }

    /// 事务性写入同样必须留下备份。`reserve` 会把 config.json rename 到兄弟位置，
    /// 所以"先 reserve 再 save"时 `rotate_backup` 会 stat 不到文件而静默跳过 ——
    /// 而分组切换、Hub 托管/还原、项目写入走的正是这条路径。
    #[test]
    fn reserving_config_in_a_transaction_still_leaves_a_backup() {
        let (_t, s) = store();
        let mut cfg = AppConfig::default();
        cfg.settings.backup_keep = 3;
        cfg.groups.push(Group::new("g1".into(), "旧".into()));
        s.save(&cfg).unwrap();
        assert!(s.list_backups().is_empty(), "首次写入没有旧文件可备份");

        let mut next = cfg.clone();
        next.groups = vec![Group::new("g2".into(), "新".into())];
        let mut tx = Transaction::begin(s.dir().join("group-switch.json")).unwrap();
        s.reserve_config(&mut tx, &next).unwrap();
        s.save(&next).unwrap();
        tx.commit().unwrap();

        let backups = s.list_backups();
        assert_eq!(backups.len(), 1, "事务性写入必须留下一份备份");
        assert_eq!(s.restore_backup(&backups[0]).unwrap().groups[0].name, "旧");
    }

    #[test]
    fn lowering_the_keep_count_removes_older_backups_down_to_zero() {
        let (_t, s) = store();
        let mut cfg = AppConfig::default();
        for _ in 0..3 {
            s.save(&cfg).unwrap();
            // 备份文件名精确到毫秒
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(s.list_backups().len(), 2);
        cfg.settings.backup_keep = 1;
        s.save(&cfg).unwrap();
        assert_eq!(s.list_backups().len(), 1);
        cfg.settings.backup_keep = 0;
        s.save(&cfg).unwrap();
        assert!(s.list_backups().is_empty(), "保留 0 份：已有的备份也不留");
    }

    #[test]
    fn deleting_a_backup_accepts_only_config_backups() {
        let (t, s) = store();
        let cfg = AppConfig::default();
        s.save(&cfg).unwrap();
        s.save(&cfg).unwrap();
        let backups = s.list_backups();
        assert_eq!(backups.len(), 1);
        for other in [
            s.config_path(),
            t.path().join("config-elsewhere.json"),
            s.backups_dir().join("notes.json"),
        ] {
            std::fs::write(&other, "{}").unwrap();
            assert!(s.delete_backup(&other).is_err(), "{}", other.display());
            assert!(other.exists());
        }
        s.delete_backup(&backups[0]).unwrap();
        assert!(s.list_backups().is_empty());
        assert!(s.config_path().exists());
    }

    #[test]
    fn backup_keep_zero_disables_backups() {
        let (_t, s) = store();
        let mut cfg = AppConfig::default();
        cfg.settings.backup_keep = 0;
        s.save(&cfg).unwrap();
        s.save(&cfg).unwrap();
        assert!(s.list_backups().is_empty());
    }

    #[test]
    fn restore_backup_brings_old_state_back() {
        let (_t, s) = store();
        let mut cfg = AppConfig::default();
        cfg.groups.push(Group::new("g1".into(), "旧".into()));
        s.save(&cfg).unwrap();

        // 改成新状态，旧状态进备份
        cfg.groups = vec![Group::new("g2".into(), "新".into())];
        s.save(&cfg).unwrap();

        let backups = s.list_backups();
        assert_eq!(backups.len(), 1);
        let restored = s.restore_backup(&backups[0]).unwrap();
        assert_eq!(restored.groups[0].name, "旧");
        assert_eq!(s.load().unwrap().groups[0].name, "旧");
    }

    #[test]
    fn restore_missing_backup_errors() {
        let (_t, s) = store();
        let err = s
            .restore_backup(Path::new("/nope/backup.json"))
            .unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "{err}");
    }

    #[test]
    fn hub_dir_defaults_under_store_and_honors_override() {
        let (t, s) = store();
        let cfg = AppConfig::default();
        assert_eq!(s.hub_dir(&cfg), t.path().join("skills"));

        let mut cfg2 = AppConfig::default();
        cfg2.settings.hub_dir = Some(PathBuf::from("/custom/hub"));
        assert_eq!(s.hub_dir(&cfg2), PathBuf::from("/custom/hub"));
    }
}
