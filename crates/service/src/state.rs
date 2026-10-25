use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, SystemTime};

use skill_studio_core::models::config::AppConfig;
use skill_studio_core::models::group::UntouchedCopy;
use skill_studio_core::models::project::ProjectSelection;
use skill_studio_core::models::same_members;
use skill_studio_core::models::skill::LinkReport;
use skill_studio_core::services::store::{ConfigLock, Store};
use skill_studio_core::services::studio::Studio;
use skill_studio_core::{Error, Result};

/// 等另一个进程写完配置的上限：一次部署大量 skill 可能要好几秒
const LOCK_WAIT: Duration = Duration::from_secs(30);

/// 读的一方等锁的上限。读取从不跟着写的一方排队：等不到就用上一次读到的完整配置
const READ_WAIT: Duration = Duration::from_secs(1);

const READ_ONLY: &str = "当前为只读模式，不能修改";

pub use skill_studio_core::services::store::CHANGED_ELSEWHERE;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Normal,
    /// 配置没能加载：只能恢复备份
    Recovery,
    /// 只读：不恢复事务、不写任何文件
    ReadOnly,
}

/// 内存里那份配置对应的磁盘版本。原子写入每次都换一个新文件，元数据足以分辨
#[derive(Clone, PartialEq, Eq, Debug)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    file: (u64, u64, i64, i64),
    #[cfg(windows)]
    created: u64,
}

#[derive(Clone, Debug)]
enum Disk {
    /// 不确定：下次读写前一定重新读
    Unknown,
    Missing,
    Present(Stamp),
}

fn disk(path: &Path) -> Disk {
    match std::fs::metadata(path) {
        Ok(meta) => Disk::Present(Stamp {
            len: meta.len(),
            modified: meta.modified().ok(),
            #[cfg(unix)]
            file: {
                use std::os::unix::fs::MetadataExt;
                (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec())
            },
            #[cfg(windows)]
            created: {
                use std::os::windows::fs::MetadataExt;
                meta.creation_time()
            },
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Disk::Missing,
        Err(_) => Disk::Unknown,
    }
}

/// 磁盘上的配置还是内存里那一份
fn same(memory: &Disk, now: &Disk) -> bool {
    match (memory, now) {
        (Disk::Missing, Disk::Missing) => true,
        (Disk::Present(a), Disk::Present(b)) => a == b,
        _ => false,
    }
}

struct Shared {
    config: AppConfig,
    disk: Disk,
}

/// 全局状态。内部全 `Arc`，本身 `Clone`；锁只包住 `AppConfig`，
/// 不做粗粒度全局锁（cc-switch 的教训：粗锁跨 IO 会连锁阻塞）。
///
/// 配置可能同时被别的进程修改（命令行、另一个 Skill Studio、远程辅助程序），内存里的
/// 只是磁盘的缓存：读时发现磁盘已变就重新读，写前拿跨进程独占锁并按磁盘上的最新版本来，
/// 不会把别人刚写下的内容用旧缓存盖掉。
#[derive(Clone)]
pub struct AppState {
    studio: Arc<Studio>,
    shared: Arc<RwLock<Shared>>,
    mode: Mode,
    /// 因为别的进程写过而重新读了配置，还没通知界面
    external: Arc<AtomicBool>,
    lock_wait: Duration,
}

/// 读配置的守卫。只是内存里的一份快照：读写双方都不必为它排队（写入是原子替换，读到的
/// 总是完整的一份）。内存里那份已过期又来不及更新（别的线程正拿着它）时，带一份刚从磁盘
/// 读出的给这次用。
pub struct ConfigRef<'a> {
    guard: RwLockReadGuard<'a, Shared>,
    fresh: Option<AppConfig>,
}

impl Deref for ConfigRef<'_> {
    type Target = AppConfig;
    fn deref(&self) -> &AppConfig {
        self.fresh.as_ref().unwrap_or(&self.guard.config)
    }
}

/// 写配置的守卫：拿着跨进程独占锁。结束时如果磁盘上的配置变了（写入成功，或写到一半
/// 失败又回滚），以磁盘为准刷新内存，保证两者一致。
pub(crate) struct ConfigMut<'a> {
    state: &'a AppState,
    guard: RwLockWriteGuard<'a, Shared>,
    start: Disk,
    _lock: ConfigLock,
}

impl Deref for ConfigMut<'_> {
    type Target = AppConfig;
    fn deref(&self) -> &AppConfig {
        &self.guard.config
    }
}

impl DerefMut for ConfigMut<'_> {
    fn deref_mut(&mut self) -> &mut AppConfig {
        &mut self.guard.config
    }
}

impl Drop for ConfigMut<'_> {
    fn drop(&mut self) {
        if self.state.mode != Mode::Normal {
            return;
        }
        let store = self.state.studio.store();
        let now = disk(&store.config_path());
        if same(&self.start, &now) {
            return;
        }
        match store.read() {
            Ok(Some(config)) => {
                self.guard.config = config;
                self.guard.disk = now;
            }
            _ => self.guard.disk = Disk::Unknown,
        }
    }
}

impl AppState {
    fn new(studio: Studio, config: AppConfig, disk: Disk, mode: Mode) -> Self {
        Self {
            studio: Arc::new(studio),
            shared: Arc::new(RwLock::new(Shared { config, disk })),
            mode,
            external: Arc::new(AtomicBool::new(false)),
            lock_wait: LOCK_WAIT,
        }
    }

    /// 启动时加载配置。加载失败**不静默回落默认值**，把错误交给上层决定怎么提示。
    pub fn bootstrap(store: Store) -> Result<Self> {
        Self::bootstrap_waiting(store, LOCK_WAIT)
    }

    /// 同 [`Self::bootstrap`]，等别的进程写完配置最多等 `wait`，之后的读写也按这个上限
    pub fn bootstrap_waiting(store: Store, wait: Duration) -> Result<Self> {
        // 启动时的事务恢复与 MCP 状态校正也会写配置，和其他进程的写入错开
        let lock = store.lock_exclusive(wait)?;
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
                .map_err(|error| Error::invalid(format!("恢复 MCP 管理状态失败：{error:#}")))?
                .suspended;
            if config.settings.manage_mcp != enabled {
                config.settings.manage_mcp = enabled;
                studio.save_config(&config)?;
            }
        }
        let stamp = disk(&studio.store().config_path());
        drop(lock);
        Ok(Self::new(studio, config, stamp, Mode::Normal).with_lock_wait(wait))
    }

    pub fn recovery(store: Store) -> Self {
        Self::new(
            Studio::new(store),
            AppConfig::default(),
            Disk::Unknown,
            Mode::Recovery,
        )
    }

    /// 只读地打开：不恢复中断的事务、不校正任何状态、不写任何文件，用于只查看的场景
    /// （命令行的查看命令，以及只允许读的 Agent 沙箱）。
    pub fn read_only(store: Store) -> Result<Self> {
        Self::read_only_waiting(store, LOCK_WAIT)
    }

    /// 同 [`Self::read_only`]，等正在写的进程最多等 `wait`
    pub fn read_only_waiting(store: Store, wait: Duration) -> Result<Self> {
        let lock = store.lock_shared(wait);
        let stamp = disk(&store.config_path());
        let config = match store.read()? {
            Some(config) => config,
            None if store.pending_transactions() => {
                return Err(Error::Other(
                    "Skill Studio 有一项写入还没完成，请先打开 Skill Studio 完成恢复，再查看"
                        .into(),
                ))
            }
            None => AppConfig::default(),
        };
        drop(lock);
        Ok(Self::new(Studio::new(store), config, stamp, Mode::ReadOnly).with_lock_wait(wait))
    }

    /// 等另一个进程写完配置的上限，默认 30 秒
    pub fn with_lock_wait(mut self, wait: Duration) -> Self {
        self.lock_wait = wait;
        self
    }

    pub fn ensure_writable(&self) -> Result<()> {
        match self.mode {
            Mode::Normal => Ok(()),
            Mode::Recovery => Err(Error::invalid(
                "配置尚未正常加载，请先恢复配置备份并重启应用",
            )),
            Mode::ReadOnly => Err(Error::Other(READ_ONLY.into())),
        }
    }

    /// 只读打开（命令行的查看命令）：读其他数据时也不要建文件、不要恢复中断的写入
    pub fn is_read_only(&self) -> bool {
        self.mode == Mode::ReadOnly
    }

    /// 配置已正常加载（可写或只读），可以据它判断
    pub fn ensure_loaded(&self) -> Result<()> {
        match self.mode {
            Mode::Recovery => self.ensure_writable(),
            _ => Ok(()),
        }
    }

    pub fn studio(&self) -> &Studio {
        &self.studio
    }

    /// 读配置。别的进程刚写过时读到的是磁盘上的最新内容；锁被毒化时恢复而不 panic
    /// （cc-switch 的惯例）。
    pub fn config(&self) -> ConfigRef<'_> {
        if self.mode == Mode::Recovery {
            return ConfigRef {
                guard: read(&self.shared),
                fresh: None,
            };
        }
        self.refresh_if_idle();
        let guard = read(&self.shared);
        let store = self.studio.store();
        let fresh = if same(&guard.disk, &disk(&store.config_path())) {
            None
        } else {
            store.read().ok().flatten()
        };
        ConfigRef { guard, fresh }
    }

    /// 磁盘上的配置变了、内存又没人用时，就地换成新的。
    ///
    /// 慢路径里刻意不持任何 `RwLock`：等另一个进程写完配置时（最多 [`READ_WAIT`]），
    /// 不能让本进程的其他线程跟着排队。内存被占着就留给下一次，这次的读取另读一份。
    fn refresh_if_idle(&self) {
        let store = self.studio.store();
        let now = disk(&store.config_path());
        match self.shared.try_read() {
            Ok(guard) if same(&guard.disk, &now) => return,
            Ok(_) => {}
            Err(_) => return,
        }
        // 等正在写的进程写完，避免读到它挪开配置文件的那一瞬间；等不到也照读
        let _lock = store.lock_shared(READ_WAIT);
        let now = disk(&store.config_path());
        // 文件不在（别的进程的事务正把它挪开）或读不出来时保留内存里的，不回落到默认值
        let Ok(Some(config)) = store.read() else {
            return;
        };
        let Ok(mut guard) = self.shared.try_write() else {
            return;
        };
        if same(&guard.disk, &now) {
            return;
        }
        guard.config = config;
        guard.disk = now;
        self.external.store(true, Ordering::Release);
    }

    /// 别的进程改过配置时返回 true，每次改动只报一次：界面据此刷新
    pub fn take_external_change(&self) -> bool {
        if self.mode != Mode::Normal {
            return false;
        }
        self.refresh_if_idle();
        self.external.swap(false, Ordering::AcqRel)
    }

    /// 拿到写锁：本进程内互斥，并持跨进程独占锁。别的进程写过时先按磁盘上的最新版本
    /// 重新加载（含恢复它写到一半留下的事务），再在它上面改
    fn config_mut(&self) -> Result<ConfigMut<'_>> {
        if self.mode == Mode::ReadOnly {
            return Err(Error::Other(READ_ONLY.into()));
        }
        // 先等跨进程的写锁，再改内存：等别的进程写完的这段时间里，本进程的读取照常进行
        // （读取不拿任何跨进程锁，只读内存快照）
        let lock = self.studio.store().lock_exclusive(self.lock_wait)?;
        let mut guard = self.shared.write().unwrap_or_else(|poisoned| {
            log::warn!("配置写锁被毒化，已恢复");
            poisoned.into_inner()
        });
        if self.mode == Mode::Normal {
            let path = self.studio.store().config_path();
            if !same(&guard.disk, &disk(&path)) {
                guard.config = self.studio.load_config()?;
                guard.disk = disk(&path);
                self.external.store(true, Ordering::Release);
            }
        }
        let start = guard.disk.clone();
        Ok(ConfigMut {
            state: self,
            guard,
            start,
            _lock: lock,
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
        let mut guard = self.config_mut()?;
        let mut next = (*guard).clone();
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
        let mut guard = self.config_mut()?;
        f(&self.studio, &mut guard)
    }

    pub fn register_skills(
        &self,
        ids: &[String],
        agents: &[String],
        mode: Option<skill_studio_core::models::skill::LinkMode>,
        force: bool,
    ) -> Result<LinkReport> {
        self.transactional(|studio, config| studio.register(config, ids, agents, mode, force))
    }

    pub fn write_project(
        &self,
        id: &str,
        selection: Option<ProjectSelection>,
    ) -> Result<LinkReport> {
        self.transactional(|studio, config| studio.write_project(config, id, selection))
    }

    /// 为一个 Agent 启用或停用项目，成员按配置里最新的。已经是目标状态时什么也不做。
    /// 在写锁内按最新状态计算，不会撤掉别处（另一个窗口、命令行）刚为其他 Agent 做的启用
    pub fn set_project_agent(&self, id: &str, agent_id: &str, enabled: bool) -> Result<LinkReport> {
        self.transactional(|studio, config| {
            let mut agents = deployed_agents(config, id)?;
            if agents.iter().any(|a| a == agent_id) == enabled {
                return Ok(LinkReport::default());
            }
            if enabled {
                skill_studio_core::models::agent::require_agent(agent_id)?;
                agents.push(agent_id.to_string());
            } else {
                agents.retain(|a| a != agent_id);
            }
            let selection = current_selection(config, id, agents)?;
            studio.write_project(config, id, Some(selection))
        })
    }

    /// 按配置里最新的成员，重新写入项目已启用的全部 Agent（应用修改）
    pub fn reapply_project(&self, id: &str) -> Result<LinkReport> {
        self.transactional(|studio, config| {
            let selection = current_selection(config, id, deployed_agents(config, id)?)?;
            studio.write_project(config, id, Some(selection))
        })
    }

    /// 保存项目成员并写入已启用的 Agent；还没有启用任何 Agent 时只保存。`expected` 是打开
    /// 详情时的成员，与配置里的不同说明别处刚改过，拒绝保存以免覆盖
    pub fn save_project(
        &self,
        id: &str,
        members: ProjectMembers,
        expected: Option<ProjectMembers>,
    ) -> Result<LinkReport> {
        self.transactional(|studio, config| {
            let project = config
                .project(id)
                .ok_or_else(|| Error::NotFound(format!("项目 {id}")))?;
            if let Some(expected) = expected {
                if !expected.matches(project) {
                    // 不加「无效输入」前缀：界面显示「写入失败，修改已保留：项目已被别处修改…」
                    return Err(Error::Other(format!("项目{CHANGED_ELSEWHERE}")));
                }
            }
            let selection = ProjectSelection {
                agent_ids: deployed_agents(config, id)?,
                skill_ids: members.skill_ids,
                group_ids: members.group_ids,
                link_mode: members.link_mode,
            };
            studio.write_project(config, id, Some(selection))
        })
    }

    /// Restore under the same write lock as every other configuration mutation.
    pub fn restore_backup(&self, path: &std::path::Path) -> Result<AppConfig> {
        let mut guard = self.config_mut()?;
        if !guard.active_groups.is_empty() || !guard.policy_suspensions.is_empty() {
            return Err(Error::invalid(
                "请先开启保留手动 skill 并停用所有分组，再恢复配置备份",
            ));
        }
        let restored = self.studio.store().read_backup(path)?;
        let mcp_enabled = if self.mode == Mode::Recovery {
            !skill_studio_mcp::management::read(&self.studio.store().dir().join("mcp"))
                .map_err(|error| Error::invalid(error.to_string()))?
                .suspended
        } else {
            guard.settings.manage_mcp
        };
        if restored.settings.manage_mcp != mcp_enabled {
            return Err(Error::invalid(
                "备份中的 MCP 管理状态与当前接入不一致，请先通过设置开关切换，再恢复此备份",
            ));
        }
        if !restored.active_groups.is_empty() || !restored.policy_suspensions.is_empty() {
            return Err(Error::invalid(
                "此备份包含运行中的分组，不能只恢复配置；请选择停用分组后生成的备份",
            ));
        }
        self.studio.save_config(&restored)?;
        *guard = restored.clone();
        Ok(restored)
    }

    /// Hub adoption persists its own filesystem/config transaction under the write lock.
    pub fn adopt_to_hub(&self, skill_id: &str) -> Result<skill_studio_core::models::skill::Skill> {
        self.transactional(|studio, config| studio.adopt_to_hub(config, skill_id))
    }

    pub fn install_catalog_skill(
        &self,
        prepared: &skill_studio_core::services::marketplace::PreparedSkill,
    ) -> Result<skill_studio_core::models::skill::Skill> {
        self.transactional(|studio, config| studio.install_catalog_skill(config, prepared))
    }

    pub fn import_local_skill(
        &self,
        prepared: &skill_studio_core::services::marketplace::PreparedSkill,
    ) -> Result<skill_studio_core::services::marketplace::ImportOutcome> {
        self.transactional(|studio, config| studio.import_local_skill(config, prepared))
    }

    pub fn release_from_hub(
        &self,
        skill_id: &str,
    ) -> Result<skill_studio_core::models::skill::Skill> {
        self.transactional(|studio, config| studio.release_from_hub(config, skill_id))
    }

    pub fn set_manual_skill_policy(&self, preserve: bool) -> Result<()> {
        self.transactional(|studio, config| {
            let mut next = config.clone();
            next.settings.preserve_manual_skills = preserve;
            studio.reconcile_manual_policy(&mut next, true)?;
            *config = next;
            Ok(())
        })
    }

    pub fn set_mcp_management(&self, enabled: bool) -> Result<()> {
        self.ensure_writable()?;
        // 全局加锁顺序：配置锁 → MCP 锁。MCP 一侧持有管理锁时不要再读配置，否则两边
        // 反向等待（service::mcp::groups 用设置快照就是这个原因）
        let mut guard = self.config_mut()?;
        if guard.settings.manage_mcp == enabled {
            return Ok(());
        }
        let dir = self.studio.store().dir().join("mcp");
        skill_studio_mcp::management::set_enabled(&dir, enabled).map_err(|error| {
            Error::invalid(format!(
                "无法{} MCP 管理：{error:#}。开关保持原状态，请核对配置文件后重试",
                if enabled { "开启" } else { "关闭" }
            ))
        })?;
        let mut next = (*guard).clone();
        next.settings.manage_mcp = enabled;
        if let Err(error) = self.studio.save_config(&next) {
            if let Err(rollback) = skill_studio_mcp::management::set_enabled(&dir, !enabled) {
                return Err(Error::invalid(format!(
                    "保存 MCP 管理开关失败：{error}；回滚 MCP 配置也失败：{rollback}"
                )));
            }
            return Err(error);
        }
        *guard = next;
        // 开关管理也会写 Agent 配置并留下备份；清理失败不影响开关本身
        let _ = skill_studio_mcp::management::backups::prune(&dir, guard.settings.backup_keep);
        Ok(())
    }

    /// 扫描 skill。可写时先按「保留手动 skill」校正 Agent 原生开关；只读时只扫描。
    ///
    /// 等不到写锁（别的进程正在大量部署）时退回只读扫描：界面刷新不该因为一次排队失败
    /// 冒出错误，校正留给下一次
    pub fn scan_with_policy(&self) -> Result<Vec<skill_studio_core::services::studio::SkillView>> {
        if self.mode == Mode::ReadOnly {
            let config = self.config();
            return self.studio.scan_skills(&config);
        }
        let scan = self.transactional(|studio, config| {
            studio.reconcile_manual_policy(config, false)?;
            studio.scan_skills(config)
        });
        match scan {
            Err(error) if error.is_config_busy() => {
                log::warn!("配置正被其他程序修改，这次只读扫描");
                let config = self.config();
                self.studio.scan_skills(&config)
            }
            other => other,
        }
    }

    pub fn set_managed_agents(&self, disabled: &[String]) -> Result<()> {
        use std::collections::HashSet;
        self.transactional(|studio, config| {
            for id in disabled {
                skill_studio_core::models::agent::require_agent(id)?;
            }
            let old: HashSet<_> = config.settings.disabled_agents.iter().cloned().collect();
            let new: HashSet<_> = disabled.iter().cloned().collect();
            let changed: Vec<_> = old.symmetric_difference(&new).cloned().collect();
            if changed.len() > 1 {
                return Err(Error::invalid("请逐个切换应用管理状态"));
            }
            if let Some(id) = changed.first() {
                studio.set_agent_management(config, id, !new.contains(id))?;
            }
            Ok(())
        })
    }

    /// 设置这个 agent 当前叠加的全部分组；空列表表示全部停用。返回原样留下的副本
    pub fn activate_agent_groups(
        &self,
        agent_id: &str,
        group_ids: &[String],
    ) -> Result<Vec<UntouchedCopy>> {
        self.transactional(|studio, config| {
            studio.activate_agent_groups(config, agent_id, group_ids)
        })
    }

    /// 为一个 Agent 启用或停用一个分组，它叠加的其他分组保持原样。已经是目标状态时什么也
    /// 不做。在写锁内按最新的启用记录计算，不会撤掉别处（另一个窗口、命令行）刚启用的分组
    pub fn set_agent_group(
        &self,
        agent_id: &str,
        group_id: &str,
        enabled: bool,
    ) -> Result<Vec<UntouchedCopy>> {
        self.transactional(|studio, config| {
            let mut ids: Vec<String> = config
                .active_groups_of(agent_id)
                .iter()
                .map(|g| g.group_id.clone())
                .collect();
            if ids.iter().any(|id| id == group_id) == enabled {
                return Ok(Vec::new());
            }
            if enabled {
                ids.push(group_id.to_string());
            } else {
                ids.retain(|id| id != group_id);
            }
            studio.activate_agent_groups(config, agent_id, &ids)
        })
    }

    /// 按最新的启用记录重新写入这个 Agent 叠加的全部分组（应用修改）
    pub fn reapply_agent_groups(&self, agent_id: &str) -> Result<Vec<UntouchedCopy>> {
        self.transactional(|studio, config| {
            let ids: Vec<String> = config
                .active_groups_of(agent_id)
                .iter()
                .map(|g| g.group_id.clone())
                .collect();
            studio.activate_agent_groups(config, agent_id, &ids)
        })
    }

    /// Native settings writes share the configuration write lock to prevent two
    /// toggles from replacing each other's read/modify/write results.
    pub fn with_exclusive<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Studio, &AppConfig) -> Result<T>,
    {
        self.ensure_writable()?;
        let guard = self.config_mut()?;
        f(&self.studio, &guard)
    }
}

fn read(shared: &RwLock<Shared>) -> RwLockReadGuard<'_, Shared> {
    shared.read().unwrap_or_else(|poisoned| {
        log::warn!("配置读锁被毒化，已恢复");
        poisoned.into_inner()
    })
}

/// 项目当前写入了哪些 Agent：以项目目录里实际的部署为准，与卡片上的图标一致
fn deployed_agents(config: &AppConfig, id: &str) -> Result<Vec<String>> {
    let project = config
        .project(id)
        .ok_or_else(|| Error::NotFound(format!("项目 {id}")))?;
    Ok(
        skill_studio_core::services::project_write::project_deployments(project)
            .into_iter()
            .map(|d| d.agent_id)
            .collect(),
    )
}

fn current_selection(
    config: &AppConfig,
    id: &str,
    agent_ids: Vec<String>,
) -> Result<ProjectSelection> {
    let project = config
        .project(id)
        .ok_or_else(|| Error::NotFound(format!("项目 {id}")))?;
    Ok(ProjectSelection {
        agent_ids,
        skill_ids: project.skill_ids.clone(),
        group_ids: project.group_ids.clone(),
        link_mode: project.link_mode,
    })
}

/// 项目成员：单独选的 skill、绑定的分组与写入方式
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMembers {
    pub skill_ids: Vec<String>,
    pub group_ids: Vec<String>,
    pub link_mode: skill_studio_core::models::skill::LinkMode,
}

impl ProjectMembers {
    /// 顺序不算修改
    fn matches(&self, project: &skill_studio_core::models::project::ProjectBinding) -> bool {
        same_members(&self.skill_ids, &project.skill_ids)
            && same_members(&self.group_ids, &project.group_ids)
            && self.link_mode == project.link_mode
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
        let backup = state
            .studio()
            .store()
            .backups_dir()
            .join("config-test-restore.json");
        std::fs::create_dir_all(backup.parent().unwrap()).unwrap();
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

#[cfg(test)]
mod cache_tests {
    use super::*;
    use skill_studio_core::models::group::Group;

    /// 不经启动流程，避免扫描 Agent 目录：内存里是空配置，磁盘上还没有文件
    fn process(store: &Store) -> AppState {
        AppState::new(
            Studio::new(store.clone()),
            AppConfig::default(),
            Disk::Missing,
            Mode::Normal,
        )
    }

    fn add(state: &AppState, name: &str) {
        state
            .mutate(|_, config| {
                config.groups.push(Group::new(name.into(), name.into()));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn a_busy_cache_still_reads_what_another_process_wrote_and_an_idle_one_is_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path().to_path_buf());
        let desktop = process(&store);
        let cli = process(&store);
        add(&cli, "命令行建的");
        // 另一个线程正读着内存里的旧配置：没法就地换，这次读取另读一份
        let busy = read(&desktop.shared);
        assert_eq!(desktop.config().groups.len(), 1);
        assert!(busy.config.groups.is_empty());
        drop(busy);
        // 内存空闲时就地换成磁盘上的新配置
        assert_eq!(desktop.config().groups.len(), 1);
        assert_eq!(read(&desktop.shared).config.groups.len(), 1);
        assert!(desktop.take_external_change());
    }
}
