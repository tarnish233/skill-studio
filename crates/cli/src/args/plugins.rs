use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum PluginsCommand {
    /// 本机插件清单：原生声明、可读文件、组件与配置启停；不运行插件
    List {
        #[arg(long)]
        agent: Option<String>,
        #[arg(long, short = 'q')]
        query: Option<String>,
    },
    /// 查看插件来源、作用域和只读组件；同名时用列表里的 ID
    Show { plugin: String },
    /// 只读检查标准或原生插件目录，不导入、安装或执行代码
    Inspect {
        path: PathBuf,
        #[arg(long)]
        agent: Option<String>,
    },
    /// 核对原生 CLI 的版本和命令能力；不代表已加载插件
    Agents,
    /// 核对待处理写入，只完成管理记录，不修改 Agent（需要 --yes）
    Recover,
    /// 调用原生 CLI 安装到个人配置（需要 --yes）；不转换或拆包
    Install {
        source: String,
        #[arg(long, required = true)]
        agent: String,
        /// 仅 Claude Code：先显式添加原生支持的市场来源
        #[arg(long)]
        marketplace: Option<String>,
    },
    /// 启用个人配置中的插件（需要 --yes；不授予额外信任）
    Enable { plugin: String },
    /// 停用个人配置，保留包和数据（需要 --yes）
    Disable { plugin: String },
    /// 使用原生命令更新一个插件；Git 有本地改动时拒绝（需要 --yes）
    Update { plugin: String },
    /// 从个人配置移除插件；缓存和数据由原生工具处理（需要 --yes）
    Uninstall { plugin: String },
    /// 仅清除 Studio 停用记录，不改原生配置或文件（需要 --yes）
    Forget { plugin: String },
}
