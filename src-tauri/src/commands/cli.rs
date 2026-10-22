//! 设置页「命令行工具」：App 自带的命令行程序在哪、终端里能不能直接运行。

use std::path::{Path, PathBuf};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInfo {
    /// App 自带的命令行程序（与主程序同目录）
    bundled: Option<PathBuf>,
    /// 终端里可以直接运行的命令名：skill-studio（Homebrew 链接），或程序本身的文件名
    command: Option<String>,
    /// 命令所在的位置
    location: Option<PathBuf>,
    /// 这个路径能不能长期使用。AppImage 运行时程序在临时挂载点里，链接过去没有意义
    stable: bool,
}

fn file_name() -> &'static str {
    if cfg!(windows) {
        "skill-studio-cli.exe"
    } else {
        "skill-studio-cli"
    }
}

/// 从程序坞打开时 PATH 很短，Homebrew 等常见位置也一并看
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    let home = skill_studio_core::fs::paths::home_dir();
    for dir in [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".local/bin"),
        home.join("bin"),
    ] {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

fn same(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// AppImage 把 App 挂在 /tmp/.mount_xxxx 里，重启后路径就没了
fn stable(path: &Path) -> bool {
    !path
        .to_string_lossy()
        .split(['/', '\\'])
        .any(|part| part.starts_with(".mount_"))
}

pub fn locate(bundled: Option<PathBuf>, dirs: &[PathBuf]) -> CliInfo {
    let Some(bundled) = bundled.filter(|p| p.is_file()) else {
        return CliInfo {
            bundled: None,
            command: None,
            location: None,
            stable: false,
        };
    };
    let stable = stable(&bundled);
    let linked = ["skill-studio", "skill-studio.exe"]
        .iter()
        .flat_map(|name| dirs.iter().map(move |dir| dir.join(name)))
        .find(|candidate| same(candidate, &bundled));
    if let Some(location) = linked {
        return CliInfo {
            bundled: Some(bundled),
            command: Some("skill-studio".into()),
            location: Some(location),
            stable,
        };
    }
    // Linux 的 deb 把它装在 /usr/bin，名字就是 skill-studio-cli
    let on_path = bundled
        .parent()
        .is_some_and(|dir| dirs.iter().any(|d| same(d, dir)));
    CliInfo {
        command: on_path.then(|| file_name().trim_end_matches(".exe").to_string()),
        location: on_path.then(|| bundled.clone()),
        bundled: Some(bundled),
        stable,
    }
}

#[tauri::command]
pub fn get_cli_info() -> CliInfo {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(file_name())));
    locate(bundled, &search_dirs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn a_homebrew_link_counts_as_installed_and_a_bare_bundle_does_not() {
        let temp = tempfile::tempdir().unwrap();
        let app = temp.path().join("Skill Studio.app/Contents/MacOS");
        std::fs::create_dir_all(&app).unwrap();
        let bundled = app.join("skill-studio-cli");
        std::fs::write(&bundled, "").unwrap();
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();

        let info = locate(Some(bundled.clone()), std::slice::from_ref(&bin));
        assert!(info.command.is_none() && info.location.is_none());
        assert_eq!(info.bundled.as_deref(), Some(bundled.as_path()));

        // 指向别的程序的 skill-studio 不算
        std::fs::write(bin.join("skill-studio"), "").unwrap();
        assert!(locate(Some(bundled.clone()), std::slice::from_ref(&bin))
            .command
            .is_none());

        std::fs::remove_file(bin.join("skill-studio")).unwrap();
        std::os::unix::fs::symlink(&bundled, bin.join("skill-studio")).unwrap();
        let info = locate(Some(bundled.clone()), std::slice::from_ref(&bin));
        assert_eq!(info.command.as_deref(), Some("skill-studio"));
        assert_eq!(info.location, Some(bin.join("skill-studio")));

        // 程序本身所在的目录就在 PATH 里（Linux 的 /usr/bin）
        let info = locate(Some(bundled.clone()), std::slice::from_ref(&app));
        assert_eq!(info.command.as_deref(), Some("skill-studio-cli"));

        assert!(locate(Some(temp.path().join("missing")), &[bin])
            .bundled
            .is_none());
    }

    #[test]
    #[cfg(unix)]
    fn an_appimage_mount_is_not_offered_as_a_permanent_path() {
        let temp = tempfile::tempdir().unwrap();
        let mount = temp.path().join(".mount_skill0/usr/bin");
        std::fs::create_dir_all(&mount).unwrap();
        let bundled = mount.join("skill-studio-cli");
        std::fs::write(&bundled, "").unwrap();
        let info = locate(Some(bundled), &[temp.path().join("bin")]);
        assert!(!info.stable);
    }
}
