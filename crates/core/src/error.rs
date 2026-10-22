use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

/// 核心错误类型。到 `#[tauri::command]` 边界统一转成 `String`（与 cc-switch 一致），
/// 前端只需处理可读消息，不必解析结构。
#[derive(Debug, Error)]
pub enum Error {
    #[error("配置错误: {0}")]
    Config(String),

    #[error("无效输入: {0}")]
    InvalidInput(String),

    #[error("IO 错误: {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{context}: {source}")]
    IoContext {
        context: String,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON 解析错误: {path}: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("JSON 序列化错误: {source}")]
    JsonSerialize {
        #[source]
        source: serde_json::Error,
    },

    #[error("TOML 解析错误: {path}: {source}")]
    Toml {
        path: String,
        #[source]
        source: toml_edit::TomlError,
    },

    /// 目标位置已存在非本工具管理的内容，拒绝覆盖
    #[error("目标已被占用且非本工具管理: {0}")]
    Foreign(String),

    #[error("未找到: {0}")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn io(path: impl AsRef<std::path::Path>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.as_ref().display().to_string(),
            source,
        }
    }

    pub fn io_context(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::IoContext {
            context: context.into(),
            source,
        }
    }

    pub fn json(path: impl AsRef<std::path::Path>, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.as_ref().display().to_string(),
            source,
        }
    }

    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::InvalidInput(msg.into())
    }
}

impl Error {
    /// 等不到跨进程的配置锁：稍后重试即可，不是这次操作本身的问题
    pub fn is_config_busy(&self) -> bool {
        matches!(self, Error::Other(message) if message.contains(crate::services::store::CONFIG_BUSY))
    }
}

impl From<Error> for String {
    fn from(err: Error) -> Self {
        err.to_string()
    }
}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        Self::Other(format!("{err:#}"))
    }
}

/// 序列化成扁平字符串，前端 `invoke` 的 reject 值就是可读消息。
impl serde::Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
