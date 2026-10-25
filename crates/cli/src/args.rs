//! 命令与参数。用词与界面一致：Skill Hub、分组、项目、启用、应用修改。

use std::ffi::OsString;
use std::path::PathBuf;

use clap::error::{ContextKind, ErrorKind};
use clap::{ArgAction, CommandFactory, Parser, Subcommand};

use crate::error::CliError;

const ABOUT: &str =
    "Skill Studio 命令行：查看和管理 Skill 与 MCP 的 Hub、分组与项目，按 Agent 启用。\n\
                     与桌面应用共用同一份配置，改动会实时出现在 Skill Studio 里。";

const AFTER: &str = "\
加 --json 输出 JSON。退出码：0 成功，1 操作失败，2 用法错误，3 找不到或名称有歧义，\
4 别的程序正在修改（重试即可），5 需要加 --yes 确认。
名称可以写 skill、MCP、分组或项目的名字；项目也可以写目录，「.」表示当前目录所在的项目。
Agent 写 claude-code（或 claude）、codex、opencode、pi、grok。";

#[derive(Parser, Debug)]
#[command(
    name = "skill-studio",
    version,
    about = ABOUT,
    after_help = AFTER,
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true,
    arg_required_else_help = true
)]
pub struct Cli {
    /// 输出 JSON，给脚本和 Agent 解析
    #[arg(long, global = true)]
    pub json: bool,

    /// 确认删除这类不可撤销的操作
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,

    /// 显示帮助
    #[arg(short = 'h', long, global = true, action = ArgAction::Help)]
    help: Option<bool>,

    /// 显示版本
    #[arg(short = 'V', long, action = ArgAction::Version)]
    version: Option<bool>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// 总览：Agent、分组与各 Agent 的启用、项目、需要检查和有待应用的修改
    Status,
    /// 列出 Agent：是否安装、是否在管理中、skill 目录
    Agents {
        /// 同时检查各 Agent 的命令行能否运行（较慢）
        #[arg(long)]
        probe: bool,
    },
    /// Skill Hub 里的 skill：列出、查看、搜索、安装、导入
    #[command(subcommand, arg_required_else_help = true)]
    Skills(SkillsCommand),
    /// 分组：一组 skill，可为每个 Agent 单独启用，多个分组可以叠加
    #[command(subcommand, arg_required_else_help = true)]
    Groups(GroupsCommand),
    /// 项目：把 skill 和分组写进项目目录，按 Agent 启用
    #[command(subcommand, arg_required_else_help = true)]
    Projects(ProjectsCommand),
    /// MCP：Hub 里的 MCP、MCP 分组与项目里的 MCP
    #[command(subcommand, arg_required_else_help = true)]
    Mcp(McpCommand),
    /// 自检：配置能否读取、有没有未完成的写入、Agent 目录、需要检查的问题
    Doctor,
    /// 生成命令行参考；内置 skill 的 references/commands.md 由它产出
    #[command(hide = true)]
    Docs,
}

#[derive(Subcommand, Debug)]
pub enum SkillsCommand {
    /// 列出 skill
    List {
        /// 只列这个 Agent 上已启用的
        #[arg(long, short = 'a', value_name = "Agent")]
        agent: Option<String>,
        /// 只列这个分组的成员
        #[arg(long, short = 'g', value_name = "分组")]
        group: Option<String>,
        /// 只列托管在 Skill Hub 里的
        #[arg(long)]
        hub: bool,
        /// 按名称或说明筛选
        #[arg(long, short = 'q', value_name = "词")]
        query: Option<String>,
    },
    /// 查看一个 skill：来源、所在分组与项目、在各 Agent 上的状态
    Show {
        /// skill 名称、ID 或目录
        #[arg(value_name = "skill")]
        skill: String,
        /// 同时输出 SKILL.md 全文
        #[arg(long)]
        content: bool,
    },
    /// 在 skills.sh 上搜索
    Search {
        /// 搜索词
        #[arg(value_name = "搜索词")]
        query: String,
    },
    /// 从 GitHub 安装到 Skill Hub；只放进 Hub，不会启用到任何 Agent
    Install {
        /// 仓库，如 owner/repo，或 owner/repo@skill
        #[arg(value_name = "仓库")]
        source: String,
        /// 仓库里的 skill 名称（仓库里没写 @skill 时）
        #[arg(long, value_name = "名称")]
        skill: Option<String>,
        /// 有多个版本时，选择仓库里的路径（可重复）
        #[arg(long = "path", value_name = "路径")]
        paths: Vec<String>,
    },
    /// 把本地的 skill 目录导入 Skill Hub
    Import {
        /// 含 SKILL.md 的目录
        #[arg(value_name = "目录")]
        path: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum GroupsCommand {
    /// 列出分组
    List,
    /// 查看一个分组：成员、在哪些 Agent 上启用、需要检查的问题
    Show {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
    },
    /// 新建分组
    Create {
        /// 分组名称
        #[arg(value_name = "名称")]
        name: String,
        /// 成员，逗号分隔
        #[arg(long, short = 's', value_delimiter = ',', value_name = "skill")]
        skills: Vec<String>,
    },
    /// 重命名分组
    Rename {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
        /// 新名称
        #[arg(value_name = "新名称")]
        name: String,
    },
    /// 加入成员，并写入正在使用这个分组的 Agent
    Add {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
        /// 要加入的 skill
        #[arg(required = true, value_delimiter = ',', value_name = "skill")]
        skills: Vec<String>,
    },
    /// 移出成员，并写入正在使用这个分组的 Agent
    Remove {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
        /// 要移出的 skill
        #[arg(required = true, value_delimiter = ',', value_name = "skill")]
        skills: Vec<String>,
    },
    /// 删除分组（需要 --yes）：只删分组记录，不删 Hub 里的 skill
    Delete {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
    },
    /// 为 Agent 启用分组，与它已启用的其他分组叠加
    Enable {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
        /// Agent，可重复或用逗号分隔
        #[arg(
            long = "agent",
            short = 'a',
            required = true,
            value_delimiter = ',',
            value_name = "Agent"
        )]
        agents: Vec<String>,
    },
    /// 为 Agent 停用分组，其他分组不受影响
    Disable {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
        /// Agent，可重复或用逗号分隔
        #[arg(
            long = "agent",
            short = 'a',
            required = true,
            value_delimiter = ',',
            value_name = "Agent"
        )]
        agents: Vec<String>,
    },
    /// 应用修改：按最新成员重新写入正在使用这个分组的 Agent
    Apply {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
        /// 只写入这些 Agent
        #[arg(
            long = "agent",
            short = 'a',
            value_delimiter = ',',
            value_name = "Agent"
        )]
        agents: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProjectsCommand {
    /// 列出项目
    List,
    /// 查看一个项目：成员、在哪些 Agent 上启用、需要检查的问题
    Show {
        /// 项目名称、ID 或目录（「.」表示当前目录所在的项目）
        #[arg(value_name = "项目")]
        project: String,
    },
    /// 添加项目
    Create {
        /// 项目目录
        #[arg(value_name = "目录")]
        dir: PathBuf,
        /// 项目名称，默认用目录名
        #[arg(long, value_name = "名称")]
        name: Option<String>,
    },
    /// 加入成员，并写入项目已启用的 Agent
    Add {
        /// 项目名称、ID 或目录（「.」表示当前目录所在的项目）
        #[arg(value_name = "项目")]
        project: String,
        /// 要加入的 skill，逗号分隔
        #[arg(long, short = 's', value_delimiter = ',', value_name = "skill")]
        skills: Vec<String>,
        /// 要绑定的分组，逗号分隔
        #[arg(long, short = 'g', value_delimiter = ',', value_name = "分组")]
        groups: Vec<String>,
    },
    /// 移出成员，并写入项目已启用的 Agent
    Remove {
        /// 项目名称、ID 或目录（「.」表示当前目录所在的项目）
        #[arg(value_name = "项目")]
        project: String,
        /// 要移出的 skill，逗号分隔
        #[arg(long, short = 's', value_delimiter = ',', value_name = "skill")]
        skills: Vec<String>,
        /// 要解绑的分组，逗号分隔
        #[arg(long, short = 'g', value_delimiter = ',', value_name = "分组")]
        groups: Vec<String>,
    },
    /// 为 Agent 启用项目：把成员写进项目里这个 Agent 的 skill 目录
    Enable {
        /// 项目名称、ID 或目录（「.」表示当前目录所在的项目）
        #[arg(value_name = "项目")]
        project: String,
        /// Agent，可重复或用逗号分隔
        #[arg(
            long = "agent",
            short = 'a',
            required = true,
            value_delimiter = ',',
            value_name = "Agent"
        )]
        agents: Vec<String>,
    },
    /// 为 Agent 停用项目：撤回写进项目目录的 skill
    Disable {
        /// 项目名称、ID 或目录（「.」表示当前目录所在的项目）
        #[arg(value_name = "项目")]
        project: String,
        /// Agent，可重复或用逗号分隔
        #[arg(
            long = "agent",
            short = 'a',
            required = true,
            value_delimiter = ',',
            value_name = "Agent"
        )]
        agents: Vec<String>,
    },
    /// 应用修改：按最新成员重新写入项目已启用的 Agent
    Apply {
        /// 项目名称、ID 或目录（「.」表示当前目录所在的项目）
        #[arg(value_name = "项目")]
        project: String,
    },
    /// 移除项目（需要 --yes）：只删记录，项目目录里的文件不动
    Delete {
        /// 项目名称、ID 或目录（「.」表示当前目录所在的项目）
        #[arg(value_name = "项目")]
        project: String,
    },
}

/// `--agent`：可重复或用逗号分隔
#[derive(clap::Args, Debug)]
pub struct Agents {
    /// Agent，可重复或用逗号分隔
    #[arg(
        long = "agent",
        short = 'a',
        required = true,
        value_delimiter = ',',
        value_name = "Agent"
    )]
    pub agents: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum McpCommand {
    /// 列出 MCP：Hub 里的，以及各 Agent 配置里原有的
    List {
        /// 只列这个 Agent 配置里有的
        #[arg(long, short = 'a', value_name = "Agent")]
        agent: Option<String>,
        /// 按名称、命令或网址筛选
        #[arg(long, short = 'q', value_name = "词")]
        query: Option<String>,
    },
    /// 查看一个 MCP：连接方式、所在分组与项目、配置在哪些地方（密钥一律隐藏）
    Show {
        /// MCP 名称或 ID
        #[arg(value_name = "MCP")]
        mcp: String,
    },
    /// 添加到 MCP Hub；只放进 Hub，不会写进任何 Agent，之后在分组或项目里选用
    #[command(after_help = "\
例子：
  skill-studio mcp add github --env GITHUB_TOKEN=xxx -- npx -y @modelcontextprotocol/server-github
  skill-studio mcp add context7 --url https://mcp.context7.com/mcp
  skill-studio mcp add --config '{\"mcpServers\":{\"fetch\":{\"command\":\"uvx\",\"args\":[\"mcp-server-fetch\"]}}}'")]
    Add {
        /// MCP 名称（用 --config 时可省略，取配置里的名称）
        #[arg(value_name = "名称")]
        name: Option<String>,
        /// 远程 MCP 的网址
        #[arg(long, value_name = "网址", conflicts_with_all = ["config", "command"])]
        url: Option<String>,
        /// 请求头，写成 "名称: 值" 或 名称=值，可重复
        #[arg(long = "header", short = 'H', value_name = "请求头", requires = "url")]
        headers: Vec<String>,
        /// 环境变量 KEY=值，可重复
        #[arg(long = "env", short = 'e', value_name = "KEY=值")]
        env: Vec<String>,
        /// 从说明里复制的配置：JSON、TOML、codex / claude mcp add 命令或网址
        #[arg(long, value_name = "配置", conflicts_with_all = ["url", "command"])]
        config: Option<String>,
        /// 由 Skill Studio 网关转发（需要桌面应用）；默认 Agent 直连
        #[arg(long)]
        gateway: bool,
        /// 启动命令和参数，写在 -- 之后
        #[arg(last = true, value_name = "命令")]
        command: Vec<String>,
    },
    /// 修改 Hub 里的 MCP：改环境变量、请求头、网址、启动命令或名称。用到它的分组和项目
    /// 会显示有待应用修改
    #[command(after_help = "\
例子：
  skill-studio mcp set github --env GITHUB_TOKEN=新的令牌
  skill-studio mcp set context7 --header \"Authorization: Bearer xxx\"
  skill-studio mcp set fetch -- uvx mcp-server-fetch --ignore-robots-txt")]
    Set {
        /// MCP 名称或 ID
        #[arg(value_name = "MCP")]
        mcp: String,
        /// 改名
        #[arg(long, value_name = "新名称")]
        name: Option<String>,
        /// 设置环境变量 KEY=值，可重复
        #[arg(long = "env", short = 'e', value_name = "KEY=值")]
        env: Vec<String>,
        /// 删掉环境变量，可重复
        #[arg(long = "unset-env", value_name = "KEY")]
        unset_env: Vec<String>,
        /// 设置请求头，写成 "名称: 值" 或 名称=值，可重复
        #[arg(long = "header", short = 'H', value_name = "请求头")]
        headers: Vec<String>,
        /// 删掉请求头，可重复
        #[arg(long = "unset-header", value_name = "名称")]
        unset_headers: Vec<String>,
        /// 换网址（远程 MCP）
        #[arg(long, value_name = "网址")]
        url: Option<String>,
        /// 换启动命令和参数，写在 -- 之后（本地启动的 MCP）
        #[arg(last = true, value_name = "命令")]
        command: Vec<String>,
    },
    /// 删除（需要 --yes）：Hub 里的 MCP 移出 Hub 并还原原来的配置；原有配置从 Agent 配置里删掉
    Remove {
        /// MCP 名称或 ID
        #[arg(value_name = "MCP")]
        mcp: String,
    },
    /// MCP 分组：一组 MCP，可为每个 Agent 单独启用，多个分组可以叠加
    #[command(subcommand, arg_required_else_help = true)]
    Groups(McpGroupsCommand),
    /// 项目里的 MCP：把 MCP 和 MCP 分组写进项目的配置，按 Agent 启用
    #[command(subcommand, arg_required_else_help = true)]
    Projects(McpProjectsCommand),
}

#[derive(Subcommand, Debug)]
pub enum McpGroupsCommand {
    /// 列出 MCP 分组
    List,
    /// 查看一个分组：成员、在哪些 Agent 上启用、需要检查的问题
    Show {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
    },
    /// 新建分组
    Create {
        /// 分组名称
        #[arg(value_name = "名称")]
        name: String,
        /// 成员，逗号分隔
        #[arg(long = "mcp", short = 'm', value_delimiter = ',', value_name = "MCP")]
        mcps: Vec<String>,
    },
    /// 重命名分组
    Rename {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
        /// 新名称
        #[arg(value_name = "新名称")]
        name: String,
    },
    /// 加入成员，并写入正在使用这个分组的 Agent
    Add {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
        /// 要加入的 MCP
        #[arg(required = true, value_delimiter = ',', value_name = "MCP")]
        mcps: Vec<String>,
    },
    /// 移出成员，并写入正在使用这个分组的 Agent
    Remove {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
        /// 要移出的 MCP
        #[arg(required = true, value_delimiter = ',', value_name = "MCP")]
        mcps: Vec<String>,
    },
    /// 删除分组（需要 --yes）：只删分组记录，不删 Hub 里的 MCP
    Delete {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
    },
    /// 为 Agent 启用分组，与它已启用的其他分组叠加
    Enable {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
        #[command(flatten)]
        agents: Agents,
    },
    /// 为 Agent 停用分组，其他分组不受影响
    Disable {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
        #[command(flatten)]
        agents: Agents,
    },
    /// 应用修改：按最新成员重新写入正在使用这个分组的 Agent
    Apply {
        /// 分组名称或 ID
        #[arg(value_name = "分组")]
        group: String,
        /// 只写入这些 Agent
        #[arg(
            long = "agent",
            short = 'a',
            value_delimiter = ',',
            value_name = "Agent"
        )]
        agents: Vec<String>,
    },
}

/// Claude Code 把项目的 MCP 写在哪里
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaudeScope {
    /// 项目里的 .mcp.json，可以随项目分享
    Project,
    /// ~/.claude.json 里这个项目的配置，只有自己用
    Local,
}

#[derive(Subcommand, Debug)]
pub enum McpProjectsCommand {
    /// 列出项目和它们的 MCP
    List,
    /// 查看一个项目的 MCP：成员、写到哪里、在哪些 Agent 上启用
    Show {
        /// 项目名称、ID 或目录（「.」表示当前目录所在的项目）
        #[arg(value_name = "项目")]
        project: String,
    },
    /// 加入成员（或改 Claude Code 的保存位置），并写入项目已启用的 Agent
    Add {
        /// 项目名称、ID 或目录（「.」表示当前目录所在的项目）
        #[arg(value_name = "项目")]
        project: String,
        /// 要加入的 MCP，逗号分隔
        #[arg(long = "mcp", short = 'm', value_delimiter = ',', value_name = "MCP")]
        mcps: Vec<String>,
        /// 要绑定的 MCP 分组，逗号分隔
        #[arg(long, short = 'g', value_delimiter = ',', value_name = "分组")]
        groups: Vec<String>,
        /// Claude Code 写到哪里：project（项目里的 .mcp.json）或 local（只给自己用）
        #[arg(long, value_enum, value_name = "位置")]
        claude: Option<ClaudeScope>,
    },
    /// 移出成员，并写入项目已启用的 Agent
    Remove {
        /// 项目名称、ID 或目录（「.」表示当前目录所在的项目）
        #[arg(value_name = "项目")]
        project: String,
        /// 要移出的 MCP，逗号分隔
        #[arg(long = "mcp", short = 'm', value_delimiter = ',', value_name = "MCP")]
        mcps: Vec<String>,
        /// 要解绑的 MCP 分组，逗号分隔
        #[arg(long, short = 'g', value_delimiter = ',', value_name = "分组")]
        groups: Vec<String>,
    },
    /// 为 Agent 启用：把成员写进项目里这个 Agent 的 MCP 配置
    Enable {
        /// 项目名称、ID 或目录（「.」表示当前目录所在的项目）
        #[arg(value_name = "项目")]
        project: String,
        #[command(flatten)]
        agents: Agents,
    },
    /// 为 Agent 停用：撤回写进项目的 MCP
    Disable {
        /// 项目名称、ID 或目录（「.」表示当前目录所在的项目）
        #[arg(value_name = "项目")]
        project: String,
        #[command(flatten)]
        agents: Agents,
    },
    /// 应用修改：按最新成员重新写入项目已启用的 Agent
    Apply {
        /// 项目名称、ID 或目录（「.」表示当前目录所在的项目）
        #[arg(value_name = "项目")]
        project: String,
    },
}

impl Command {
    /// 会改动配置或文件的命令：以可写方式打开，并记入命令行日志
    pub fn writes(&self) -> bool {
        match self {
            Command::Status | Command::Agents { .. } | Command::Doctor | Command::Docs => false,
            Command::Skills(c) => matches!(
                c,
                SkillsCommand::Install { .. } | SkillsCommand::Import { .. }
            ),
            Command::Groups(c) => !matches!(c, GroupsCommand::List | GroupsCommand::Show { .. }),
            Command::Projects(c) => {
                !matches!(c, ProjectsCommand::List | ProjectsCommand::Show { .. })
            }
            Command::Mcp(c) => match c {
                McpCommand::List { .. } | McpCommand::Show { .. } => false,
                McpCommand::Add { .. } | McpCommand::Set { .. } | McpCommand::Remove { .. } => true,
                McpCommand::Groups(c) => {
                    !matches!(c, McpGroupsCommand::List | McpGroupsCommand::Show { .. })
                }
                McpCommand::Projects(c) => !matches!(
                    c,
                    McpProjectsCommand::List | McpProjectsCommand::Show { .. }
                ),
            },
        }
    }
}

/// 帮助用中文：标题、占位符都换掉，子命令一并处理。用法行固定写 skill-studio
/// （Homebrew 装好后的命令名），不随可执行文件名变化
pub fn command() -> clap::Command {
    fn localize(command: clap::Command) -> clap::Command {
        let mut template = String::from("{about-with-newline}\n用法：{usage}\n");
        if command.has_subcommands() {
            template.push_str("\n命令：\n{subcommands}\n");
        }
        if command.get_positionals().next().is_some() {
            template.push_str("\n参数：\n{positionals}\n");
        }
        // -h/--help 是全局选项，每个命令都有
        template.push_str("\n选项：\n{options}\n{after-help}");
        command
            .help_template(template)
            .subcommand_value_name("命令")
            .mut_subcommands(localize)
    }
    fn usage(mut command: clap::Command) -> clap::Command {
        let rendered = command.render_usage().to_string();
        let line = rendered
            .trim_start_matches("Usage:")
            .trim()
            .replace("[OPTIONS]", "[选项]");
        command.override_usage(line).mut_subcommands(usage)
    }
    let mut command = localize(Cli::command().bin_name("skill-studio"));
    command.build();
    usage(command)
}

pub enum Exit {
    /// 帮助、版本，或缺少子命令时显示的帮助
    Print(String, i32),
    Usage(CliError),
}

/// `skill-studio docs`：根命令与每个子命令的帮助拼成一份参考。隐藏命令，不出现在帮助里；
/// 内置 skill 的 `references/commands.md` 与它逐字一致（测试会核对）
pub fn reference() -> String {
    fn walk(command: &clap::Command, path: &str, out: &mut String) {
        out.push_str(&format!("## {path}\n\n"));
        out.push_str(command.clone().render_help().to_string().trim_end());
        out.push_str("\n\n");
        for sub in command.get_subcommands() {
            if !sub.is_hide_set() {
                walk(sub, &format!("{path} {}", sub.get_name()), out);
            }
        }
    }
    let mut out =
        String::from("# 命令行参考\n\n由 `skill-studio docs` 生成，与当前版本一致；不要手改。\n\n");
    walk(&command(), "skill-studio", &mut out);
    out
}

pub fn parse(args: &[OsString]) -> Result<Cli, Exit> {
    use clap::FromArgMatches;
    let matches = command()
        .try_get_matches_from(args)
        .map_err(|error| match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                Exit::Print(error.render().to_string(), 0)
            }
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
                Exit::Print(error.render().to_string(), 2)
            }
            _ => Exit::Usage(usage_error(&error)),
        })?;
    Cli::from_arg_matches(&matches).map_err(|error| Exit::Usage(usage_error(&error)))
}

fn usage_error(error: &clap::Error) -> CliError {
    let context = |kind| {
        error
            .get(kind)
            .map(|value| value.to_string())
            .unwrap_or_default()
    };
    let message = match error.kind() {
        ErrorKind::InvalidSubcommand => {
            format!("没有这个命令：{}", context(ContextKind::InvalidSubcommand))
        }
        ErrorKind::UnknownArgument => {
            format!("不认识的参数：{}", context(ContextKind::InvalidArg))
        }
        ErrorKind::MissingRequiredArgument => {
            format!("缺少参数：{}", context(ContextKind::InvalidArg))
        }
        ErrorKind::InvalidValue | ErrorKind::ValueValidation => format!(
            "参数 {} 的值无效：{}",
            context(ContextKind::InvalidArg),
            context(ContextKind::InvalidValue)
        ),
        ErrorKind::ArgumentConflict => {
            format!("参数不能同时使用：{}", context(ContextKind::InvalidArg))
        }
        _ => error
            .to_string()
            .lines()
            .next()
            .unwrap_or_default()
            .trim_start_matches("error: ")
            .to_string(),
    };
    CliError::usage(message)
        .hint("运行 skill-studio --help 查看用法，子命令加 --help 查看各自的参数")
}
