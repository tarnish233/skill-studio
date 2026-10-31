//! 审查发现的边界：软链接不能变成删除目标、目录别名可用、未确认的备份设置必须检查快照。
use serial_test::serial;
use skill_studio_core::{fs::paths, services::store::Store};
use skill_studio_service::{
    settings::{self, SettingsPatch},
    state::AppState,
};
use std::path::PathBuf;

struct Home(tempfile::TempDir);
impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, dir.path());
        for name in [
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "XDG_CONFIG_HOME",
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_CONFIG",
            "PI_CODING_AGENT_DIR",
            "GROK_HOME",
            "DSH_HOME",
            "DSH_AGENTS_HOME",
            "SKILL_STUDIO_LOCK_WAIT_MS",
            "SKILL_STUDIO_APP_EXECUTABLE",
        ] {
            std::env::remove_var(name);
        }
        for name in [".claude", ".codex"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        Self(dir)
    }
    fn path(&self, path: &str) -> PathBuf {
        self.0.path().join(path)
    }
    #[cfg(unix)]
    fn skill(&self, path: &str, name: &str) -> PathBuf {
        let path = self.path(path);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("SKILL.md"),
            format!("---\nname: {name}\n---\nbody\n"),
        )
        .unwrap();
        path
    }
    #[cfg(unix)]
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let mut out = vec![];
        let mut err = vec![];
        let code = skill_studio_cli::run(
            std::iter::once("skill-studio").chain(args.iter().copied()),
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }
    #[cfg(unix)]
    fn ok(&self, args: &[&str]) -> String {
        let (code, out, err) = self.run(args);
        assert_eq!(code, 0, "{args:?}: {out}{err}");
        out
    }
    fn state(&self) -> AppState {
        AppState::bootstrap(Store::new(self.path(".skill-studio"))).unwrap()
    }
    #[cfg(unix)]
    fn project(&self) {
        std::fs::create_dir_all(self.path("work")).unwrap();
        self.ok(&[
            "projects",
            "create",
            self.path("work").to_str().unwrap(),
            "--name",
            "app",
        ]);
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        std::env::remove_var(paths::TEST_HOME_ENV);
    }
}

#[test]
#[serial]
#[cfg(unix)]
fn project_skill_names_do_not_silently_select_the_copy_under_the_current_directory() {
    struct WorkingDirectory(PathBuf);
    impl Drop for WorkingDirectory {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).unwrap();
        }
    }
    let home = Home::new();
    home.project();
    let claude = home.skill("work/.claude/skills/alpha", "alpha");
    let shared = home.skill("work/.agents/skills/alpha", "alpha");
    let _cwd = WorkingDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(claude.parent().unwrap()).unwrap();
    let (code, _, err) = home.run(&["projects", "skills", "delete", "app", "alpha", "--yes"]);
    assert_eq!(code, 3, "{err}");
    assert!(claude.join("SKILL.md").is_file() && shared.join("SKILL.md").is_file());
}

#[test]
#[serial]
#[cfg(unix)]
fn deleting_an_unlisted_leaf_symlink_never_deletes_the_hub_target() {
    let home = Home::new();
    let target = home.skill(".skill-studio/skills/alpha", "alpha");
    let alias = home.path("outside-alias");
    std::os::unix::fs::symlink(&target, &alias).unwrap();
    let (code, _, err) = home.run(&["skills", "delete", alias.to_str().unwrap(), "--yes"]);
    assert_eq!(code, 3, "{err}");
    assert!(target.join("SKILL.md").is_file());
    assert!(alias.symlink_metadata().is_ok());
}

#[test]
#[serial]
#[cfg(unix)]
fn parent_directory_aliases_still_select_real_hub_sources() {
    let home = Home::new();
    let target = home.skill(".skill-studio/skills/alpha", "alpha");
    let alias = home.path("hub-alias");
    std::os::unix::fs::symlink(home.path(".skill-studio/skills"), &alias).unwrap();
    home.ok(&[
        "skills",
        "delete",
        alias.join("alpha").to_str().unwrap(),
        "--yes",
    ]);
    assert!(!target.exists());
    assert!(alias.symlink_metadata().is_ok());
}

#[test]
#[serial]
#[cfg(unix)]
fn project_originals_can_be_selected_through_a_parent_directory_alias() {
    let home = Home::new();
    home.project();
    let original = home.skill("work/.claude/skills/manual", "manual");
    let alias = home.path("project-alias");
    std::os::unix::fs::symlink(home.path("work"), &alias).unwrap();
    let input = alias.join(".claude/skills/manual");
    home.ok(&[
        "projects",
        "skills",
        "disable",
        "app",
        input.to_str().unwrap(),
    ]);
    assert!(!original.exists());
    home.ok(&[
        "projects",
        "skills",
        "enable",
        "app",
        input.to_str().unwrap(),
    ]);
    assert!(original.join("SKILL.md").is_file());
}

#[test]
#[serial]
#[cfg(unix)]
fn deleting_a_listed_project_symlink_only_removes_that_link() {
    let home = Home::new();
    home.project();
    let target = home.skill("external/manual", "manual");
    std::fs::create_dir_all(home.path("work/.claude/skills")).unwrap();
    std::os::unix::fs::symlink(&target, home.path("work/.claude/skills/link")).unwrap();
    let listed: serde_json::Value =
        serde_json::from_str(&home.ok(&["--json", "projects", "skills", "list", "app"])).unwrap();
    let path = listed[0]["path"].as_str().unwrap();
    home.ok(&["projects", "skills", "delete", "app", path, "--yes"]);
    assert!(target.join("SKILL.md").is_file());
    assert!(home
        .path("work/.claude/skills/link")
        .symlink_metadata()
        .is_err());
}

#[test]
#[serial]
fn unconfirmed_backup_retention_does_not_apply_against_a_changed_snapshot() {
    let home = Home::new();
    let cli = home.state().for_interactive_cli();
    cli.mutate(|_, config| {
        config.settings.backup_keep = 10;
        Ok(())
    })
    .unwrap();
    let before = settings::get_settings(&cli).unwrap();
    home.state()
        .mutate(|_, config| {
            config.settings.backup_keep = 30;
            Ok(())
        })
        .unwrap();
    let store = cli.studio().store();
    let config = std::fs::read(store.config_path()).unwrap();
    let backups = store.list_backups();
    let failed = settings::update_settings_with_backup_snapshot(
        &cli,
        SettingsPatch {
            backup_keep: Some(20),
            ..Default::default()
        },
        Some(before.backup_keep),
    );
    let error = failed.unwrap_err();
    assert_eq!(skill_studio_cli::CliError::from(error).exit_code(), 4);
    assert_eq!(std::fs::read(store.config_path()).unwrap(), config);
    assert_eq!(store.list_backups(), backups);
}

#[test]
#[serial]
fn explicitly_confirmed_backup_retention_can_use_the_latest_configuration() {
    let home = Home::new();
    let cli = home.state().for_interactive_cli();
    cli.mutate(|_, config| {
        config.settings.backup_keep = 30;
        Ok(())
    })
    .unwrap();
    let after = settings::update_settings_with_backup_snapshot(
        &cli,
        SettingsPatch {
            backup_keep: Some(20),
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(after.backup_keep, 20);
}
