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
