//! Watch physical directories without following nested symlinks. Reconcile the
//! directory set after changes and periodically so newly created roots and
//! settings changes take effect without restarting the application.
use crate::state::AppState;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use skill_studio_core::{models::agent::AGENTS, services::scanner};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};

pub const SKILLS_CHANGED_EVENT: &str = "skills-changed";
/// 别的进程（命令行、另一个 Skill Studio、远程辅助程序）改了配置：分组、项目、设置都可能变了
pub const CONFIG_CHANGED_EVENT: &str = "config-changed";
/// MCP 托管状态变了（含本进程的写入：重取一次 MCP 状态无妨）
pub const MCP_CHANGED_EVENT: &str = "mcp-changed";
const DEBOUNCE: Duration = Duration::from_millis(400);
const RECONCILE: Duration = Duration::from_secs(1);

pub struct SkillWatcher {
    stop: Arc<AtomicBool>,
}
impl Drop for SkillWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn add_physical_tree(path: &Path, dirs: &mut HashSet<PathBuf>, depth: usize) {
    if depth > 16 {
        return;
    }
    let Ok(meta) = path.symlink_metadata() else {
        return;
    };
    if !meta.is_dir() || scanner::is_symlink_or_junction(path) {
        return;
    }
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !dirs.insert(canonical) {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.file_name() != ".git" {
                add_physical_tree(&entry.path(), dirs, depth + 1);
            }
        }
    }
}

fn desired_directories(roots: &[PathBuf]) -> HashSet<PathBuf> {
    let mut dirs = HashSet::new();
    for root in roots {
        // Observe a missing root's nearest ancestor until the root is created.
        let mut ancestor = root.as_path();
        while !ancestor.is_dir() {
            let Some(parent) = ancestor.parent() else {
                break;
            };
            ancestor = parent;
        }
        if let Ok(canonical) = ancestor.canonicalize() {
            dirs.insert(canonical);
        }
        // The root may itself be a configured alias. Enumerate its physical tree.
        if let Ok(canonical) = root.canonicalize() {
            dirs.remove(&canonical);
            add_physical_tree(&canonical, &mut dirs, 0);
        }
        // Only first-level links represent skill sources. Internal links are never followed.
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                if scanner::is_symlink_or_junction(&entry.path()) {
                    if let Ok(source) = entry.path().canonicalize() {
                        add_physical_tree(&source, &mut dirs, 0);
                    }
                }
            }
        }
    }
    dirs
}

/// 原子写入每次都换一个新文件：修改时间和大小足以看出变化
type FileVersion = Option<(std::time::SystemTime, u64)>;

fn file_version(path: &Path) -> FileVersion {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// 每秒看一次：别的进程改了配置、MCP 托管状态变了，各要推送哪个事件。
/// 配置只看版本，不监听整个配置目录（日志、备份写入频繁）
fn outside_changes(state: &AppState, catalog: &Path, seen: &mut FileVersion) -> Vec<&'static str> {
    let mut events = Vec::new();
    if state.take_external_change() {
        events.push(CONFIG_CHANGED_EVENT);
    }
    let version = file_version(catalog);
    if version != *seen {
        *seen = version;
        events.push(MCP_CHANGED_EVENT);
    }
    events
}

fn roots(state: &AppState) -> Vec<PathBuf> {
    let config = state.config();
    let mut roots: Vec<_> = AGENTS
        .iter()
        .flat_map(|a| a.resolved_global_roots(&config.settings.agent_dir_overrides))
        .collect();
    roots.push(state.studio().store().hub_dir(&config));
    roots
}

fn reconcile(
    watcher: &mut RecommendedWatcher,
    watched: &mut HashSet<PathBuf>,
    desired: HashSet<PathBuf>,
) -> bool {
    let changed = *watched != desired;
    for removed in watched.difference(&desired) {
        let _ = watcher.unwatch(removed);
    }
    watched.retain(|p| desired.contains(p));
    for added in desired.difference(watched).cloned().collect::<Vec<_>>() {
        match watcher.watch(&added, RecursiveMode::NonRecursive) {
            Ok(()) => {
                watched.insert(added);
            }
            Err(e) => log::debug!("监听 {} 失败: {e}", added.display()),
        }
    }
    changed
}

pub fn spawn(app: AppHandle, state: AppState) -> Option<SkillWatcher> {
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = tx.send(event);
    })
    .ok()?;
    let mut watched = HashSet::new();
    reconcile(
        &mut watcher,
        &mut watched,
        desired_directories(&roots(&state)),
    );
    log::info!("已监听 {} 个 skill 目录（含内部资源目录）", watched.len());
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let catalog = state
        .studio()
        .store()
        .dir()
        .join("mcp")
        .join("catalog.json");
    std::thread::spawn(move || {
        let mut pending: Option<Instant> = None;
        let mut refreshed = Instant::now();
        let mut catalog_seen = file_version(&catalog);
        while !stopped.load(Ordering::Relaxed) {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(event)) if is_interesting(&event) => {
                    pending.get_or_insert_with(Instant::now);
                }
                Ok(Err(e)) => log::debug!("文件监听事件错误: {e}"),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                _ => {}
            }
            if refreshed.elapsed() >= RECONCILE {
                if reconcile(
                    &mut watcher,
                    &mut watched,
                    desired_directories(&roots(&state)),
                ) {
                    pending.get_or_insert_with(Instant::now);
                }
                for event in outside_changes(&state, &catalog, &mut catalog_seen) {
                    if let Err(e) = app.emit(event, ()) {
                        log::debug!("推送 {event} 失败: {e}");
                    }
                }
                refreshed = Instant::now();
            }
            if pending.is_some_and(|since| since.elapsed() >= DEBOUNCE) {
                pending = None;
                if let Err(e) = app.emit(SKILLS_CHANGED_EVENT, ()) {
                    log::debug!("推送刷新事件失败: {e}");
                }
            }
        }
    });
    Some(SkillWatcher { stop })
}

fn is_interesting(event: &notify::Event) -> bool {
    matches!(
        event.kind,
        notify::EventKind::Create(_) | notify::EventKind::Remove(_) | notify::EventKind::Modify(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_changes_made_elsewhere_refresh_the_configuration_and_each_mcp_write_refreshes_mcp() {
        use skill_studio_core::services::store::Store;
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path().join(".skill-studio"));
        std::fs::create_dir_all(store.dir()).unwrap();
        std::fs::write(store.config_path(), "{}").unwrap();
        let desktop = AppState::bootstrap(store.clone()).unwrap();
        let cli = AppState::bootstrap(store.clone()).unwrap();
        let catalog = store.dir().join("mcp/catalog.json");
        let mut seen = file_version(&catalog);
        assert!(outside_changes(&desktop, &catalog, &mut seen).is_empty());

        let named = |state: &AppState, name: &str| {
            state
                .mutate(|_, config| {
                    config.settings.language = name.into();
                    Ok(())
                })
                .unwrap()
        };
        named(&desktop, "zh");
        assert!(
            outside_changes(&desktop, &catalog, &mut seen).is_empty(),
            "自己的写入已经刷新过界面"
        );
        named(&cli, "en");
        assert_eq!(
            outside_changes(&desktop, &catalog, &mut seen),
            [CONFIG_CHANGED_EVENT]
        );
        assert!(outside_changes(&desktop, &catalog, &mut seen).is_empty());

        std::fs::create_dir_all(catalog.parent().unwrap()).unwrap();
        std::fs::write(&catalog, "{}").unwrap();
        assert_eq!(
            outside_changes(&desktop, &catalog, &mut seen),
            [MCP_CHANGED_EVENT]
        );
        assert!(outside_changes(&desktop, &catalog, &mut seen).is_empty());
    }
    #[test]
    fn missing_hub_and_nested_directories_are_reconciled() {
        let dir = tempfile::tempdir().unwrap();
        let hub = dir.path().join("hub");
        let first = desired_directories(std::slice::from_ref(&hub));
        assert!(first.contains(&dir.path().canonicalize().unwrap()));
        std::fs::create_dir_all(hub.join("skill/scripts")).unwrap();
        let next = desired_directories(std::slice::from_ref(&hub));
        assert!(next.contains(&hub.join("skill/scripts").canonicalize().unwrap()));
    }
    #[test]
    #[cfg(unix)]
    fn external_sources_are_watched_but_internal_cycles_are_not_traversed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("agent");
        let source = dir.path().join("external");
        std::fs::create_dir_all(source.join("scripts")).unwrap();
        std::fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(&source, root.join("alias")).unwrap();
        std::os::unix::fs::symlink(&source, source.join("loop")).unwrap();
        let watched = desired_directories(&[root]);
        assert!(watched.contains(&source.join("scripts").canonicalize().unwrap()));
        assert_eq!(watched.len(), 3);
    }
}
