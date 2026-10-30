use serial_test::serial;

#[test]
#[serial]
fn generated_reference_has_no_trailing_whitespace_and_never_writes_config() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var(skill_studio_core::fs::paths::TEST_HOME_ENV, home.path());
    let mut out = vec![];
    let mut err = vec![];
    let code = skill_studio_cli::run(["skill-studio", "docs"], &mut out, &mut err);
    std::env::remove_var(skill_studio_core::fs::paths::TEST_HOME_ENV);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    assert!(!home.path().join(".skill-studio").exists());
    let output = String::from_utf8(out).unwrap();
    for (index, line) in output.lines().enumerate() {
        assert_eq!(line, line.trim_end(), "第 {} 行有尾随空白", index + 1);
    }
}
