use clap::{Args, Subcommand, ValueEnum};
use skill_studio_core::models::skill::LinkMode;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum LinkChoice {
    Auto,
    Symlink,
    Copy,
}
impl From<LinkChoice> for LinkMode {
    fn from(value: LinkChoice) -> Self {
        match value {
            LinkChoice::Auto => Self::Auto,
            LinkChoice::Symlink => Self::Symlink,
            LinkChoice::Copy => Self::Copy,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum SettingsCommand {
    /// 查看完整设置（只读）
    Show,
    /// 修改设置；管理开关与手动 skill 策略必须单独保存
    Set(Box<SettingsSet>),
}
#[derive(Args, Debug, Default)]
pub struct SettingsSet {
    /// 开启这个 Agent 的管理；每次只切换一个，不自动启用分组
    #[arg(long, conflicts_with = "unmanage_agent")]
    pub manage_agent: Vec<String>,
    /// 退出这个 Agent 的管理；每次只切换一个（需要 --yes）
    #[arg(long, conflicts_with = "manage_agent")]
    pub unmanage_agent: Vec<String>,
    /// MCP 管理总开关 true/false（需要 --yes）
    #[arg(long)]
    pub manage_mcp: Option<bool>,
    /// 是否保留手动 skill true/false（需要 --yes）
    #[arg(long)]
    pub preserve_manual_skills: Option<bool>,
    /// CLI 与内置 skill 总开关（关闭需要 --yes；关闭后须在桌面重新开启）
    #[arg(long)]
    pub cli_enabled: Option<bool>,
    /// 默认安装链接方式
    #[arg(long)]
    pub default_link_mode: Option<LinkChoice>,
    /// Agent 配置目录，写成 Agent=目录，可重复
    #[arg(long)]
    pub agent_dir: Vec<String>,
    /// 取消这个 Agent 的目录覆盖，可重复
    #[arg(long)]
    pub clear_agent_dir: Vec<String>,
    /// Hub 目录；不会迁移文件，非空 Hub 不能切换
    #[arg(long, conflicts_with = "clear_hub_dir")]
    pub hub_dir: Option<PathBuf>,
    /// 恢复默认 Hub 目录
    #[arg(long, conflicts_with = "hub_dir")]
    pub clear_hub_dir: bool,
    /// 配置备份保留份数 0–100；调小会清理备份，需要 --yes
    #[arg(long, value_parser = clap::value_parser!(u64).range(0..=100))]
    pub backup_keep: Option<u64>,
    /// 显示 Codex App 内置 MCP true/false（仍只读）
    #[arg(long)]
    pub show_codex_builtin_mcp: Option<bool>,
    /// 桌面主题
    #[arg(long, value_parser = ["light", "dark", "system"])]
    pub theme: Option<String>,
    /// 界面语言
    #[arg(long)]
    pub language: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BackupKind {
    Config,
    Skill,
    Hub,
    Mcp,
}
impl BackupKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Skill => "skill",
            Self::Hub => "hub",
            Self::Mcp => "mcp",
        }
    }
}
#[derive(Subcommand, Debug)]
pub enum BackupsCommand {
    /// 列出备份；不指定种类时分别列出全部，MCP 管理关闭时跳过 MCP
    List {
        #[arg(value_name = "种类")]
        kind: Option<BackupKind>,
    },
    /// 从备份恢复（需要 --yes）；hub 完整备份由移出 Hub 流程还原
    Restore {
        #[arg(value_name = "种类")]
        kind: BackupKind,
        #[arg(value_name = "备份ID")]
        id: String,
    },
    /// 删除备份（需要 --yes）；仍在使用的 hub 完整备份不能删
    Delete {
        #[arg(value_name = "种类")]
        kind: BackupKind,
        #[arg(value_name = "备份ID", conflicts_with_all = ["all", "unused", "unrestorable", "legacy"])]
        ids: Vec<String>,
        /// 删除这个种类的全部备份
        #[arg(long, conflicts_with_all = ["ids", "unused", "unrestorable", "legacy"])]
        all: bool,
        /// 只删除未被引用的 hub 完整备份
        #[arg(long, conflicts_with_all = ["ids", "all", "unrestorable", "legacy"])]
        unused: bool,
        /// 只删除已无法恢复的 MCP 备份
        #[arg(long, conflicts_with_all = ["ids", "all", "unused", "legacy"])]
        unrestorable: bool,
        /// 清理旧版本写在 Agent 配置旁边的 MCP 备份
        #[arg(long, conflicts_with_all = ["ids", "all", "unused", "unrestorable"])]
        legacy: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProjectSkillsCommand {
    /// 列出项目里的原件、部署副本与停用记录
    List {
        #[arg(value_name = "项目")]
        project: String,
    },
    /// 收录一个项目原件到 Hub，保留项目文件（与全局 skills adopt 的搬移不同）
    Adopt {
        project: String,
        #[arg(value_name = "skill名称或路径")]
        skill: String,
    },
    /// 恢复项目里被 Studio 暂存停用的原件
    Enable {
        project: String,
        #[arg(value_name = "skill名称或路径")]
        skill: String,
    },
    /// 暂存停用一个项目原件，不影响其他成员或 Agent
    Disable {
        project: String,
        #[arg(value_name = "skill名称或路径")]
        skill: String,
    },
    /// 删除一个项目原件（需要 --yes）；非软链接保留可恢复备份
    Delete {
        project: String,
        #[arg(value_name = "skill名称或路径")]
        skill: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum McpMode {
    Direct,
    Gateway,
}
impl McpMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Gateway => "gateway",
        }
    }
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum McpTransport {
    Stdio,
    Http,
    Sse,
}
impl McpTransport {
    pub fn name(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
            Self::Sse => "sse",
        }
    }
}
#[derive(Args, Debug, Default)]
pub struct McpAdvanced {
    /// 替换完整连接定义（JSON、TOML 或安装命令）；密钥不写入输出/日志
    #[arg(long)]
    pub config: Option<String>,
    /// 服务类型；切换时清除原类型的专属连接字段
    #[arg(long)]
    pub transport: Option<McpTransport>,
    /// 修改直连/网关模式；网关需安装桌面应用，不自动启动或授权
    #[arg(long)]
    pub mode: Option<McpMode>,
    /// 本地 MCP 工作目录
    #[arg(long, conflicts_with = "clear_cwd")]
    pub cwd: Option<PathBuf>,
    /// 清除工作目录
    #[arg(long, conflicts_with = "cwd")]
    pub clear_cwd: bool,
    /// OAuth Client ID（网关 HTTP 服务）
    #[arg(long, conflicts_with = "clear_client_id")]
    pub client_id: Option<String>,
    /// 清除 OAuth Client ID
    #[arg(long, conflicts_with = "client_id")]
    pub clear_client_id: bool,
    /// OAuth scopes，逗号分隔
    #[arg(long, value_delimiter = ',', conflicts_with = "clear_scopes")]
    pub scopes: Option<Vec<String>>,
    /// 清除 OAuth scopes
    #[arg(long, conflicts_with = "scopes")]
    pub clear_scopes: bool,
    /// 明确要求 OAuth true/false；未设置时保留自动检查授权需求
    #[arg(long)]
    pub oauth: Option<bool>,
}
