//! `AppConfig` 的读写。原子写入 + 备份轮转。
//!
//! 分组和项目绑定是用户手工攒出来的，丢了很痛，所以每次写入前先把旧文件轮转到
//! `backups/`，并且写入本身走 `atomic_write`（tmp + fsync + rename）。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::transaction::Transaction;
use crate::error::{Error, Result};
use crate::fs::{atomic, paths};
use crate::models::config::{AppConfig, CONFIG_VERSION};

/// 备份文件名前缀
const BACKUP_PREFIX: &str = "config-";

/// 会连带改写 config.json 的事务记录。写到一半退出时留在磁盘上，下次加载时回滚或补完
const JOURNALS: &[&str] = &[
    "registration.json",
    "migration.json",
    "skill-files.json",
    "group-switch.json",
    "skill-unregister.json",
    "manual-policy.json",
    "project-write.json",
    "builtin-skills.json",
];

/// 等不到配置锁时的提示。命令行据此区分「稍后重试即可」与其他失败
pub const CONFIG_BUSY: &str = "另一个程序（命令行或另一个 Skill Studio）正在修改配置，请稍后重试";

/// 保存时发现别处（另一个窗口、命令行）刚改过：不覆盖。命令行据此按最新内容重试。
/// Skill 与 MCP 的分组、项目共用这一句，前面加上「分组」「项目」
pub const CHANGED_ELSEWHERE: &str = "已被别处修改（另一个窗口或命令行），请按最新内容重新修改";

/// 跨进程的配置锁：桌面应用、命令行、远程辅助程序都在改配置前拿独占锁，读的时候拿
/// 共享锁。随文件句柄关闭而释放，进程退出或崩溃也会释放
pub struct ConfigLock {
    _file: std::fs::File,
}

/// 反复尝试加锁直到超时。文件系统不支持文件锁时放行，只记一条警告：宁可失去跨进程
/// 互斥，也不能让所有写入都失败
fn acquire(
    file: &std::fs::File,
    wait: Duration,
    try_lock: fn(&std::fs::File) -> std::io::Result<()>,
) -> bool {
    let contended = fs2::lock_contended_error().raw_os_error();
    let deadline = Instant::now() + wait;
    loop {
        match try_lock(file) {
            Ok(()) => return true,
            Err(e)
                if e.raw_os_error() == contended || e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => {
                log::warn!("配置锁不可用，跳过跨进程互斥: {e}");
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 给一个已打开的锁文件加跨进程锁，最多等 `wait`；等不到返回 false。规则与配置锁相同：
/// 文件系统不支持文件锁时放行。供 MCP 托管状态这类各自有锁文件的数据共用
pub fn lock_file(file: &std::fs::File, wait: Duration, exclusive: bool) -> bool {
    if exclusive {
        acquire(file, wait, fs2::FileExt::try_lock_exclusive)
    } else {
        acquire(file, wait, fs2::FileExt::try_lock_shared)
    }
}

/// 打开一个**已存在**的锁文件再加锁，读的一方从不创建文件：
/// 文件不在或打不开时返回 `Ok(None)`（没有别的进程在用锁）；文件在但等不到时返回
/// `Err(CONFIG_BUSY)`。配置与 MCP 托管状态的只读查看共用这一份
pub fn lock_existing(path: &Path, wait: Duration, exclusive: bool) -> Result<Option<ConfigLock>> {
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .or_else(|_| std::fs::File::open(path))
    else {
        return Ok(None);
    };
    if !lock_file(&file, wait, exclusive) {
        return Err(Error::Other(CONFIG_BUSY.into()));
    }
    Ok(Some(ConfigLock { _file: file }))
}

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
        for journal in JOURNALS {
            super::transaction::recover(&self.dir.join(journal))?;
        }
        Ok(self.read()?.unwrap_or_default())
    }

    /// 只读地读出配置：不恢复中断的事务，也不写任何文件。文件不存在时返回 `None`。
    pub fn read(&self) -> Result<Option<AppConfig>> {
        let Some(mut config) = atomic::read_json_file::<AppConfig>(&self.config_path())? else {
            return Ok(None);
        };
        self.migrate(&mut config)?;
        Ok(Some(config))
    }

    /// 有没有未完成的事务：另一个进程正在写，或上次写到一半退出
    pub fn pending_transactions(&self) -> bool {
        JOURNALS
            .iter()
            .any(|journal| self.dir.join(journal).exists())
    }

    pub fn lock_path(&self) -> PathBuf {
        self.dir.join("config.json.lock")
    }

    /// 改配置前拿独占锁，等别的进程写完，最多等 `wait`。
    pub fn lock_exclusive(&self, wait: Duration) -> Result<ConfigLock> {
        std::fs::create_dir_all(&self.dir).map_err(|e| Error::io(&self.dir, e))?;
        let path = self.lock_path();
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| Error::io(&path, e))?;
        if !acquire(&file, wait, fs2::FileExt::try_lock_exclusive) {
            // 不加「无效输入」前缀：这不是输入的问题，界面和命令行都原样显示这句
            return Err(Error::Other(CONFIG_BUSY.into()));
        }
        Ok(ConfigLock { _file: file })
    }

    /// 读配置时拿共享锁，等正在写的进程写完，最多等 `wait`。只打开已存在的锁文件——
    /// 读的一方从不创建文件；目录只读或等不到时返回 `None`，调用方照常读：写入是原子
    /// 替换，读到的总是完整的一份。
    pub fn lock_shared(&self, wait: Duration) -> Option<ConfigLock> {
        lock_existing(&self.lock_path(), wait, false).ok().flatten()
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

    /// 备份路径必须是 `backups/` 下的普通 `config-*.json` 文件。
    /// 不要求文件存在：删除可以幂等，读取时再报「找不到」。
    fn ensure_backup_path(&self, backup: &Path, action: &str) -> Result<()> {
        let named = backup
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(BACKUP_PREFIX) && n.ends_with(".json"));
        let inside = backup
            .parent()
            .is_some_and(|p| paths::paths_alias(p, &self.backups_dir()));
        if !named || !inside || backup.symlink_metadata().is_ok_and(|m| !m.is_file()) {
            return Err(Error::invalid(format!("只能{action}配置备份")));
        }
        Ok(())
    }

    /// 删除一份配置备份。只接受 `backups/` 下的 `config-*.json`。
    pub fn delete_backup(&self, backup: &Path) -> Result<()> {
        self.ensure_backup_path(backup, "删除")?;
        match std::fs::remove_file(backup) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::io(backup, e)),
        }
    }

    pub fn read_backup(&self, backup: &Path) -> Result<AppConfig> {
        self.ensure_backup_path(backup, "读取")?;
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
    fn reading_a_backup_accepts_only_config_backups() {
        let (t, s) = store();
        let cfg = AppConfig::default();
        s.save(&cfg).unwrap();
        s.save(&cfg).unwrap();
        let backups = s.list_backups();
        assert_eq!(s.read_backup(&backups[0]).unwrap().version, CONFIG_VERSION);

        for other in [
            s.config_path(),
            t.path().join("config-elsewhere.json"),
            s.backups_dir().join("notes.json"),
        ] {
            std::fs::write(&other, "{}").unwrap();
            assert!(s.read_backup(&other).is_err(), "{}", other.display());
        }
        #[cfg(unix)]
        {
            let link = s.backups_dir().join("config-link.json");
            std::os::unix::fs::symlink(s.config_path(), &link).unwrap();
            assert!(s.read_backup(&link).is_err());
            assert!(s.restore_backup(&link).is_err());
        }
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
        // 任意路径先被拦住，不会读配置目录以外的 JSON
        let err = s
            .restore_backup(Path::new("/nope/backup.json"))
            .unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)), "{err}");
        // 合法位置里已经不见的备份仍然是「找不到」
        let missing = s.backups_dir().join("config-19990101-000000.000.json");
        let err = s.restore_backup(&missing).unwrap_err();
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
