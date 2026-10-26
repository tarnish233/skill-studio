mod doctor;
mod groups;
mod mcp;
mod projects;
mod skills;
mod status;

use std::time::Duration;

use skill_studio_core::services::store::Store;
use skill_studio_service::state::AppState;

use crate::args::Command;
use crate::error::CliError;
use crate::report::Report;

pub struct Ctx {
    pub yes: bool,
    pub store: Store,
    pub wait: Duration,
    /// 被忽略的 Agent 目录环境变量，交给 doctor 提示
    pub ignored_env: Vec<(String, String)>,
}

impl Ctx {
    /// 查看：只读打开，不恢复事务、不写任何文件（Agent 沙箱里只能读时也能用）
    pub fn reader(&self) -> Result<AppState, CliError> {
        let state = AppState::read_only_waiting(self.store.clone(), self.wait)?;
        if !state.config().settings.cli_skill_enabled {
            return Err(crate::cli_disabled());
        }
        Ok(state)
    }

    /// 修改：和桌面应用启动时一样加载（含恢复中断的事务），写入时持配置锁
    pub fn writer(&self) -> Result<AppState, CliError> {
        let state = AppState::bootstrap_waiting(self.store.clone(), self.wait)?;
        if !state.config().settings.cli_skill_enabled {
            return Err(crate::cli_disabled());
        }
        Ok(state.for_interactive_cli())
    }
}

pub fn run(command: &Command, ctx: &Ctx) -> Result<Report, CliError> {
    match command {
        // 参考不需要配置和锁：生成内置 skill 的 commands.md 时也不该碰用户的目录
        Command::Docs => Ok(Report::new(crate::args::reference(), serde_json::json!({}))),
        Command::Status => status::status(ctx),
        Command::Agents { probe } => status::agents(ctx, *probe),
        Command::Skills(command) => skills::run(command, ctx),
        Command::Groups(command) => groups::run(command, ctx),
        Command::Projects(command) => projects::run(command, ctx),
        Command::Mcp(command) => mcp::run(command, ctx),
        Command::Doctor => doctor::run(ctx),
    }
}

/// 保存时发现别处刚改过：按最新内容重做一次
fn retry<T>(mut attempt: impl FnMut() -> Result<T, CliError>) -> Result<T, CliError> {
    match attempt() {
        Err(error) if error.changed_elsewhere() => attempt(),
        other => other,
    }
}
