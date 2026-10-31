//! 新接入的备份/项目辅助写入也须在配置锁内复核 CLI 开关。
use serial_test::serial;
use skill_studio_core::{fs::paths, services::store::Store};
use skill_studio_service::{mcp, projects, settings, state::AppState};

struct Env(tempfile::TempDir);
impl Env {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, temp.path());
        std::env::remove_var("DSH_HOME");
        std::env::remove_var("DSH_AGENTS_HOME");
        Self(temp)
    }
    fn state(&self) -> AppState {
        AppState::bootstrap(Store::new(self.0.path().join(".skill-studio"))).unwrap()
    }
    fn disable(&self) {
        self.state()
            .mutate(|_, cfg| {
                cfg.settings.cli_skill_enabled = false;
                Ok(())
            })
            .unwrap();
    }
}
impl Drop for Env {
    fn drop(&mut self) {
        std::env::remove_var(paths::TEST_HOME_ENV);
    }
}

#[test]
#[serial]
fn config_backup_delete_rechecks_cli_switch_at_write_time() {
    let env = Env::new();
    let cli = env.state().for_interactive_cli();
    cli.mutate(|_, cfg| {
        cfg.settings.theme = "dark".into();
        Ok(())
    })
    .unwrap();
    env.disable();
    let backup = cli
        .studio()
        .store()
        .list_backups()
        .into_iter()
        .next()
        .unwrap();
    let error = settings::delete_backup(&cli, backup.to_string_lossy().into()).unwrap_err();
    assert!(error.contains("设置中关闭"), "{error}");
    assert!(backup.exists());
}

#[test]
#[serial]
fn gitignore_write_rechecks_cli_switch_at_write_time() {
    let env = Env::new();
    let cli = env.state().for_interactive_cli();
    let root = env.0.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let project = projects::create_project(&cli, "app".into(), root.clone()).unwrap();
    env.disable();
    let error = projects::write_project_gitignore(&cli, project.id).unwrap_err();
    assert!(error.contains("设置中关闭"), "{error}");
    assert!(!root.join(".gitignore").exists());
}

#[test]
#[serial]
fn mcp_backup_delete_rechecks_cli_switch_at_write_time() {
    let env = Env::new();
    let cli = env.state().for_interactive_cli();
    env.disable();
    let error = mcp::backups::delete(&cli, Some("all"), None).unwrap_err();
    assert!(error.contains("设置中关闭"), "{error}");
}
