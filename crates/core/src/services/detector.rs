//! 探测本机装了哪些 agent。
//!
//! 两条独立线索，缺一不可：
//! 1. **配置目录是否存在** —— 决定我们能不能往它的 skills 目录写东西
//! 2. **CLI 是否可执行** —— 只用于 UI 提示版本，不作为可写判据
//!
//! CLI 结果是三态而非布尔：cc-switch 特意区分了"装了但跑不起来"（例如 Node
//! 版本不达标），这样前端不必去匹配错误文案反推语义。

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::fs::paths;
use crate::models::agent::{AgentDescriptor, AgentInfo, ToggleMechanism, AGENTS};

/// `--version` 的最长等待时间。坏掉的 CLI 可能挂住不返回。
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// CLI 探测的三态结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliProbe {
    /// 找到并成功返回版本
    Found(String),
    /// 找到可执行文件，但 `--version` 非零退出或超时
    FoundButBroken,
    /// 没找到
    NotFound,
}

/// 除 PATH 之外还要搜的常见安装位置。
fn extra_search_dirs() -> Vec<PathBuf> {
    let home = paths::home_dir();
    let mut dirs = vec![
        home.join(".local/bin"),
        home.join(".opencode/bin"),
        home.join(".grok/bin"),
        home.join(".npm-global/bin"),
        home.join(".volta/bin"),
        home.join("n/bin"),
        home.join(".bun/bin"),
    ];
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/usr/bin"));
    }
    #[cfg(windows)]
    {
        dirs.push(home.join("AppData/Roaming/npm"));
        dirs.push(home.join("AppData/Local/Programs"));
    }
    dirs
}

#[cfg(windows)]
const EXE_SUFFIXES: &[&str] = &[".cmd", ".exe", ".bat", ""];
#[cfg(not(windows))]
const EXE_SUFFIXES: &[&str] = &[""];

/// Resolve a native CLI without running it; plugin operations must use the same GUI search roots.
pub fn find_cli(command: &str) -> Option<PathBuf> {
    let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    directories.extend(extra_search_dirs());
    directories
        .into_iter()
        .flat_map(|dir| {
            EXE_SUFFIXES
                .iter()
                .map(move |suffix| dir.join(format!("{command}{suffix}")))
        })
        .find(|path| path.is_file())
}

/// 跑 `<cmd> --version`，带超时。
fn probe_version(program: &std::path::Path) -> CliProbe {
    let mut child = match Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return CliProbe::NotFound,
        // 权限问题等：文件在但跑不起来
        Err(_) => return CliProbe::FoundButBroken,
    };

    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output().ok();
                if !status.success() {
                    return CliProbe::FoundButBroken;
                }
                let text = output
                    .map(|o| {
                        let mut s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        if s.is_empty() {
                            s = String::from_utf8_lossy(&o.stderr).trim().to_string();
                        }
                        s
                    })
                    .unwrap_or_default();
                let version = text.lines().next().unwrap_or("").trim().to_string();
                return if version.is_empty() {
                    CliProbe::FoundButBroken
                } else {
                    CliProbe::Found(version)
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return CliProbe::FoundButBroken;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return CliProbe::FoundButBroken,
        }
    }
}

/// 先按 PATH 跑，未命中再扫常见安装目录。
pub fn probe_cli(command: &str) -> CliProbe {
    match probe_version(std::path::Path::new(command)) {
        CliProbe::NotFound => {}
        found => return found,
    }
    for dir in extra_search_dirs() {
        for suffix in EXE_SUFFIXES {
            let candidate = dir.join(format!("{command}{suffix}"));
            if !candidate.is_file() {
                continue;
            }
            match probe_version(&candidate) {
                CliProbe::NotFound => continue,
                found => return found,
            }
        }
    }
    CliProbe::NotFound
}

/// 汇总一个 agent 的运行时状态。
///
/// `probe_cli` 会起子进程，成本不低；`skip_cli_probe` 为 true 时只做目录判定，
/// 用于需要快速刷新的场景。
pub fn describe_agent(
    agent: &AgentDescriptor,
    overrides: &HashMap<String, PathBuf>,
    skip_cli_probe: bool,
) -> AgentInfo {
    let config_dir = agent.resolved_config_dir(overrides);
    let global_skill_dirs = agent.resolved_global_roots(overrides);

    // Shared skills are consumable by several Agents, but do not prove any one is installed.
    let detected = config_dir.is_dir()
        || agent
            .global_roots
            .iter()
            .zip(&global_skill_dirs)
            .any(|(root, path)| !root.shared && path.is_dir())
        || global_skill_dirs
            .iter()
            .skip(agent.global_roots.len())
            .any(|path| path.is_dir());

    let probe = if skip_cli_probe {
        CliProbe::NotFound
    } else {
        probe_cli(agent.cli_command)
    };

    AgentInfo {
        id: agent.id.to_string(),
        display_name: agent.display_name.to_string(),
        detected: detected || !matches!(probe, CliProbe::NotFound),
        cli_available: matches!(probe, CliProbe::Found(_)),
        cli_broken: matches!(probe, CliProbe::FoundButBroken),
        cli_version: match probe {
            CliProbe::Found(v) => Some(v),
            _ => None,
        },
        config_dir,
        global_skill_dirs,
        supports_project_skills: agent.project_skill_dir.is_some(),
        project_skill_dir: agent.project_skill_dir.map(str::to_string),
        supports_native_toggle: !matches!(agent.toggle, ToggleMechanism::None),
    }
}

/// 汇总全部 agent
pub fn describe_all(overrides: &HashMap<String, PathBuf>, skip_cli_probe: bool) -> Vec<AgentInfo> {
    AGENTS
        .iter()
        .map(|a| describe_agent(a, overrides, skip_cli_probe))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::agent::find_agent;
    use serial_test::serial;

    #[test]
    fn missing_command_reports_not_found() {
        assert_eq!(
            probe_cli("skill-studio-definitely-not-a-real-binary"),
            CliProbe::NotFound
        );
    }

    #[test]
    fn known_command_reports_a_version() {
        // `git --version` 在开发机和 CI 上都必然存在
        match probe_cli("git") {
            CliProbe::Found(v) => assert!(v.to_lowercase().contains("git"), "{v}"),
            other => panic!("git 应可探测到版本，实际 {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_exit_is_classified_as_broken() {
        // `false --version` 一定非零退出 —— 对应"装了但跑不起来"
        assert_eq!(probe_cli("false"), CliProbe::FoundButBroken);
    }

    #[test]
    #[serial]
    fn undetected_when_nothing_exists_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, dir.path());
        std::env::remove_var("CLAUDE_CONFIG_DIR");

        let claude = find_agent("claude-code").unwrap();
        let info = describe_agent(claude, &HashMap::new(), true);
        assert!(!info.detected);
        assert!(info.supports_project_skills);
        assert!(info.supports_native_toggle);
        assert_eq!(info.config_dir, dir.path().join(".claude"));

        std::env::remove_var(paths::TEST_HOME_ENV);
    }

    #[test]
    #[serial]
    fn shared_skills_do_not_imply_an_agent_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, dir.path());
        std::env::remove_var("CODEX_HOME");
        // 只建共享根 ~/.agents/skills，不建 ~/.codex
        std::fs::create_dir_all(dir.path().join(".agents/skills")).unwrap();

        let codex = find_agent("codex").unwrap();
        let info = describe_agent(codex, &HashMap::new(), true);
        assert!(!info.detected, "共享根不能用来判断 Agent 已安装");
        assert_eq!(info.global_skill_dirs.len(), 2);

        std::env::remove_var(paths::TEST_HOME_ENV);
    }

    #[test]
    #[serial]
    fn describe_all_covers_every_registered_agent() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, dir.path());
        let all = describe_all(&HashMap::new(), true);
        assert_eq!(all.len(), AGENTS.len());
        std::env::remove_var(paths::TEST_HOME_ENV);
    }
}
