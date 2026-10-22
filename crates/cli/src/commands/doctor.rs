use std::fmt::Write as _;
use std::time::Duration;

use serde_json::json;
use skill_studio_core::models::config::CONFIG_VERSION;

use super::status::{attention, load_mcp};
use super::Ctx;
use crate::error::CliError;
use crate::report::{home, Report};
use crate::view;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Ok,
    Warn,
    Error,
}

struct Check {
    level: Level,
    title: &'static str,
    detail: String,
}

/// 只读：不恢复事务、不写任何文件，在只允许读的沙箱里也能跑
pub fn run(ctx: &Ctx) -> Result<Report, CliError> {
    let store = &ctx.store;
    let mut checks = Vec::new();
    let mut push = |level, title, detail: String| {
        checks.push(Check {
            level,
            title,
            detail,
        })
    };

    push(
        Level::Ok,
        "命令行",
        format!(
            "skill-studio {}，配置目录 {}",
            env!("CARGO_PKG_VERSION"),
            home(store.dir())
        ),
    );
    let readable = match store.read() {
        Ok(Some(config)) => {
            push(
                Level::Ok,
                "配置",
                format!(
                    "可以读取（格式版本 {}，本程序支持 {CONFIG_VERSION}）",
                    config.version
                ),
            );
            true
        }
        Ok(None) if store.pending_transactions() => {
            push(
                Level::Error,
                "配置",
                "配置文件不在，且有一项写入没有完成：打开 Skill Studio 会自动恢复".into(),
            );
            false
        }
        Ok(None) => {
            push(Level::Ok, "配置", "还没有配置，首次使用".into());
            true
        }
        Err(error) => {
            push(Level::Error, "配置", format!("读不出来：{error}"));
            false
        }
    };
    let mcp_pending = store.dir().join("mcp/management-transaction.json").exists();
    if store.pending_transactions() || mcp_pending {
        push(
            Level::Warn,
            "写入",
            "有一项写入没有完成（另一个程序正在写，或上次写到一半退出）；打开 Skill Studio 或运行任一修改命令会自动恢复".into(),
        );
    } else if store.lock_path().exists() && store.lock_shared(Duration::ZERO).is_none() {
        push(
            Level::Warn,
            "写入",
            "另一个程序正在修改配置，修改命令会等它写完".into(),
        );
    } else {
        push(Level::Ok, "写入", "没有进行中的写入".into());
    }
    if ctx.ignored_env.is_empty() {
        push(
            Level::Ok,
            "Agent 目录",
            "与 Skill Studio 桌面应用一致".into(),
        );
    }
    for (name, value) in &ctx.ignored_env {
        push(
            Level::Warn,
            "Agent 目录",
            format!(
                "已忽略环境变量 {name}={value}：从程序坞打开的 Skill Studio 看不到 shell 里设置的变量，\
                 命令行与它保持一致。如果 Agent 实际用的是这个目录，请在 Skill Studio 设置的「Agent 目录覆盖」里指定"
            ),
        );
    }
    let mut problems = Vec::new();
    if readable {
        let state = ctx.reader()?;
        let data = view::load(&state)?;
        let mcp = load_mcp(&state, &data);
        match &mcp {
            None => push(Level::Ok, "MCP", "管理已关闭".into()),
            Some(Ok(mcp)) => push(
                Level::Ok,
                "MCP",
                format!(
                    "可以读取（Hub 里 {} 个，MCP 分组 {} 个）",
                    mcp.rows.iter().filter(|r| r.managed).count(),
                    mcp.catalog.groups.len()
                ),
            ),
            Some(Err(error)) => push(Level::Warn, "MCP", format!("读不出来：{}", error.message)),
        }
        let found = attention(&data, mcp.as_ref().and_then(|m| m.as_ref().ok()));
        if found.issues.is_empty() && found.pending.is_empty() {
            push(
                Level::Ok,
                "分组与项目",
                "没有需要检查或有待应用的修改".into(),
            );
        } else {
            push(
                Level::Warn,
                "分组与项目",
                format!(
                    "{} 处需要检查，{} 处有待应用修改，详见下方",
                    found.issues.len(),
                    found.pending.len()
                ),
            );
            problems = found.issues.into_iter().chain(found.pending).collect();
        }
    }

    let mut text = String::new();
    for check in &checks {
        let mark = match check.level {
            Level::Ok => "✓",
            Level::Warn => "!",
            Level::Error => "✗",
        };
        let _ = writeln!(text, "{mark} {}：{}", check.title, check.detail);
    }
    for problem in &problems {
        let _ = writeln!(text, "    {problem}");
    }
    let failed = checks.iter().any(|c| c.level == Level::Error);
    let json = json!({
        "ok": !failed,
        "checks": checks.iter().map(|c| json!({
            "level": match c.level { Level::Ok => "ok", Level::Warn => "warn", Level::Error => "error" },
            "title": c.title, "detail": c.detail,
        })).collect::<Vec<_>>(),
        "problems": problems,
    });
    let mut report = Report::new(text.trim_end(), json);
    report.exit = if failed { 1 } else { 0 };
    Ok(report)
}
