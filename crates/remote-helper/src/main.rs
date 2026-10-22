//! 服务器上的 Skill Studio：与桌面应用捆绑的命令行是同一个程序，为 Linux 服务器单独构建。
//! 桌面应用经 SSH 运行 `skill-studio-remote serve` 管理服务器；用户在服务器上也能直接把它
//! 当命令行用（桌面应用连接时会链接成 `~/.skill-studio/bin/skill-studio`）。
fn main() {
    std::process::exit(skill_studio_cli::main());
}
