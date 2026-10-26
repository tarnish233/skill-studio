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

/// MCP 网址用于列表、详情与审计日志：隐藏用户名/密码、所有查询值（包括没有 `=`
/// 的令牌）和片段，不在输出或日志里保留可能是凭据的原始字节。
pub fn redact_url(url: &str) -> String {
    let (without_fragment, fragment) = match url.split_once('#') {
        Some((base, _)) => (base, true),
        None => (url, false),
    };
    let (base, query) = match without_fragment.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (without_fragment, None),
    };
    let base = if let Some((scheme, rest)) = base.split_once("://") {
        let boundary = rest.find('/').unwrap_or(rest.len());
        let (authority, suffix) = rest.split_at(boundary);
        match authority.rsplit_once('@') {
            Some((_, host)) => format!("{scheme}://{HIDDEN}@{host}{suffix}"),
            None => base.to_string(),
        }
    } else {
        base.to_string()
    };
    let mut safe = match query {
        None => base,
        Some(query) => format!(
            "{base}?{}",
            query
                .split('&')
                .map(|pair| match pair.split_once('=') {
                    Some((name, _)) if !name.is_empty() => format!("{name}={HIDDEN}"),
                    Some(_) => HIDDEN.to_string(),
                    None if !pair.is_empty() => HIDDEN.to_string(),
                    None => String::new(),
                })
                .collect::<Vec<_>>()
                .join("&")
        ),
    };
    if fragment {
        safe.push('#');
        safe.push_str(HIDDEN);
    }
    safe
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
        "--url" => {
            let redacted = redact_url(value);
            match redacted.split_once('?') {
                Some((base, _)) => format!("{base}?{HIDDEN}"),
                None => redacted,
            }
        }
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

    #[test]
    fn url_redaction_hides_userinfo_query_tokens_and_fragments() {
        assert_eq!(
            redact_url("https://user:password@example.com/mcp?api_key=token&bare-secret#fragment"),
            "https://<已隐藏>@example.com/mcp?api_key=<已隐藏>&<已隐藏>#<已隐藏>"
        );
        assert_eq!(
            hide(
                "--url",
                "https://user:password@example.com/mcp?api_key=token"
            ),
            "https://<已隐藏>@example.com/mcp?<已隐藏>"
        );
    }
}
