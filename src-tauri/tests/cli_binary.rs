//! 随桌面应用打包的命令行程序：不启动界面、退出码与输出符合约定

use std::process::Command;

fn run(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_skill-studio-cli"))
        .args(args)
        .env("SKILL_STUDIO_TEST_HOME", home)
        .output()
        .unwrap()
}

#[test]
#[cfg(unix)]
fn stdio_server_stderr_cannot_leak_into_cli_json_or_logs() {
    let home = tempfile::tempdir().unwrap();
    let marker = "review-dummy-credential";
    let added = run(
        home.path(),
        &[
            "mcp",
            "add",
            "stderr-test",
            "--env",
            &format!("TOKEN={marker}"),
            "--",
            "sh",
            "-c",
            "printf '%s\\n' \"$TOKEN\" >&2; exit 1",
        ],
    );
    assert!(added.status.success());
    let failed = run(
        home.path(),
        &["--json", "mcp", "test", "stderr-test", "--timeout", "1"],
    );
    assert_eq!(failed.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(
        !stderr.contains(marker),
        "upstream stderr reached the CLI: {stderr}"
    );
    assert!(
        serde_json::from_slice::<serde_json::Value>(&failed.stderr).is_ok(),
        "stderr must be one JSON error: {stderr}"
    );
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
        .unwrap();
    let state = String::from_utf8_lossy(&output.stdout);
    !state.trim().is_empty() && !state.contains('Z')
}

#[test]
#[cfg(unix)]
fn a_timed_out_stdio_probe_does_not_leave_descendants_running() {
    let home = tempfile::tempdir().unwrap();
    let pidfile = home.path().join("child.pid");
    let script = "sleep 3 >/dev/null 2>&1 & printf '%s' \"$!\" > \"$PIDFILE\"; exec sleep 3";
    let added = run(
        home.path(),
        &[
            "mcp",
            "add",
            "timeout-test",
            "--env",
            &format!("PIDFILE={}", pidfile.display()),
            "--",
            "sh",
            "-c",
            script,
        ],
    );
    assert!(added.status.success());
    let failed = run(
        home.path(),
        &["mcp", "test", "timeout-test", "--timeout", "1"],
    );
    assert_eq!(failed.status.code(), Some(1));
    let pid = std::fs::read_to_string(pidfile)
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let alive_at_return = process_is_alive(pid);
    // The fixture is bounded even on the old implementation: wait for its natural exit before
    // reporting a failure, so a red regression run does not leave any process behind.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    while process_is_alive(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert!(
        !alive_at_return,
        "stdio descendant remained alive after the CLI returned"
    );
}

#[test]
#[cfg(unix)]
fn interrupted_cli_probes_cancel_without_leaving_their_process_group_running() {
    let home = tempfile::tempdir().unwrap();
    let pidfile = home.path().join("interrupt-child.pid");
    let script = "sleep 3 >/dev/null 2>&1 & printf '%s' \"$!\" > \"$PIDFILE\"; exec sleep 3";
    let added = run(
        home.path(),
        &[
            "mcp",
            "add",
            "interrupt-test",
            "--env",
            &format!("PIDFILE={}", pidfile.display()),
            "--",
            "sh",
            "-c",
            script,
        ],
    );
    assert!(added.status.success());
    let child = Command::new(env!("CARGO_BIN_EXE_skill-studio-cli"))
        .args(["--json", "mcp", "test", "interrupt-test", "--timeout", "10"])
        .env("SKILL_STUDIO_TEST_HOME", home.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !pidfile.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let pid = std::fs::read_to_string(&pidfile)
        .unwrap()
        .parse::<u32>()
        .unwrap();
    assert!(Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap()
        .success());
    let result = child.wait_with_output().unwrap();
    let alive_at_return = process_is_alive(pid);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    while process_is_alive(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert!(
        !alive_at_return,
        "SIGTERM left the stdio descendant running"
    );
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("已取消"));
}

#[test]
fn the_bundled_command_line_program_runs_without_the_desktop() {
    let home = tempfile::tempdir().unwrap();
    let version = run(home.path(), &["--version"]);
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("skill-studio {}", env!("CARGO_PKG_VERSION"))
    );

    let status = run(home.path(), &["--json", "status"]);
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    assert!(
        !home.path().join(".skill-studio").exists(),
        "查看命令不建任何文件"
    );

    let missing = run(home.path(), &["groups", "show", "没有的分组"]);
    assert_eq!(missing.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("找不到分组"));
}

#[test]
fn the_builtin_skill_reference_matches_the_command_line() {
    let home = tempfile::tempdir().unwrap();
    let output = run(home.path(), &["docs"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !home.path().join(".skill-studio").exists(),
        "生成参考不该碰用户的配置目录"
    );
    let generated = String::from_utf8(output.stdout)
        .unwrap()
        .replace("\r\n", "\n");
    let checked_in =
        include_str!("../resources/builtin-skills/skill-studio/references/commands.md")
            .replace("\r\n", "\n");
    assert_eq!(
        generated, checked_in,
        "命令行变了：运行 target/debug/skill-studio-cli docs > src-tauri/resources/builtin-skills/skill-studio/references/commands.md"
    );
}

#[test]
fn the_builtin_skill_stays_short_and_names_itself() {
    // Windows 上检出的资源是 CRLF，先归一化再比较
    let skill =
        include_str!("../resources/builtin-skills/skill-studio/SKILL.md").replace("\r\n", "\n");
    assert!(skill.starts_with("---\nname: skill-studio\n"), "{skill}");
    assert!(
        skill.lines().count() <= 150,
        "SKILL.md 太长（{} 行），细节放进 references/",
        skill.lines().count()
    );
    assert!(skill.contains("references/commands.md"), "{skill}");
}
