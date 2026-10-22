//! 命令行入口，随桌面应用一起打包（macOS 在 Skill Studio.app/Contents/MacOS 里）。
//! 逻辑都在 skill-studio-cli：这里不引用 Tauri，编出来的程序不带界面。

fn main() {
    std::process::exit(skill_studio_cli::main());
}
