//! 密钥隐藏：命令行输出和 `cli.log` 共用一套判断，免得一边隐藏了、另一边原样记下。
//!
//! 两种场景的写法不同（命令行自己的 `--env` / `--header`，与 MCP 启动命令里的参数），
//! 所以「像不像密钥的名字」只有一份，值怎么隐藏各留一个函数。

pub const HIDDEN: &str = "<已隐藏>";

/// 参数名或环境变量名里带这些词的，值都当作密钥
pub fn secret_name(name: &str) -> bool {
    let name = name.to_lowercase();
    [
        "key",
        "token",
        "secret",
        "password",
        "passwd",
        "auth",
        "credential",
    ]
    .iter()
    .any(|word| name.contains(word))
}

/// 命令行选项的值隐藏成什么样：环境变量和请求头留名字，配置整段隐藏，网址只隐藏参数值
pub fn hide(flag: &str, value: &str) -> String {
    match flag {
        "--env" => match value.split_once('=') {
            Some((name, _)) => format!("{name}={HIDDEN}"),
            None => HIDDEN.into(),
        },
        "--header" => match value.split_once(':').or_else(|| value.split_once('=')) {
            Some((name, _)) => format!("{}: {HIDDEN}", name.trim()),
            None => HIDDEN.into(),
        },
        "--url" => match value.split_once('?') {
            Some((base, _)) => format!("{base}?{HIDDEN}"),
            None => value.into(),
        },
        _ => HIDDEN.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_carry_credentials_are_recognised_in_both_spellings() {
        for name in [
            "--api-key",
            "--token",
            "--client-secret",
            "--password",
            "--passwd",
            "--authorization",
            "--credentials",
        ] {
            assert!(secret_name(name), "{name}");
        }
        for name in ["--port", "--ignore-robots-txt", "--config"] {
            assert!(!secret_name(name), "{name}");
        }
    }
}
