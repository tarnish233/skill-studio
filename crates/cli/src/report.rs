//! 输出：每条命令同时给出一段中文和一份 JSON，按 --json 选一种打印。

use std::path::Path;

use serde_json::{json, Value};
use skill_studio_core::models::agent::AGENTS;
use skill_studio_core::models::group::UntouchedCopy;

/// 与界面一致的提醒：改动要等 Agent 重新加载
pub const RELOAD: &str = "请在 Agent 中重新加载 skill 或新开会话";

pub struct Report {
    pub text: String,
    pub json: Value,
    /// 自检发现问题时非零
    pub exit: i32,
}

impl Report {
    pub fn new(text: impl Into<String>, json: Value) -> Self {
        Self {
            text: text.into(),
            json,
            exit: 0,
        }
    }

    /// 写操作的结果：做了什么、是否真有改动、原样留下了什么、要不要重新加载。
    /// `reload`：改动写进了 Agent 的目录，要等 Agent 重新加载才生效
    pub fn write(
        message: impl Into<String>,
        changed: bool,
        reload: bool,
        untouched: &[UntouchedCopy],
        extra: Value,
    ) -> Self {
        Self::written(
            message.into(),
            changed,
            reload.then_some(RELOAD),
            json!(untouched),
            (!untouched.is_empty()).then(|| untouched_text(untouched)),
            extra,
        )
    }

    /// 同 [`Self::write`]：`reload` 是改动生效前要做的提醒，`untouched` 是原样留下的内容
    /// （JSON 与给人看的说明各一份）
    pub fn written(
        message: String,
        changed: bool,
        reload: Option<&str>,
        untouched: Value,
        untouched_text: Option<String>,
        extra: Value,
    ) -> Self {
        let reload = reload.filter(|_| changed);
        let mut text = message.clone();
        if let Some(reload) = reload {
            text.push_str(&format!("。{reload}"));
        }
        if let Some(untouched) = untouched_text {
            text.push_str(&format!("\n\n{untouched}"));
        }
        let mut json = json!({
            "ok": true,
            "changed": changed,
            "message": message,
            "reload": reload,
            "untouched": untouched,
        });
        if let (Value::Object(map), Value::Object(more)) = (&mut json, extra) {
            map.extend(more);
        }
        Self {
            text,
            json,
            exit: 0,
        }
    }

    /// 接下来可以做什么：文字里另起一行，JSON 里放 `next`
    pub fn next(mut self, step: impl Into<String>) -> Self {
        let step = step.into();
        self.text.push_str(&format!("\n下一步：{step}"));
        if let Value::Object(map) = &mut self.json {
            map.insert("next".into(), Value::String(step));
        }
        self
    }
}

/// 名称里有空格时加引号，命令可以直接复制运行
pub fn arg(name: &str) -> String {
    if name.chars().any(char::is_whitespace) || name.is_empty() {
        format!("\"{}\"", name.replace('"', "\\\""))
    } else {
        name.to_string()
    }
}

/// Agent 的显示名；不认识的原样返回
pub fn agent_name(id: &str) -> String {
    AGENTS
        .iter()
        .find(|a| a.id == id)
        .map(|a| a.display_name.to_string())
        .unwrap_or_else(|| id.to_string())
}

pub fn agent_names(ids: &[String]) -> String {
    list(&ids.iter().map(|id| agent_name(id)).collect::<Vec<_>>())
}

/// 「甲、乙、丙」
pub fn list<S: AsRef<str>>(items: &[S]) -> String {
    items
        .iter()
        .map(|s| s.as_ref())
        .collect::<Vec<_>>()
        .join("、")
}

/// 主目录写成 ~，读起来短一些
pub fn home(path: &Path) -> String {
    let home = skill_studio_core::fs::paths::home_dir();
    // 主目录本身经过软链时（如 /tmp → /private/tmp），解析过的路径要对照解析过的主目录
    let canonical = home.canonicalize().unwrap_or_else(|_| home.clone());
    match path
        .strip_prefix(&home)
        .or_else(|_| path.strip_prefix(&canonical))
    {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        // Windows 上也写成 ~/.codex/skills，和提示里的写法一致，复制回来也能用
        Ok(rest) => format!("~/{}", rest.to_string_lossy().replace('\\', "/")),
        Err(_) => path.display().to_string(),
    }
}

pub fn tokens(total: u64) -> String {
    if total >= 1000 {
        format!("≈ {:.1}k tokens", total as f64 / 1000.0)
    } else {
        format!("≈ {total} tokens")
    }
}

/// 部署后被修改或替换的副本：Studio 不覆盖也不删除，原样告诉用户
pub fn untouched_text(untouched: &[UntouchedCopy]) -> String {
    let mut text = String::from("原样留下（部署后被修改或替换，Studio 不覆盖也不删除）：");
    for copy in untouched {
        let state = if copy.tracked {
            "仍由分组管理，标为需要检查"
        } else {
            "已不再管理，留给你处理"
        };
        text.push_str(&format!(
            "\n  {} · {} · {}（{}）",
            copy.name,
            home(&copy.path),
            copy.reason,
            state
        ));
    }
    text
}
