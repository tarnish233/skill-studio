//! 失败的种类决定退出码，Agent 据此判断要不要重试、要不要先问用户。

use skill_studio_core::services::store::CONFIG_BUSY;
use skill_studio_service::state::CHANGED_ELSEWHERE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 操作本身失败（1）
    Failed,
    /// CLI 偏好关闭：退出码仍是 1，但不能记录审计日志
    CliDisabled,
    /// 命令或参数写错了（2）
    Usage,
    /// 找不到，或名称对应多个（3）
    NotFound,
    /// 别的程序正在改配置、或刚改过：稍后或直接重试即可（4）
    Busy,
    /// 不可撤销的操作需要加 --yes 确认（5）
    Confirm,
}

/// 成员已不在 Hub 里时的出路：先看缺了什么，再移出
const MISSING_HINT: &str = "用 skill-studio groups show / projects show 查看缺失的成员，\
     再用 groups remove 或 projects remove 移出，然后重新运行";

#[derive(Debug)]
pub struct CliError {
    pub kind: Kind,
    pub message: String,
    pub hint: Option<String>,
    pub candidates: Vec<String>,
}

impl CliError {
    fn new(kind: Kind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            hint: None,
            candidates: Vec::new(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(Kind::Failed, message)
    }
    pub(crate) fn disabled_cli(message: impl Into<String>) -> Self {
        Self::new(Kind::CliDisabled, message)
    }
    pub(crate) fn is_cli_disabled(&self) -> bool {
        self.kind == Kind::CliDisabled
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(Kind::Usage, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(Kind::NotFound, message)
    }

    pub fn confirm(message: impl Into<String>) -> Self {
        Self::new(Kind::Confirm, message)
    }

    /// 在已有错误前面补一句上下文（哪个 Agent、哪个分组），种类和提示都保留
    pub fn context(mut self, prefix: impl Into<String>) -> Self {
        self.message = format!("{}：{}", prefix.into(), self.message);
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn candidates(mut self, candidates: Vec<String>) -> Self {
        self.candidates = candidates;
        self
    }

    pub fn exit_code(&self) -> i32 {
        match self.kind {
            Kind::Failed | Kind::CliDisabled => 1,
            Kind::Usage => 2,
            Kind::NotFound => 3,
            Kind::Busy => 4,
            Kind::Confirm => 5,
        }
    }

    /// JSON 里的 `error.code`
    pub fn code(&self) -> &'static str {
        match self.kind {
            Kind::Failed | Kind::CliDisabled => "failed",
            Kind::Usage => "usage",
            Kind::NotFound => "not_found",
            Kind::Busy => "busy",
            Kind::Confirm => "needs_confirmation",
        }
    }

    /// 保存时发现别处刚改过：调用方可以按最新内容重做一次
    pub fn changed_elsewhere(&self) -> bool {
        self.kind == Kind::Busy && self.message.contains(CHANGED_ELSEWHERE)
    }
}

/// 服务层的错误是给界面看的一句话：等不到配置锁、或保存时发现别处改过，都可以重试
impl From<String> for CliError {
    fn from(message: String) -> Self {
        // 服务层的错误带「无效输入: 」「未找到: 」这类前缀，命令行里只是噪音
        if let Some(rest) = message.strip_prefix("未找到: ") {
            // 「分组成员已不存在：x」本身就是一句话，别再套一层「找不到」
            if rest.contains('：') {
                return Self::not_found(rest.to_string()).hint(MISSING_HINT);
            }
            let space = rest.starts_with(|c: char| c.is_ascii_alphanumeric());
            return Self::not_found(format!("找不到{}{rest}", if space { " " } else { "" }))
                .hint(MISSING_HINT);
        }
        let message = message
            .strip_prefix("无效输入: ")
            .map(str::to_string)
            .unwrap_or(message);
        if message.contains("命令行工具已在 Skill Studio 设置中关闭") {
            return crate::cli_disabled();
        }
        if message.contains(CONFIG_BUSY) {
            return Self::new(Kind::Busy, message).hint("稍后重新运行同一条命令");
        }
        if message.contains(CHANGED_ELSEWHERE) || message.contains("已被别处删除") {
            return Self::new(Kind::Busy, message).hint("重新运行同一条命令，会按最新内容处理");
        }
        if message.contains(skill_studio_service::mcp::DISABLED) {
            return Self::failed(message).hint("在 Skill Studio 设置里打开「启用 MCP 管理」");
        }
        if message.contains(skill_studio_mcp::management::GATEWAY_NEEDS_APP) {
            return Self::failed(message)
                .hint("在装有 Skill Studio 的电脑上运行命令行，或在 Skill Studio 里操作");
        }
        if message.contains(skill_studio_mcp::management::PENDING) {
            return Self::failed(message);
        }
        if message.contains("还没有成员") {
            return Self::failed(message)
                .hint("用 skill-studio groups add <分组> <skill> 加入成员，再为 Agent 启用");
        }
        if message.contains("Permission denied") || message.contains("Operation not permitted") {
            return Self::failed(message)
                .hint("没有写入权限。在 Agent 的沙箱里运行时，修改命令需要批准在沙箱外执行");
        }
        Self::failed(message)
    }
}

impl From<skill_studio_core::Error> for CliError {
    fn from(error: skill_studio_core::Error) -> Self {
        error.to_string().into()
    }
}
