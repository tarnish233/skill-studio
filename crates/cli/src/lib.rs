//! Skill Studio 命令行。
//!
//! 给 Agent 和脚本用：与桌面应用共用同一套服务层，直接读写 `~/.skill-studio`。多个进程
//! 同时操作时由配置锁保证不互相覆盖；查看类命令只读打开，不写任何文件。

mod args;
mod commands;
mod error;
mod redact;
mod report;
mod resolve;
mod serve;
mod view;

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use serde_json::json;
use skill_studio_core::models::agent::AGENTS;
use skill_studio_core::services::store::{lock_existing, Store};

use redact::{hide, secret_name, HIDDEN};

pub use error::{CliError, Kind};

/// 等别的进程写完配置的上限
const LOCK_WAIT: Duration = Duration::from_secs(30);

pub fn main() -> i32 {
    let args: Vec<OsString> = std::env::args_os().collect();
    // 桌面应用经 SSH 管理服务器时的协议模式，不经过命令解析
    if args.get(1).is_some_and(|a| a == "serve") {
        return serve::main(&args[2..]);
    }
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    run(args, &mut out, &mut err)
}

/// 执行一条命令，返回退出码。结果写到 `out`，错误写到 `err`
pub fn run<I, T>(args: I, out: &mut dyn Write, err: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let json = args.iter().skip(1).any(|a| a == "--json");
    let cli = match args::parse(&args) {
        Ok(cli) => cli,
        Err(args::Exit::Print(text, code)) => {
            let _ = write!(out, "{text}");
            return code;
        }
        Err(args::Exit::Usage(error)) => {
            print_error(err, json, &error);
            return error.exit_code();
        }
    };
    let store = Store::default();
    if let Err(error) = ensure_cli_enabled(&store) {
        print_error(err, cli.json, &error);
        return error.exit_code();
    }
    let ctx = commands::Ctx {
        yes: cli.yes,
        store,
        wait: lock_wait(),
        ignored_env: isolate_agent_env(),
    };
    // MCP 托管状态有自己的锁，等别的进程写完的上限与配置锁一致
    skill_studio_mcp::management::set_lock_wait(ctx.wait);
    let result = commands::run(&cli.command, &ctx);
    if cli.command.writes() {
        audit(&ctx, &args, &result);
    }
    match result {
        Ok(report) => {
            if cli.json {
                let _ = writeln!(out, "{}", report.json);
            } else {
                let _ = writeln!(out, "{}", report.text);
            }
            report.exit
        }
        Err(error) => {
            print_error(err, cli.json, &error);
            error.exit_code()
        }
    }
}

pub(crate) fn cli_disabled() -> CliError {
    CliError::failed("命令行工具已在 Skill Studio 设置中关闭")
        .hint("前往 Skill Studio 设置 › 通用重新开启「启用 CLI 与内置 skill」")
}

fn ensure_cli_enabled(store: &Store) -> Result<(), CliError> {
    // This is a usage preference, not a security boundary. `serve` is used by
    // the desktop over SSH and intentionally bypasses the interactive CLI.
    // A read-only check never creates a config directory or recovery journal.
    let _lock = lock_existing(&store.lock_path(), lock_wait(), false)?;
    let config = store.read()?;
    if config.is_none() && store.pending_transactions() {
        return Err(CliError::failed(
            "Skill Studio 有一项写入还没完成，请先打开 Skill Studio 完成恢复，再查看",
        ));
    }
    if config.is_some_and(|config| !config.settings.cli_skill_enabled) {
        return Err(cli_disabled());
    }
    Ok(())
}

fn print_error(err: &mut dyn Write, json: bool, error: &CliError) {
    if json {
        let _ = writeln!(
            err,
            "{}",
            json!({ "error": {
                "code": error.code(),
                "message": error.message,
                "hint": error.hint,
                "candidates": error.candidates,
            }})
        );
        return;
    }
    let _ = writeln!(err, "错误：{}", error.message);
    if !error.candidates.is_empty() {
        let _ = writeln!(err, "可选：");
        for candidate in &error.candidates {
            let _ = writeln!(err, "  {candidate}");
        }
    }
    if let Some(hint) = &error.hint {
        let _ = writeln!(err, "提示：{hint}");
    }
}

fn lock_wait() -> Duration {
    std::env::var("SKILL_STUDIO_LOCK_WAIT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(LOCK_WAIT)
}

/// 从程序坞打开的桌面应用看不到 shell 里设置的 `CODEX_HOME` 这类变量；命令行若照着
/// 它们解析，就会和桌面应用管理不同的目录。按桌面应用的方式解析（以设置里的「Agent
/// 目录覆盖」为准），被忽略的变量交给 doctor 提示。
fn isolate_agent_env() -> Vec<(String, String)> {
    let mut names: Vec<&str> = AGENTS
        .iter()
        .flat_map(|a| [a.home_env, a.extra_config_env])
        .flatten()
        .collect();
    names.push("OPENCODE_CONFIG");
    let mut ignored = Vec::new();
    for name in names {
        if let Some(value) = std::env::var_os(name) {
            ignored.push((name.to_string(), value.to_string_lossy().into_owned()));
            std::env::remove_var(name);
        }
    }
    ignored
}

/// 尽量认出是哪个 Agent 在调用，写进日志方便事后核对
fn caller() -> &'static str {
    let set = |name| std::env::var_os(name).is_some();
    if set("CLAUDECODE") {
        "Claude Code"
    } else if set("CODEX_SANDBOX") || set("CODEX_SANDBOX_NETWORK_DISABLED") {
        "Codex"
    } else if set("PI_CODING_AGENT") {
        "Pi"
    } else {
        "终端"
    }
}

/// 修改命令记到 `~/.skill-studio/logs/cli.log`：谁、什么时候、做了什么、结果如何。
/// 记不下来不影响命令本身
fn audit(ctx: &commands::Ctx, args: &[OsString], result: &Result<report::Report, CliError>) {
    let command = logged(args)
        .into_iter()
        .map(|a| {
            if a.chars().any(char::is_whitespace) {
                format!("\"{a}\"")
            } else {
                a
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let outcome = match result {
        Ok(report) => report.json["message"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| "完成".into()),
        Err(error) => format!("失败：{}", error.message),
    };
    let line = format!(
        "{} [{}] skill-studio {command} → {outcome}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        caller()
    );
    let dir = ctx.store.dir().join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("cli.log");
    if path.metadata().is_ok_and(|m| m.len() > 1024 * 1024) {
        let _ = std::fs::rename(&path, dir.join("cli.log.1"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// 记进日志的命令行：环境变量、请求头、粘贴的配置和网址参数都可能带密钥，只留名字
fn logged(args: &[OsString]) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending: Option<&str> = None;
    let mut trailing = false;
    let mut hide_next = false;
    for arg in args
        .iter()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
    {
        if let Some(flag) = pending.take() {
            out.push(hide(flag, &arg));
            continue;
        }
        if trailing {
            // -- 之后是 MCP 的启动命令：像令牌的参数照样隐藏
            if hide_next {
                out.push(HIDDEN.into());
                hide_next = false;
            } else if let Some((flag, _)) = arg
                .split_once('=')
                .filter(|(f, _)| f.starts_with('-') && secret_name(f))
            {
                out.push(format!("{flag}={HIDDEN}"));
            } else {
                hide_next = arg.starts_with('-') && secret_name(&arg);
                out.push(arg);
            }
            continue;
        }
        match arg.as_str() {
            "--" => {
                trailing = true;
                out.push(arg);
            }
            "--unset-env" | "--unset-header" => {
                // These flags take names, but a typo can put an entire credential there.
                pending = Some("--config");
                out.push(arg);
            }
            "--env" | "-e" | "--header" | "-H" | "--config" | "--url" => {
                pending = Some(match arg.as_str() {
                    "--env" | "-e" => "--env",
                    "--header" | "-H" => "--header",
                    "--config" => "--config",
                    _ => "--url",
                });
                out.push(arg);
            }
            _ => match arg.split_once('=') {
                Some((flag @ ("--env" | "--header" | "--config" | "--url"), value)) => {
                    out.push(format!("{flag}={}", hide(flag, value)))
                }
                Some((flag @ ("--unset-env" | "--unset-header"), _)) => {
                    out.push(format!("{flag}={HIDDEN}"))
                }
                // clap 也接受短选项紧跟值：-eTOKEN=…、-HAuthorization: …、-e=TOKEN=…
                _ => match attached(&arg) {
                    Some((flag, value)) => out.push(format!("{flag}{value}")),
                    None => out.push(arg),
                },
            },
        }
    }
    out
}

/// `-e值`、`-e=值`、`-H值`、`-H=值`：返回原样的选项和已经隐藏好的值
fn attached(arg: &str) -> Option<(&'static str, String)> {
    let (flag, canonical) = if arg.starts_with("-e") && !arg.starts_with("--") {
        ("-e", "--env")
    } else if arg.starts_with("-H") && !arg.starts_with("--") {
        ("-H", "--header")
    } else {
        return None;
    };
    let value = &arg[2..];
    let value = value.strip_prefix('=').unwrap_or(value);
    // 只有选项本身、没有值时，交给上面等下一个参数的逻辑
    (!value.is_empty()).then(|| (flag, hide(canonical, value)))
}

#[cfg(test)]
mod tests {
    use super::logged;

    #[test]
    fn secrets_never_reach_the_command_log() {
        let args: Vec<std::ffi::OsString> = [
            "skill-studio",
            "mcp",
            "add",
            "gh",
            "-e",
            "GITHUB_TOKEN=ghp_1",
            "--env=B=2",
            "-H",
            "Authorization: Bearer x",
            "--url",
            "https://a.dev/mcp?key=k",
            "--config",
            "{\"t\":\"s\"}",
            "--",
            "npx",
            "server",
            "--api-key",
            "sk-9",
            "--token=t-1",
            "--credentials",
            "sk-cred",
            "--port",
            "80",
        ]
        .iter()
        .map(Into::into)
        .collect();
        let line = logged(&args).join(" ");
        for secret in [
            "ghp_1", "=2", "Bearer x", "key=k", "\"s\"", "sk-9", "t-1", "sk-cred",
        ] {
            assert!(!line.contains(secret), "{secret}：{line}");
        }
        assert!(line.contains("GITHUB_TOKEN=<已隐藏>"), "{line}");
        assert!(line.contains("Authorization: <已隐藏>"), "{line}");
        assert!(line.contains("https://a.dev/mcp?<已隐藏>"), "{line}");
        // 其他参数原样记下，看得出做了什么
        assert!(line.starts_with("mcp add gh -e"), "{line}");
        assert!(line.ends_with("--port 80"), "{line}");
    }

    /// clap 也接受短选项紧跟值，这三种写法原来会原样进日志
    #[test]
    fn secrets_attached_to_a_short_option_never_reach_the_command_log() {
        let args: Vec<std::ffi::OsString> = [
            "skill-studio",
            "mcp",
            "add",
            "gh",
            "-eTOKEN=ghp_attached",
            "-HAuthorization: Bearer attached",
            "-e=API=also-attached",
            "--",
            "npx",
            "server",
        ]
        .iter()
        .map(Into::into)
        .collect();
        let line = logged(&args).join(" ");
        for secret in ["ghp_attached", "Bearer attached", "also-attached"] {
            assert!(!line.contains(secret), "{secret}：{line}");
        }
        assert!(line.contains("-eTOKEN=<已隐藏>"), "{line}");
        assert!(line.contains("-HAuthorization: <已隐藏>"), "{line}");
        assert!(line.contains("-eAPI=<已隐藏>"), "{line}");
    }
}
