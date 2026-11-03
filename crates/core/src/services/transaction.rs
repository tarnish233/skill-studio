//! Write-ahead undo journal for filesystem replacements. Backups stay beside each
//! target so rename never crosses filesystems. Recovery runs before loading config.
use super::scanner::is_symlink_or_junction;
use crate::{
    error::{Error, Result},
    fs::atomic,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Entry {
    target: PathBuf,
    backup: PathBuf,
    existed: bool,
}
#[derive(Serialize, Deserialize)]
pub struct Transaction {
    journal: PathBuf,
    entries: Vec<Entry>,
    committed: bool,
    #[serde(skip)]
    _lock: Option<JournalLock>,
}

pub fn remove(path: &Path) -> Result<()> {
    if is_symlink_or_junction(path) {
        return super::linker::remove_symlink_or_junction(path).map_err(|e| Error::io(path, e));
    }
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::io(path, e)),
    };
    let delete = || {
        if meta.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        }
    };
    match delete() {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            // Only called after ownership/confinement checks. A copied read-only
            // directory needs owner access for unlink; never chmod through a link.
            make_removable(path)?;
            delete().map_err(|e| Error::io(path, e))
        }
        result => result.map_err(|e| Error::io(path, e)),
    }
}

fn make_removable(path: &Path) -> Result<()> {
    if is_symlink_or_junction(path) {
        return Ok(());
    }
    let meta = fs::symlink_metadata(path).map_err(|e| Error::io(path, e))?;
    #[cfg(unix)]
    if meta.is_dir() {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode();
        if mode & 0o700 != 0o700 {
            fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o700))
                .map_err(|e| Error::io(path, e))?;
        }
    }
    #[cfg(windows)]
    if meta.permissions().readonly() {
        let mut permissions = meta.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).map_err(|e| Error::io(path, e))?;
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| Error::io(path, e))? {
            let entry = entry.map_err(|e| Error::io(path, e))?;
            make_removable(&entry.path())?;
        }
    }
    Ok(())
}

const BACKUP_LABEL: &str = "backup";

pub fn sibling(path: &Path, label: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    path.with_file_name(format!(
        ".{}.{}-{}-{nonce}",
        crate::models::skill::skill_id_for(path),
        label,
        std::process::id()
    ))
}

/// Recognize the bounded, hashed backup names used by `reserve`. These are
/// transaction-owned objects, not published backups, even when cleanup is pending.
pub(super) fn is_backup_sibling(path: &Path) -> bool {
    let Some(name) = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix('.'))
    else {
        return false;
    };
    let Some((hash, suffix)) = name.split_once('.') else {
        return false;
    };
    let Some((pid, nonce)) = suffix
        .strip_prefix(BACKUP_LABEL)
        .and_then(|suffix| suffix.strip_prefix('-'))
        .and_then(|suffix| suffix.split_once('-'))
    else {
        return false;
    };
    let decimal = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    hash.len() == 16
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
        && decimal(pid)
        && pid.parse::<u32>().is_ok()
        && decimal(nonce)
        && nonce.parse::<u128>().is_ok()
}

/// Whether `path` sits directly inside `dir`.
///
/// Resolve parents through the filesystem so legitimate `..` components and
/// directory aliases work without allowing symlink-assisted escapes. Do not resolve
/// the final component: recovery operates on the entry, including dangling links.
fn is_direct_child(path: &Path, dir: &Path) -> bool {
    if !matches!(path.components().next_back(), Some(Component::Normal(_))) {
        return false;
    }
    match (
        path.parent().and_then(|p| p.canonicalize().ok()),
        dir.canonicalize(),
    ) {
        (Some(parent), Ok(dir)) => parent == dir,
        _ => false,
    }
}

struct JournalLock(fs::File);

impl Drop for JournalLock {
    fn drop(&mut self) {
        // Closing alone can leave flock held by a descriptor inherited during a
        // concurrent fork. Unlock before close so finished transactions never
        // remain locked until that unrelated child reaches exec or exits.
        if let Err(error) = fs2::FileExt::unlock(&self.0) {
            log::warn!("释放事务锁失败: {error}");
        }
    }
}

// The lock file is intentionally retained: unlinking it would allow two
// processes to lock different inodes while recovering the same journal.
fn lock_journal(journal: &Path) -> Result<JournalLock> {
    if let Some(parent) = journal.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let lock_path = journal.with_extension("json.lock");
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| Error::io(&lock_path, e))?;
    fs2::FileExt::try_lock_exclusive(&file)
        .map_err(|e| Error::io_context("另一个进程正在修改此目录，请稍后重试", e))?;
    Ok(JournalLock(file))
}

impl Transaction {
    pub fn begin(journal: PathBuf) -> Result<Self> {
        let lock = lock_journal(&journal)?;
        if journal.try_exists().map_err(|e| Error::io(&journal, e))? {
            recover_locked(&journal)?;
        }
        let tx = Self {
            journal,
            entries: vec![],
            committed: false,
            _lock: Some(lock),
        };
        tx.persist()?;
        Ok(tx)
    }
    fn persist(&self) -> Result<()> {
        atomic::write_json_file(&self.journal, self)
    }
    /// Reserve a path before changing it; an existing object is moved intact.
    pub fn reserve(&mut self, target: &Path) -> Result<()> {
        let existed = match fs::symlink_metadata(target) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(Error::io(target, e)),
        };
        let backup = sibling(target, BACKUP_LABEL);
        self.entries.push(Entry {
            target: target.to_owned(),
            backup: backup.clone(),
            existed,
        });
        self.persist()?;
        if existed {
            fs::rename(target, &backup).map_err(|e| Error::io(target, e))?;
        }
        Ok(())
    }
    /// Keep a writable copy at the original path while journaling the original,
    /// including its permissions. Callers must use the returned physical path.
    pub fn reserve_file_for_update(&mut self, path: &Path) -> Result<PathBuf> {
        let path = if is_symlink_or_junction(path) {
            path.canonicalize().map_err(|e| Error::io(path, e))?
        } else {
            path.to_owned()
        };
        self.reserve(&path)?;
        let entry = self.entries.last().expect("reserved entry");
        if entry.existed {
            fs::copy(&entry.backup, &path).map_err(|e| Error::io(&path, e))?;
        }
        Ok(path)
    }
    pub fn commit(mut self) -> Result<()> {
        self.committed = true;
        if let Err(err) = self.persist() {
            self.committed = false;
            self.finish()?;
            return Err(err);
        }
        // A committed journal is safe to clean again at the next startup.
        if let Err(e) = self.finish() {
            log::warn!("事务已提交，清理将在启动时重试: {e}");
        }
        Ok(())
    }
    pub fn rollback(self) -> Result<()> {
        self.finish()
    }
    /// The first target or backup that reaches outside `dir`, if any.
    fn escaping_entry(&self, dir: &Path) -> Option<&Path> {
        self.entries
            .iter()
            .flat_map(|e| [e.target.as_path(), e.backup.as_path()])
            .find(|p| !is_direct_child(p, dir))
    }
    fn finish(&self) -> Result<()> {
        let mut errors = Vec::new();
        for entry in self.entries.iter().rev() {
            let result = (|| -> Result<()> {
                if self.committed {
                    remove(&entry.backup)?;
                } else if entry.existed {
                    match fs::symlink_metadata(&entry.backup) {
                        Ok(_) => {
                            remove(&entry.target)?;
                            fs::rename(&entry.backup, &entry.target)
                                .map_err(|e| Error::io(&entry.target, e))?;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(Error::io(&entry.backup, e)),
                    }
                } else {
                    remove(&entry.target)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                errors.push(error.to_string());
            }
        }
        if !errors.is_empty() {
            // Keep the recovery map until every undo/cleanup step has succeeded.
            return Err(Error::Other(format!(
                "事务恢复未完成：{}",
                errors.join("；")
            )));
        }
        fs::remove_file(&self.journal).map_err(|e| Error::io(&self.journal, e))
    }
}

pub fn recover(journal: &Path) -> Result<()> {
    if !journal.try_exists().map_err(|e| Error::io(journal, e))? {
        return Ok(());
    }
    let _lock = lock_journal(journal)?;
    recover_locked(journal)
}

fn recover_locked(journal: &Path) -> Result<()> {
    if let Some(mut tx) = atomic::read_json_file::<Transaction>(journal)? {
        tx.journal = journal.to_path_buf();
        tx.finish()?;
    }
    Ok(())
}

/// Recover a journal that was found by scanning a directory the app does not own.
///
/// `recover` executes absolute paths taken from the journal's *contents*. That is
/// sound for the store's own journals, which are opened by fixed path, but
/// `.skill-studio-replace-*.json` files are different: they live inside user project
/// trees and are gitignored rather than hidden, so they travel with a clone and their
/// contents are untrusted input. A replacement journal only ever reserves `dest` plus
/// its staging and backup siblings, so every path it carries must sit directly in the
/// journal's own directory — which is taken from `journal`, not from the deserialized
/// `journal` field, since that field is untrusted too.
///
/// A journal reaching past that is reported and left on disk rather than executed,
/// and deliberately does not become an `Err`: `load_config` propagates one, so
/// erroring here would trade arbitrary deletion for an app that cannot start.
pub fn recover_confined(journal: &Path) -> Result<()> {
    if !journal.try_exists().map_err(|e| Error::io(journal, e))? {
        return Ok(());
    }
    let Some(dir) = journal.parent() else {
        return Ok(());
    };
    let _lock = lock_journal(journal)?;
    let Some(mut tx) = atomic::read_json_file::<Transaction>(journal)? else {
        return Ok(());
    };
    if let Some(escaping) = tx.escaping_entry(dir) {
        log::warn!(
            "忽略 {}：条目 {} 越出所在目录，不执行恢复",
            journal.display(),
            escaping.display()
        );
        return Ok(());
    }
    tx.journal = journal.to_path_buf();
    tx.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_sibling_recognition_matches_generator_and_rejects_lookalikes() {
        let path = Path::new("store/skill-backups/.tmpfixture");
        assert!(is_backup_sibling(&sibling(path, BACKUP_LABEL)));
        assert!(!is_backup_sibling(&sibling(path, "staging")));
        for name in [
            ".tmpfixture",
            ".tmpfixture.backup-123-456",
            "0123456789abcdef.backup-123-456",
            ".0123456789abcde.backup-123-456",
            ".0123456789abcdeg.backup-123-456",
            ".0123456789abcdef.backup--456",
            ".0123456789abcdef.backup-123-",
            ".0123456789abcdef.backup-+123-456",
            ".0123456789abcdef.backup-123-invalid",
            ".0123456789abcdef.backup-123-456-extra",
            ".0123456789abcdef.backup-4294967296-456",
            ".0123456789abcdef.backup-123-340282366920938463463374607431768211456",
        ] {
            assert!(!is_backup_sibling(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn backup_listing_skips_live_and_interrupted_transactions() {
        use crate::services::{skill_files::SkillBackup, store::Store, studio::Studio};

        for committed in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let studio = Studio::new(Store::new(tmp.path().join("store")));
            let record = SkillBackup {
                id: ".tmpfixture".into(),
                scope: "hub".into(),
                name: "demo".into(),
                original_path: tmp.path().join("demo"),
                deleted_at: 1,
                disabled: false,
                restore_disabled: false,
            };
            let payload = studio.backup_payload(&record);
            let folder = payload.parent().unwrap();
            fs::create_dir_all(&payload).unwrap();
            fs::write(payload.join("SKILL.md"), "original").unwrap();
            atomic::write_json_file(&folder.join("record.json"), &record).unwrap();
            assert_eq!(studio.skill_backups().unwrap()[0].id, record.id);

            let journal = studio.store().dir().join("skill-files.json");
            let mut tx = Transaction::begin(journal.clone()).unwrap();
            tx.reserve(folder).unwrap();
            let reserved = tx.entries[0].backup.clone();
            assert!(reserved.join("payload/SKILL.md").is_file());
            assert!(studio.skill_backups().unwrap().is_empty());

            // Reserved contents are not published backups, even if their record
            // cannot be parsed. Readers must skip them before opening metadata.
            fs::write(reserved.join("record.json"), "{incomplete").unwrap();
            assert!(studio.skill_backups().unwrap().is_empty());
            atomic::write_json_file(&reserved.join("record.json"), &record).unwrap();

            tx.committed = committed;
            tx.persist().unwrap();
            drop(tx); // Interrupted either before commit or during commit cleanup.
            assert!(studio.skill_backups().unwrap().is_empty());
            recover(&journal).unwrap();
            assert!(!reserved.exists());
            assert!(!journal.exists());
            if committed {
                assert!(studio.skill_backups().unwrap().is_empty());
                assert!(!folder.exists());
            } else {
                assert_eq!(studio.skill_backups().unwrap()[0].id, record.id);
                assert_eq!(
                    fs::read_to_string(payload.join("SKILL.md")).unwrap(),
                    "original"
                );
            }
        }
    }

    #[test]
    fn restart_rolls_back_files_and_configuration_together() {
        let tmp = tempfile::tempdir().unwrap();
        let journal = tmp.path().join("migration.json");
        let source = tmp.path().join("source");
        let config = tmp.path().join("config.json");
        let hub = tmp.path().join("hub");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("SKILL.md"), "original").unwrap();
        fs::write(&config, "old config").unwrap();
        let mut tx = Transaction::begin(journal.clone()).unwrap();
        tx.reserve(&hub).unwrap();
        fs::create_dir(&hub).unwrap();
        tx.reserve(&source).unwrap();
        fs::create_dir(&source).unwrap();
        fs::write(source.join("SKILL.md"), "replacement").unwrap();
        tx.reserve(&config).unwrap();
        fs::write(&config, "new config").unwrap();
        drop(tx); // Simulate process termination before the commit marker.
        recover(&journal).unwrap();
        recover(&journal).unwrap(); // recovery is idempotent
        assert_eq!(
            fs::read_to_string(source.join("SKILL.md")).unwrap(),
            "original"
        );
        assert_eq!(fs::read_to_string(&config).unwrap(), "old config");
        assert!(!hub.exists());
    }
    #[test]
    fn recovery_after_commit_keeps_new_content() {
        let tmp = tempfile::tempdir().unwrap();
        let journal = tmp.path().join("migration.json");
        let target = tmp.path().join("file");
        fs::write(&target, "old").unwrap();
        let mut tx = Transaction::begin(journal.clone()).unwrap();
        tx.reserve(&target).unwrap();
        fs::write(&target, "new").unwrap();
        tx.committed = true;
        tx.persist().unwrap();
        drop(tx);
        recover(&journal).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 2);
    }
    /// A replacement journal travels with a cloned repo, so its contents are
    /// untrusted: an entry pointing outside the journal's own directory must be
    /// refused, and refusing it must not take startup down with it.
    #[test]
    fn confined_recovery_refuses_entries_reaching_outside_the_journal_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let scanned = tmp.path().join("repo/.claude/skills");
        let outside = tmp.path().join("precious");
        fs::create_dir_all(&scanned).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep.txt"), "mine").unwrap();
        let journal = scanned.join(".skill-studio-replace-demo.json");
        for target in [outside.clone(), scanned.join("..").join("escape")] {
            fs::write(
                &journal,
                serde_json::to_vec(&serde_json::json!({
                    "journal": journal,
                    "committed": false,
                    "entries": [{
                        "target": target,
                        "backup": tmp.path().join("nowhere"),
                        "existed": false,
                    }],
                }))
                .unwrap(),
            )
            .unwrap();
            recover_confined(&journal).unwrap();
            assert!(journal.is_file(), "越界的日志应原样保留，不静默删除");
        }
        assert!(outside.join("keep.txt").is_file());
    }

    /// The confinement check must not break the case it guards: an in-bounds
    /// replacement journal still has to roll back. This path had no coverage at all.
    #[test]
    fn confined_recovery_still_rolls_back_an_in_bounds_journal() {
        let tmp = tempfile::tempdir().unwrap();
        let scanned = tmp.path().join("repo/.claude/skills");
        fs::create_dir_all(&scanned).unwrap();
        let dest = scanned.join("demo");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("SKILL.md"), "original").unwrap();
        let journal = scanned.join(".skill-studio-replace-demo.json");
        let mut tx = Transaction::begin(journal.clone()).unwrap();
        tx.reserve(&dest).unwrap();
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("SKILL.md"), "replacement").unwrap();
        drop(tx); // Terminated before the commit marker.
        recover_confined(&journal).unwrap();
        assert_eq!(
            fs::read_to_string(dest.join("SKILL.md")).unwrap(),
            "original"
        );
        assert!(!journal.exists());
    }

    #[test]
    fn confined_recovery_restores_a_journal_written_with_parent_components() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("existing")).unwrap();
        let scanned = tmp.path().join("existing/../skills");
        fs::create_dir_all(&scanned).unwrap();
        let dest = scanned.join("demo");
        fs::write(&dest, "original").unwrap();
        let journal = scanned.join(".skill-studio-replace-demo.json");
        let mut tx = Transaction::begin(journal.clone()).unwrap();
        tx.reserve(&dest).unwrap();
        drop(tx); // Interrupted after moving the original into its backup.
        recover_confined(&journal).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "original");
        assert!(!journal.exists());
    }

    #[cfg(unix)]
    #[test]
    fn confinement_resolves_symlinks_before_parent_components() {
        let tmp = tempfile::tempdir().unwrap();
        let scanned = tmp.path().join("skills");
        let outside = tmp.path().join("outside/child");
        fs::create_dir_all(&scanned).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, scanned.join("alias")).unwrap();
        // Lexically inside skills, but actually inside outside after following alias.
        assert!(!is_direct_child(
            &scanned.join("alias/../precious"),
            &scanned
        ));
        assert!(!is_direct_child(&scanned.join(".."), &scanned));
        let alias = tmp.path().join("skills-alias");
        std::os::unix::fs::symlink(&scanned, &alias).unwrap();
        assert!(is_direct_child(&alias.join("missing-backup"), &scanned));
    }

    #[test]
    fn failed_install_restores_original_target() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("file");
        fs::write(&target, "old").unwrap();
        let mut tx = Transaction::begin(tmp.path().join("journal.json")).unwrap();
        tx.reserve(&target).unwrap();
        assert!(fs::rename(tmp.path().join("missing-stage"), &target).is_err());
        tx.rollback().unwrap();
        assert_eq!(fs::read_to_string(target).unwrap(), "old");
    }
}

#[cfg(test)]
mod locking_tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn a_forked_child_cannot_keep_a_finished_transaction_locked() {
        use std::{
            io::{Read, Write},
            os::unix::{net::UnixStream, process::CommandExt},
            process::Command,
            time::Duration,
        };

        let tmp = tempfile::tempdir().unwrap();
        let journal = tmp.path().join("migration.json");
        let tx = Transaction::begin(journal.clone()).unwrap();
        let (mut parent, mut child) = UnixStream::pair().unwrap();
        parent
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        child
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let worker = std::thread::spawn(move || {
            let mut command = Command::new("/usr/bin/true");
            // Hold the child between fork and exec while it still has inherited
            // file descriptors. Only socket IO runs in the pre-exec callback.
            unsafe {
                command.pre_exec(move || {
                    child.write_all(&[1])?;
                    child.read_exact(&mut [0])?;
                    Ok(())
                });
            }
            command.status()
        });
        parent.read_exact(&mut [0]).unwrap();
        drop(tx);
        let recovered = Transaction::begin(journal);
        // Always release and reap the child before checking the result, including
        // on the old implementation where the inherited lock causes WouldBlock.
        parent.write_all(&[1]).unwrap();
        assert!(worker.join().unwrap().unwrap().success());
        recovered.unwrap().rollback().unwrap();
    }

    #[test]
    fn recovery_cannot_undo_another_live_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let journal = dir.path().join("migration.json");
        let tx = Transaction::begin(journal.clone()).unwrap();
        assert!(recover(&journal).is_err());
        assert!(Transaction::begin(journal.clone()).is_err());
        drop(tx);
        recover(&journal).unwrap();
    }
}

#[cfg(test)]
mod audit_regressions {
    use super::*;
    #[test]
    fn recovery_uses_opened_journal_not_serialized_path() {
        let tmp = tempfile::tempdir().unwrap();
        let victim = tmp.path().join("keep");
        fs::write(&victim, "precious").unwrap();
        let journal = tmp.path().join(".skill-studio-replace-demo.json");
        fs::write(
            &journal,
            serde_json::to_vec(&serde_json::json!({
                "journal": victim, "entries": [], "committed": false
            }))
            .unwrap(),
        )
        .unwrap();
        recover_confined(&journal).unwrap();
        assert!(victim.exists());
        assert!(!journal.exists());
    }
    #[test]
    fn begin_recovers_interrupted_transaction_without_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("file");
        fs::write(&path, "old").unwrap();
        let journal = tmp.path().join("tx.json");
        let mut tx = Transaction::begin(journal.clone()).unwrap();
        tx.reserve(&path).unwrap();
        fs::write(&path, "new").unwrap();
        drop(tx);
        let tx = Transaction::begin(journal).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "old");
        tx.rollback().unwrap();
    }
}
