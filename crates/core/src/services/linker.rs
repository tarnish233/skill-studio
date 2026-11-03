//! 链接引擎：把一个 skill 真身注册到某个 agent 的 skills 目录。
//!
//! 三种方式（沿用 cc-switch 的 `SyncMethod`）：symlink / copy / auto。
//! auto 优先 symlink，失败自动回退 copy —— Windows 没开开发者模式时就会走到这里。
//!
//! 安全底线（全部来自 cc-switch 的实战经验，都有对应测试）：
//! 1. **源必须含 `SKILL.md`** 才允许替换目标，否则空目录会抹掉用户的真 skill
//! 2. **`Foreign` 绝不覆盖**：目标已有非本工具管理的内容时拒绝，除非用户显式 force
//! 3. **目录级替换走 tmp + rename**，任何失败都清理临时目录
//! 4. Hub 目录不能与任何 agent 的 skills 目录重叠，否则同步会自己吃自己

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::fs::{atomic, paths};
use crate::models::skill::{LinkMode, LinkStatus};
use crate::services::scanner::{
    self, is_symlink_or_junction, read_copy_sidecar, validate_sync_source, CopySidecar,
    COPY_SIDECAR, MAX_SCAN_DEPTH,
};

/// 判断某个 skill 在目标位置的状态。
pub fn link_status(source: &Path, dest: &Path) -> LinkStatus {
    // symlink_metadata 不跟随链接，因此悬空链接也能被发现
    let Ok(meta) = dest.symlink_metadata() else {
        return LinkStatus::NotLinked;
    };

    if is_symlink_or_junction(dest) {
        let Ok(target) = fs::read_link(dest) else {
            return LinkStatus::Conflict;
        };
        // 相对链接要按 dest 的父目录解析
        let resolved = if target.is_absolute() {
            target
        } else {
            dest.parent().unwrap_or(Path::new(".")).join(target)
        };
        if !resolved.exists() {
            return LinkStatus::BrokenLink;
        }
        return if paths::paths_alias(&resolved, source) {
            LinkStatus::Linked
        } else {
            LinkStatus::Conflict
        };
    }

    if !meta.is_dir() {
        // 同名文件占位，不是我们能管的
        return LinkStatus::Foreign;
    }

    match read_copy_sidecar(dest) {
        Some(sidecar) => {
            if !paths::paths_alias(&sidecar.source_path, source) {
                // 是本工具复制的，但来源是别的 skill
                return LinkStatus::Conflict;
            }
            if validate_sync_source(dest).is_err() {
                return LinkStatus::CopyDamaged;
            }
            let Ok(actual) = scanner::dir_content_hash(dest) else {
                return LinkStatus::CopyDamaged;
            };
            let Ok(current) = scanner::dir_content_hash(source) else {
                // Source availability does not determine ownership of an intact
                // deployed copy. Never discard target-local modifications.
                return if actual == sidecar.source_hash {
                    LinkStatus::Copied
                } else {
                    LinkStatus::CopyModified
                };
            };
            if actual != sidecar.source_hash {
                if current == sidecar.source_hash {
                    LinkStatus::CopyModified
                } else {
                    LinkStatus::CopyConflict
                }
            } else if current == sidecar.source_hash {
                LinkStatus::Copied
            } else {
                LinkStatus::CopyStale
            }
        }
        // 真目录且没有边车 —— 用户自己放的，绝不动
        None => LinkStatus::Foreign,
    }
}

/// 建符号链接。Windows 上先试目录 symlink，失败回退 junction。
pub fn create_symlink(source: &Path, dest: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, dest).map_err(|e| {
            Error::io_context(
                format!(
                    "创建符号链接失败: {} -> {}",
                    dest.display(),
                    source.display()
                ),
                e,
            )
        })
    }
    #[cfg(windows)]
    {
        // 目录 symlink 需要开发者模式或管理员权限
        match std::os::windows::fs::symlink_dir(source, dest) {
            Ok(()) => Ok(()),
            Err(sym_err) => {
                // junction 不需要特权，是 Windows 上的可靠回退
                match create_junction(source, dest) {
                    Ok(()) => {
                        log::info!(
                            "symlink 失败（{sym_err}），已回退为 junction: {}",
                            dest.display()
                        );
                        Ok(())
                    }
                    Err(j_err) => Err(Error::Other(format!(
                        "创建符号链接与 junction 均失败: symlink={sym_err}, junction={j_err}"
                    ))),
                }
            }
        }
    }
}

#[cfg(windows)]
fn create_junction(source: &Path, dest: &Path) -> std::io::Result<()> {
    use std::process::Command;
    let status = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(dest)
        .arg(source)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "mklink /J 退出码 {:?}",
            status.code()
        )))
    }
}

/// 删除符号链接或 junction。
///
/// Windows 上 junction 是目录类型，`remove_file` 无效，必须先试 `remove_dir`。
pub fn remove_symlink_or_junction(path: &Path) -> std::io::Result<()> {
    // 两个分支都写成块尾表达式（而不是 `return`）：cfg 剥离后剩下的那个块就是
    // 函数尾表达式，写 return 会在对应平台上触发 clippy::needless_return。
    #[cfg(windows)]
    {
        fs::remove_dir(path).or_else(|_| fs::remove_file(path))
    }
    #[cfg(not(windows))]
    {
        // Unix 上文件和目录 symlink 都用 remove_file
        fs::remove_file(path)
    }
}

/// 移除目标位置的内容。只处理链接与本工具管理的复制。
fn remove_dest(dest: &Path) -> Result<()> {
    super::transaction::remove(dest)
}

/// Copy without following links. Internal absolute links move with the tree;
/// external relative links retain their original destination.
pub(crate) fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    copy_tree_contents(src, dst)?;
    copy_root_permissions(src, dst)
}

fn copy_root_permissions(src: &Path, dst: &Path) -> Result<()> {
    fs::set_permissions(
        dst,
        fs::metadata(src)
            .map_err(|e| Error::io(src, e))?
            .permissions(),
    )
    .map_err(|e| Error::io(dst, e))
}

/// Leave the new root writable until its Studio metadata has been written.
fn copy_tree_contents(src: &Path, dst: &Path) -> Result<()> {
    fn walk(root: &Path, target: &Path, src: &Path, dst: &Path, depth: usize) -> Result<()> {
        if depth > MAX_SCAN_DEPTH {
            return Err(Error::invalid("目录层级过深"));
        }
        fs::create_dir_all(dst).map_err(|e| Error::io(dst, e))?;
        for entry in fs::read_dir(src).map_err(|e| Error::io(src, e))? {
            let entry = entry.map_err(|e| Error::io(src, e))?;
            if src == root && entry.file_name() == COPY_SIDECAR {
                continue;
            }
            let from = entry.path();
            let to = dst.join(entry.file_name());
            let meta = fs::symlink_metadata(&from).map_err(|e| Error::io(&from, e))?;
            if is_symlink_or_junction(&from) {
                let resolved = scanner::link_destination(&from)?;
                let link =
                    if let Ok(rel) = resolved.strip_prefix(paths::normalize_path_lexically(root)) {
                        // Relative links remain valid when the staging directory is renamed.
                        let parent_rel = to.parent().unwrap().strip_prefix(target).unwrap();
                        let mut link = PathBuf::new();
                        for _ in parent_rel.components() {
                            link.push("..");
                        }
                        link.push(rel);
                        // A root-level link back to the tree must point to ".";
                        // Linux rejects an empty symlink target with ENOENT.
                        if link.as_os_str().is_empty() {
                            link.push(".");
                        }
                        link
                    } else {
                        resolved
                    };
                #[cfg(unix)]
                std::os::unix::fs::symlink(&link, &to).map_err(|e| Error::io(&to, e))?;
                #[cfg(windows)]
                {
                    if from.is_dir() {
                        std::os::windows::fs::symlink_dir(&link, &to)
                    } else {
                        std::os::windows::fs::symlink_file(&link, &to)
                    }
                    .map_err(|e| Error::io(&to, e))?;
                }
            } else if meta.is_dir() {
                walk(root, target, &from, &to, depth + 1)?;
            } else if meta.is_file() {
                let mut input = fs::File::open(&from).map_err(|e| Error::io(&from, e))?;
                let mut output = fs::File::create(&to).map_err(|e| Error::io(&to, e))?;
                std::io::copy(&mut input, &mut output).map_err(|e| Error::io(&to, e))?;
                // Windows FlushFileBuffers requires a writable handle. Apply
                // source permissions only after flushing, including read-only files.
                output.sync_all().map_err(|e| Error::io(&to, e))?;
                drop(output);
                fs::set_permissions(&to, meta.permissions()).map_err(|e| Error::io(&to, e))?;
            } else {
                return Err(Error::invalid(format!(
                    "不支持的文件类型: {}",
                    from.display()
                )));
            }
        }
        if dst != target {
            copy_root_permissions(src, dst)?;
        }
        Ok(())
    }
    walk(src, dst, src, dst, 0)
}

/// All deployment copies share verification, sidecar ordering and source permissions.
pub(crate) fn copy_skill(source: &Path, dest: &Path, skill_id: &str) -> Result<String> {
    validate_sync_source(source)?;
    let before = scanner::dir_content_hash(source)?;
    copy_tree_contents(source, dest)?;
    let after = scanner::dir_content_hash(source)?;
    if before != after || scanner::dir_content_hash(dest)? != after {
        return Err(Error::invalid("复制校验失败或源在复制期间发生变化"));
    }
    validate_sync_source(dest)?;
    atomic::write_json_file(
        &dest.join(COPY_SIDECAR),
        &CopySidecar {
            skill_id: skill_id.to_owned(),
            source_path: source.to_owned(),
            source_hash: after.clone(),
            copied_at: now_secs(),
        },
    )?;
    copy_root_permissions(source, dest)?;
    Ok(after)
}

/// 用 tmp + rename 的方式把 `source` 复制到 `dest`，并写下复制边车。
///
/// 先复制到同目录下的临时名，成功后再替换，任何失败都清理临时目录 ——
/// 避免中途失败留下半个 skill。
pub fn replace_dest_with_copy(source: &Path, dest: &Path, skill_id: &str) -> Result<String> {
    let journal = replacement_journal(dest);
    super::transaction::recover_confined(&journal)?;
    if journal.exists() {
        return Err(Error::invalid("存在无法安全恢复的副本事务"));
    }
    let (hash, tx) = prepare_copy(source, dest, skill_id, journal)?;
    tx.commit()?;
    Ok(hash)
}

fn prepare_copy(
    source: &Path,
    dest: &Path,
    skill_id: &str,
    journal: PathBuf,
) -> Result<(String, super::transaction::Transaction)> {
    validate_sync_source(source)?;
    // Compare the entry itself, not a final symlink pointing back to source.
    let entry = dest
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.join(dest.file_name().unwrap()))
        .unwrap_or_else(|| dest.to_path_buf());
    let source_real = source.canonicalize().map_err(|e| Error::io(source, e))?;
    if paths::path_is_within(&source_real, &entry) || paths::path_is_within(&entry, &source_real) {
        return Err(Error::invalid("源与目标目录重叠"));
    }
    let parent = dest
        .parent()
        .ok_or_else(|| Error::config("目标没有父目录"))?;
    fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;

    let dest_before = destination_fingerprint(dest)?;
    let mut tx = super::transaction::Transaction::begin(journal)?;
    let tmp = super::transaction::sibling(dest, "staging");
    let operation = (|| -> Result<String> {
        tx.reserve(&tmp)?;
        let after = copy_skill(source, &tmp, skill_id)?;
        if destination_fingerprint(dest)? != dest_before {
            return Err(Error::invalid(
                "目标在复制期间发生变化，已保留目标，请刷新后重试",
            ));
        }
        tx.reserve(dest)?;
        super::transaction::rename(&tmp, dest)?;
        Ok(after)
    })();
    match operation {
        Ok(hash) => Ok((hash, tx)),
        Err(err) => {
            tx.rollback()?;
            Err(err)
        }
    }
}

fn prepare_symlink(
    source: &Path,
    dest: &Path,
    journal: PathBuf,
) -> Result<super::transaction::Transaction> {
    let mut tx = super::transaction::Transaction::begin(journal)?;
    let tmp = super::transaction::sibling(dest, "staging");
    let operation = (|| -> Result<()> {
        tx.reserve(&tmp)?;
        create_symlink(source, &tmp)?;
        tx.reserve(dest)?;
        super::transaction::rename(&tmp, dest)?;
        Ok(())
    })();
    match operation {
        Ok(()) => Ok(tx),
        Err(err) => {
            tx.rollback()?;
            Err(err)
        }
    }
}

fn destination_fingerprint(dest: &Path) -> Result<Option<(String, Vec<u8>)>> {
    let meta = match fs::symlink_metadata(dest) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::io(dest, e)),
    };
    if is_symlink_or_junction(dest) {
        return Ok(Some((
            format!("link:{}", scanner::link_destination(dest)?.display()),
            vec![],
        )));
    }
    if meta.is_dir() {
        let sidecar = match fs::read(dest.join(COPY_SIDECAR)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => return Err(Error::io(dest, e)),
        };
        Ok(Some((scanner::dir_content_hash(dest)?, sidecar)))
    } else {
        Ok(Some((
            "file".into(),
            fs::read(dest).map_err(|e| Error::io(dest, e))?,
        )))
    }
}

fn replacement_journal(dest: &Path) -> PathBuf {
    dest.with_file_name(format!(
        ".skill-studio-replace-{}.json",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ))
}

pub(crate) fn recover_replacements(root: &Path) -> Result<()> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::io(root, e)),
    };
    for entry in entries {
        let path = entry.map_err(|e| Error::io(root, e))?.path();
        let name = path.file_name().unwrap().to_string_lossy();
        if name.starts_with(".skill-studio-replace-") && name.ends_with(".json") {
            super::transaction::recover_confined(&path)?;
        }
    }
    Ok(())
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// 注册成功后的实际落地情况
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    pub mode: LinkMode,
    pub status: LinkStatus,
    /// Copy 模式下记录源哈希，供后续 drift 检测
    pub source_hash: Option<String>,
}

/// 把 `source` 注册到 `dest`。
///
/// `force` 为 false 时，目标是 `Foreign` 或 `Conflict` 一律拒绝 —— 那是用户自己
/// 放的东西或别的 skill，静默覆盖是不可接受的。
pub fn register(
    source: &Path,
    dest: &Path,
    mode: LinkMode,
    skill_id: &str,
    force: bool,
) -> Result<Registered> {
    let (registered, tx) = prepare_registration(
        source,
        dest,
        mode,
        skill_id,
        force,
        replacement_journal(dest),
    )?;
    tx.commit()?;
    Ok(registered)
}

pub(crate) fn prepare_registration(
    source: &Path,
    dest: &Path,
    mode: LinkMode,
    skill_id: &str,
    force: bool,
    journal: PathBuf,
) -> Result<(Registered, super::transaction::Transaction)> {
    // Recover before inspecting ownership or source/destination state.
    if journal == replacement_journal(dest) {
        super::transaction::recover_confined(&journal)?;
        if journal.exists() {
            return Err(Error::invalid("存在无法安全恢复的副本事务"));
        }
    } else {
        super::transaction::recover(&journal)?;
    }
    validate_sync_source(source)?;
    if paths::is_same_path(source, dest) {
        return Err(Error::invalid(format!(
            "源与目标是同一路径，无需注册: {}",
            source.display()
        )));
    }

    let source_real = source.canonicalize().map_err(|e| Error::io(source, e))?;
    let dest_entry = dest
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.join(dest.file_name().unwrap()))
        .unwrap_or_else(|| dest.to_path_buf());
    if paths::path_is_within(&source_real, &dest_entry)
        || paths::path_is_within(&dest_entry, &source_real)
    {
        return Err(Error::invalid("源与目标目录重叠，拒绝注册"));
    }
    let current = link_status(source, dest);
    match current {
        LinkStatus::Foreign if !force => {
            return Err(Error::Foreign(dest.display().to_string()));
        }
        LinkStatus::CopyModified | LinkStatus::CopyConflict | LinkStatus::CopyDamaged if !force => {
            return Err(Error::invalid(
                "副本有本地修改、冲突或损坏；请先备份/合并，拒绝自动覆盖",
            ));
        }
        LinkStatus::Conflict if !force => {
            return Err(Error::Foreign(format!(
                "{}（指向别处，需显式覆盖）",
                dest.display()
            )));
        }
        _ => {}
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }

    let copy = || -> Result<(Registered, super::transaction::Transaction)> {
        let (hash, tx) = prepare_copy(source, dest, skill_id, journal.clone())?;
        Ok((
            Registered {
                mode: LinkMode::Copy,
                status: LinkStatus::Copied,
                source_hash: Some(hash),
            },
            tx,
        ))
    };
    match mode {
        LinkMode::Copy => copy(),
        LinkMode::Symlink => {
            let tx = prepare_symlink(source, dest, journal)?;
            Ok((
                Registered {
                    mode: LinkMode::Symlink,
                    status: LinkStatus::Linked,
                    source_hash: None,
                },
                tx,
            ))
        }
        LinkMode::Auto => {
            if dest.is_dir() && !is_symlink_or_junction(dest) {
                return copy();
            }
            match prepare_symlink(source, dest, journal.clone()) {
                Ok(tx) => Ok((
                    Registered {
                        mode: LinkMode::Symlink,
                        status: LinkStatus::Linked,
                        source_hash: None,
                    },
                    tx,
                )),
                Err(err) => {
                    log::warn!("symlink 创建失败，回退为文件复制: {err}");
                    copy()
                }
            }
        }
    }
}

/// 取消注册。**只移除本工具管理的内容**，`Foreign` 一律跳过。
///
/// 返回是否真的移除了东西。
pub fn unregister(source: &Path, dest: &Path, force: bool) -> Result<bool> {
    let status = link_status(source, dest);
    match status {
        LinkStatus::NotLinked => Ok(false),
        LinkStatus::Foreign
        | LinkStatus::Conflict
        | LinkStatus::CopyModified
        | LinkStatus::CopyConflict
        | LinkStatus::CopyDamaged
            if !force =>
        {
            Err(Error::Foreign(dest.display().to_string()))
        }
        _ => {
            remove_dest(dest)?;
            Ok(true)
        }
    }
}

/// Hub 目录不能与任何 agent 的 skills 目录重叠，否则同步会自己吃自己。
pub fn ensure_distinct_roots(hub: &Path, agent_roots: &[PathBuf]) -> Result<()> {
    for root in agent_roots {
        if paths::paths_overlap(hub, root) {
            return Err(Error::invalid(format!(
                "Hub 目录不能与 agent 的 skills 目录重叠: {} 与 {}",
                hub.display(),
                root.display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(dir: &Path, body: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        source: PathBuf,
        dest_root: PathBuf,
        dest: PathBuf,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("src/pdf");
        make_skill(&source, "---\nname: pdf\n---\nbody");
        let dest_root = tmp.path().join("agent/skills");
        fs::create_dir_all(&dest_root).unwrap();
        let dest = dest_root.join("pdf");
        Fixture {
            _tmp: tmp,
            source,
            dest_root,
            dest,
        }
    }

    #[test]
    fn status_is_not_linked_when_dest_absent() {
        let f = fixture();
        assert_eq!(link_status(&f.source, &f.dest), LinkStatus::NotLinked);
    }

    #[test]
    fn copy_registration_reports_copied_then_stale_after_source_changes() {
        let f = fixture();
        let r = register(&f.source, &f.dest, LinkMode::Copy, "id1", false).unwrap();
        assert_eq!(r.mode, LinkMode::Copy);
        assert_eq!(link_status(&f.source, &f.dest), LinkStatus::Copied);
        assert!(f.dest.join(COPY_SIDECAR).is_file(), "应写下复制边车");
        assert!(f.dest.join("SKILL.md").is_file());

        // 源变了 → 目标过期
        fs::write(f.source.join("SKILL.md"), "---\nname: pdf\n---\nCHANGED").unwrap();
        assert_eq!(link_status(&f.source, &f.dest), LinkStatus::CopyStale);

        // 重新注册后回到 Copied
        register(&f.source, &f.dest, LinkMode::Copy, "id1", false).unwrap();
        assert_eq!(link_status(&f.source, &f.dest), LinkStatus::Copied);
    }

    #[test]
    fn copy_does_not_include_sidecar_from_source() {
        let f = fixture();
        // 源自己带了一个边车（比如源本身是别处复制来的），不应被带进目标
        fs::write(f.source.join(COPY_SIDECAR), r#"{"skillId":"stale"}"#).unwrap();
        register(&f.source, &f.dest, LinkMode::Copy, "id1", false).unwrap();
        let sidecar = read_copy_sidecar(&f.dest).unwrap();
        assert_eq!(sidecar.skill_id, "id1", "边车应是本次注册写的");
    }

    #[test]
    fn refuses_source_without_skill_md() {
        let tmp = tempfile::tempdir().unwrap();
        let empty = tmp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        let dest = tmp.path().join("dest");
        let err = register(&empty, &dest, LinkMode::Copy, "id", false).unwrap_err();
        assert!(err.to_string().contains("SKILL.md"), "{err}");
        assert!(!dest.exists(), "失败时不应留下任何东西");
    }

    #[test]
    fn empty_source_cannot_wipe_an_existing_dest() {
        // 这是最关键的一条不变式
        let f = fixture();
        register(&f.source, &f.dest, LinkMode::Copy, "id1", false).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let empty = tmp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();

        let err = replace_dest_with_copy(&empty, &f.dest, "id1").unwrap_err();
        assert!(err.to_string().contains("SKILL.md"), "{err}");
        assert!(f.dest.join("SKILL.md").is_file(), "已有目标必须完好无损");
    }

    #[test]
    fn foreign_dest_is_never_overwritten_without_force() {
        let f = fixture();
        // 用户自己在 agent 目录里放了一个同名 skill（无边车）
        make_skill(&f.dest, "---\nname: users-own\n---\nMINE");
        assert_eq!(link_status(&f.source, &f.dest), LinkStatus::Foreign);

        let err = register(&f.source, &f.dest, LinkMode::Copy, "id1", false).unwrap_err();
        assert!(matches!(err, Error::Foreign(_)), "{err}");
        assert_eq!(
            fs::read_to_string(f.dest.join("SKILL.md")).unwrap(),
            "---\nname: users-own\n---\nMINE",
            "用户的内容必须原样保留"
        );

        // force 时才允许覆盖
        register(&f.source, &f.dest, LinkMode::Copy, "id1", true).unwrap();
        assert_eq!(link_status(&f.source, &f.dest), LinkStatus::Copied);
    }

    #[test]
    fn unregister_skips_foreign_but_removes_managed() {
        let f = fixture();
        make_skill(&f.dest, "---\nname: users-own\n---\n");
        let err = unregister(&f.source, &f.dest, false).unwrap_err();
        assert!(matches!(err, Error::Foreign(_)), "{err}");
        assert!(f.dest.exists(), "Foreign 必须留着");

        // 换成我们管理的复制，就能移除
        register(&f.source, &f.dest, LinkMode::Copy, "id1", true).unwrap();
        assert!(unregister(&f.source, &f.dest, false).unwrap());
        assert!(!f.dest.exists());
        // 幂等：再来一次返回 false 而不是报错
        assert!(!unregister(&f.source, &f.dest, false).unwrap());
    }

    #[test]
    fn rejects_registering_onto_itself() {
        let f = fixture();
        let err = register(&f.source, &f.source, LinkMode::Copy, "id", false).unwrap_err();
        assert!(err.to_string().contains("同一路径"), "{err}");
    }

    #[test]
    fn ensure_distinct_roots_rejects_overlap() {
        let tmp = tempfile::tempdir().unwrap();
        let hub = tmp.path().join("hub");
        let inside = hub.join("nested");
        assert!(ensure_distinct_roots(&hub, &[inside]).is_err());
        assert!(ensure_distinct_roots(&hub, &[tmp.path().join("other")]).is_ok());
        assert!(ensure_distinct_roots(&hub, std::slice::from_ref(&hub)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_registration_reports_linked() {
        let f = fixture();
        let r = register(&f.source, &f.dest, LinkMode::Symlink, "id1", false).unwrap();
        assert_eq!(r.mode, LinkMode::Symlink);
        assert_eq!(link_status(&f.source, &f.dest), LinkStatus::Linked);
        assert!(is_symlink_or_junction(&f.dest));
        // 通过链接能读到源内容
        assert!(f.dest.join("SKILL.md").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn re_registering_an_existing_symlink_is_idempotent() {
        // 回归：曾用 paths_alias 判定「源与目标同一路径」，目标一旦是指向源的软链
        // 就会被误判成真身本体，重复注册直接报错、也无法移除。
        let f = fixture();
        register(&f.source, &f.dest, LinkMode::Symlink, "id1", false).unwrap();
        let again = register(&f.source, &f.dest, LinkMode::Symlink, "id1", false)
            .expect("重复注册应当成功");
        assert_eq!(again.status, LinkStatus::Linked);
        assert!(unregister(&f.source, &f.dest, false).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn auto_prefers_symlink_on_unix() {
        let f = fixture();
        let r = register(&f.source, &f.dest, LinkMode::Auto, "id1", false).unwrap();
        assert_eq!(r.mode, LinkMode::Symlink);
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_is_broken_not_linked() {
        let f = fixture();
        std::os::unix::fs::symlink(f.dest_root.join("missing"), &f.dest).unwrap();
        assert_eq!(link_status(&f.source, &f.dest), LinkStatus::BrokenLink);
        // 悬空链接算本工具管理，可以直接清理
        assert!(unregister(&f.source, &f.dest, false).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_another_target_is_conflict() {
        let f = fixture();
        let other = f._tmp.path().join("src/other");
        make_skill(&other, "---\nname: other\n---\n");
        std::os::unix::fs::symlink(&other, &f.dest).unwrap();
        assert_eq!(link_status(&f.source, &f.dest), LinkStatus::Conflict);

        // Conflict 不 force 也拒绝
        let err = register(&f.source, &f.dest, LinkMode::Symlink, "id", false).unwrap_err();
        assert!(matches!(err, Error::Foreign(_)), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn relative_symlink_pointing_at_source_counts_as_linked() {
        let f = fixture();
        // 建一个相对链接，必须按 dest 的父目录解析
        let rel = std::path::Path::new("../../src/pdf");
        std::os::unix::fs::symlink(rel, &f.dest).unwrap();
        assert_eq!(link_status(&f.source, &f.dest), LinkStatus::Linked);
    }

    #[cfg(unix)]
    #[test]
    fn copy_preserves_symlinks_without_following_them() {
        let f = fixture();
        std::os::unix::fs::symlink("/etc/hosts", f.source.join("outside")).unwrap();
        register(&f.source, &f.dest, LinkMode::Copy, "id1", false).unwrap();
        assert!(
            f.dest
                .join("outside")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink(),
            "源里的 symlink 应保留且不能递归跟随"
        );
    }

    #[test]
    fn a_plain_file_at_dest_is_foreign() {
        let f = fixture();
        fs::write(&f.dest, "not a dir").unwrap();
        assert_eq!(link_status(&f.source, &f.dest), LinkStatus::Foreign);
    }

    #[test]
    fn sidecar_from_a_different_source_is_conflict() {
        let f = fixture();
        register(&f.source, &f.dest, LinkMode::Copy, "id1", false).unwrap();
        // 篡改边车，让它指向别的源
        let mut sidecar = read_copy_sidecar(&f.dest).unwrap();
        sidecar.source_path = PathBuf::from("/somewhere/else");
        atomic::write_json_file(&f.dest.join(COPY_SIDECAR), &sidecar).unwrap();
        assert_eq!(link_status(&f.source, &f.dest), LinkStatus::Conflict);
    }

    #[test]
    fn no_temp_dirs_left_behind_after_copy() {
        let f = fixture();
        register(&f.source, &f.dest, LinkMode::Copy, "id1", false).unwrap();
        let leftovers: Vec<_> = fs::read_dir(&f.dest_root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "残留临时目录: {leftovers:?}");
    }

    #[test]
    fn copy_preserves_nested_structure() {
        let f = fixture();
        fs::create_dir_all(f.source.join("scripts")).unwrap();
        fs::write(f.source.join("scripts/run.sh"), "echo hi").unwrap();
        fs::create_dir_all(f.source.join("references")).unwrap();
        fs::write(f.source.join("references/api.md"), "# api").unwrap();

        register(&f.source, &f.dest, LinkMode::Copy, "id1", false).unwrap();

        assert_eq!(
            fs::read_to_string(f.dest.join("scripts/run.sh")).unwrap(),
            "echo hi"
        );
        assert!(f.dest.join("references/api.md").is_file());
    }
}
