//! 远程模式：桌面应用经 SSH 启动 `skill-studio serve`，按行收发 JSON 请求与响应。
//! 默认只读；加 `--allow-writes` 才能修改（Skill 与 Agent 直连的 MCP）。服务器上装的就是
//! 这个命令行程序，所以同一个程序在服务器上也能直接当命令行用。
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use skill_studio_core::fs::{atomic, paths};
use skill_studio_core::models::agent::AGENTS;
use skill_studio_core::models::config::{AppConfig, CONFIG_VERSION};
use skill_studio_core::services::{store::Store, studio::Studio};
use skill_studio_mcp::{gateway, management::GATEWAY_NEEDS_APP};
use skill_studio_service::{mcp::requests, state::AppState};

const PROTOCOL_VERSION: u32 = 1;
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: String,
    version: u32,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Toggle {
    skill_id: String,
    agent_id: String,
    enabled: bool,
}

struct Helper {
    studio: Studio,
    writable: bool,
    // Kept for the entire stdio session, including recovery/config loading.
    _write_lock: Option<File>,
    upload: std::cell::RefCell<Option<tempfile::TempDir>>,
}

impl Helper {
    fn new(writable: bool) -> Result<Self, String> {
        let store = Store::default();
        let lock = if writable {
            fs::create_dir_all(store.dir()).map_err(|e| e.to_string())?;
            let f = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(store.dir().join("remote-helper.lock"))
                .map_err(|e| e.to_string())?;
            fs2::FileExt::try_lock_exclusive(&f)
                .map_err(|_| "此服务器已有可写管理会话，请先断开该会话".to_string())?;
            Some(f)
        } else {
            None
        };
        Ok(Self {
            studio: Studio::new(store),
            writable,
            _write_lock: lock,
            upload: std::cell::RefCell::new(None),
        })
    }

    fn config(&self) -> Result<AppConfig, String> {
        if self.writable {
            return self.studio.load_config().map_err(String::from);
        }
        // Store::load performs recovery/migration. Real read-only probes must
        // never trigger those writes, nor reconcile manual-skill policies.
        let config: AppConfig = atomic::read_json_file(&self.studio.store().config_path())
            .map_err(String::from)?
            .unwrap_or_default();
        if config.version != CONFIG_VERSION {
            return Err("配置版本不匹配，请使用兼容版本的辅助程序".into());
        }
        Ok(config)
    }

    fn execute(&self, request: &Request) -> Result<Value, String> {
        if request.version != PROTOCOL_VERSION {
            return Err("协议版本不兼容".into());
        }
        if request.method == "scan_mcp" {
            return serde_json::to_value(skill_studio_service::mcp::scan_inventory(
                self.studio.store(),
            )?)
            .map_err(|e| e.to_string());
        }
        if request.method == "list_directory" {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("");
            let path = if path.is_empty() || path == "~" {
                paths::home_dir()
            } else if let Some(tail) = path.strip_prefix("~/") {
                paths::home_dir().join(tail)
            } else {
                PathBuf::from(path)
            };
            if !path.is_absolute() {
                return Err("请输入服务器绝对路径".into());
            }
            let path = path.canonicalize().map_err(|e| e.to_string())?;
            let path = if path.is_file() {
                path.parent().ok_or("路径没有父目录")?.to_path_buf()
            } else {
                path
            };
            let mut directories = Vec::new();
            for entry in fs::read_dir(&path).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.path().is_dir() {
                    directories.push(
                        json!({"name":entry.file_name().to_string_lossy(),"path":entry.path()}),
                    );
                }
                if directories.len() >= 2000 {
                    break;
                }
            }
            directories.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
            return Ok(json!({"path":path,"parent":path.parent(),"directories":directories}));
        }
        if self.writable && request.method != "hello" {
            if request.method == "upload_begin" {
                *self.upload.borrow_mut() = Some(
                    tempfile::Builder::new()
                        .prefix("skill-studio-upload-")
                        .tempdir()
                        .map_err(|e| e.to_string())?,
                );
                return Ok(Value::Null);
            }
            if request.method == "upload_file" {
                let upload = self.upload.borrow();
                let root = upload.as_ref().ok_or("上传会话不存在")?.path();
                let relative = request.params["path"].as_str().ok_or("缺少文件路径")?;
                let relative = std::path::Path::new(relative);
                if relative.as_os_str().is_empty()
                    || relative
                        .components()
                        .any(|c| !matches!(c, std::path::Component::Normal(_)))
                {
                    return Err("上传路径无效".into());
                }
                let data: Vec<u8> = serde_json::from_value(request.params["data"].clone())
                    .map_err(|e| e.to_string())?;
                let path = root.join(relative);
                fs::create_dir_all(path.parent().ok_or("文件路径无效")?)
                    .map_err(|e| e.to_string())?;
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .map_err(|e| e.to_string())?;
                file.write_all(&data).map_err(|e| e.to_string())?;
                #[cfg(unix)]
                if request.params["executable"] == true {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                        .map_err(|e| e.to_string())?;
                }
                return Ok(Value::Null);
            }
            let state = AppState::bootstrap(self.studio.store().clone()).map_err(String::from)?;
            if request.method == "mcp_request" {
                return mcp_request(&state, &request.params);
            }
            if request.method == "upload_finish" {
                let upload = self.upload.borrow_mut().take().ok_or("上传会话不存在")?;
                let mut prepared =
                    skill_studio_core::services::marketplace::prepare_local(upload.path())
                        .map_err(String::from)?;
                prepared.source = request.params["source"]
                    .as_str()
                    .ok_or("缺少来源")?
                    .to_string();
                prepared.skill_id = request.params["skillId"]
                    .as_str()
                    .ok_or("缺少 skillId")?
                    .to_string();
                prepared.repository_path = request.params["repositoryPath"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                prepared.variants = serde_json::from_value(
                    request
                        .params
                        .get("variants")
                        .cloned()
                        .unwrap_or_else(|| json!([])),
                )
                .map_err(|e| e.to_string())?;
                return serde_json::to_value(
                    state
                        .install_catalog_skill(&prepared)
                        .map_err(String::from)?,
                )
                .map_err(|e| e.to_string());
            }
            return skill_studio_service::dispatch(&state, &request.method, request.params.clone());
        }
        match request.method.as_str() {
            "hello" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "helperVersion": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "home": paths::home_dir(),
                "writable": self.writable,
                "desktopServices": 1,
                "skillVariants": 1,
                "methods": ["hello", "list_agents", "scan_skills", "scan_mcp", "set_skill_enabled"]
            })),
            "list_agents" => {
                let config = self.config()?;
                // Do not run third-party executables for discovery.
                serde_json::to_value(self.studio.agents(&config, true)).map_err(|e| e.to_string())
            }
            "scan_skills" => {
                let config = self.config()?;
                let skills = self.studio.scan_skills(&config).map_err(String::from)?;
                serde_json::to_value(skills).map_err(|e| e.to_string())
            }
            "mcp_request" => Err("当前为只读连接，未允许修改 MCP".into()),
            "set_skill_enabled" => {
                if !self.writable {
                    return Err("当前为只读连接，未允许修改".into());
                }
                let toggle: Toggle = serde_json::from_value(request.params.clone())
                    .map_err(|e| format!("参数错误: {e}"))?;
                let config = self.config()?;
                self.studio
                    .set_skill_enabled(&config, &toggle.skill_id, &toggle.agent_id, toggle.enabled)
                    .map_err(String::from)?;
                Ok(Value::Null)
            }
            _ => Err("不支持的操作".into()),
        }
    }
}

/// 服务器上没有桌面应用：网关接入要写进 Agent 配置的命令不存在，浏览器授权也回不到本机
const NO_GATEWAY: &str =
    "服务器上暂不支持 Studio 网关：网关和浏览器授权只在本机运行，请改用 Agent 直连";

/// 连接测试必须在桌面应用等一次响应的时限（120 秒）内结束：启动最多等这么久，列出工具另有
/// 30 秒。Agent 自己的启动等待更长时，测试可能先超时，Agent 里仍能正常连接
const PROBE_STARTUP_LIMIT: Duration = Duration::from_secs(75);

/// 界面的 MCP 请求：与桌面应用共用服务层的分派。服务器上只管理 Agent 直连的 MCP；连接测试
/// 在服务器上进行，与服务器上的 Agent 连接时一样
fn mcp_request(state: &AppState, request: &Value) -> Result<Value, String> {
    let method = request["method"].as_str().ok_or("缺少 MCP 操作")?;
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    skill_studio_service::mcp::ensure_enabled(state)?;
    let result = match method {
        "testDirect" => probe(state, &params),
        "searchRegistry" => Err("官方 MCP 目录由桌面应用搜索".into()),
        // 不写接入时也能存进 Hub，但在服务器上永远用不上：保存前就拒绝
        "saveEntry" if params["entry"]["mode"] == "gateway" => Err(NO_GATEWAY.into()),
        _ if requests::handles(method) => requests::request(state, None, None, method, &params),
        _ => Err(NO_GATEWAY.into()),
    };
    result.map_err(|error| {
        if error.contains(GATEWAY_NEEDS_APP) {
            NO_GATEWAY.into()
        } else {
            error
        }
    })
}

fn probe(state: &AppState, params: &Value) -> Result<Value, String> {
    let id = params["id"].as_str().ok_or("缺少 MCP ID")?;
    let (server, startup) = requests::direct_probe(state, id)?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(gateway::probe_direct(
            server,
            startup.min(PROBE_STARTUP_LIMIT),
        ))
        .map_err(|e| e.to_string())
}

fn serve(helper: Helper, input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    // Split on newline with a bound before allocating an arbitrarily large line.
    let mut input = input;
    loop {
        let mut line = Vec::new();
        let count = input
            .by_ref()
            .take((MAX_REQUEST_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            return Ok(());
        }
        if count > MAX_REQUEST_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "请求过大"));
        }
        let response = match serde_json::from_slice::<Request>(&line) {
            Ok(request) => match helper.execute(&request) {
                Ok(value) => json!({"id": request.id, "result": value}),
                Err(message) => json!({"id": request.id, "error": {"message": message}}),
            },
            Err(_) => json!({"id": null, "error": {"message": "请求 JSON 格式无效"}}),
        };
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
}

/// `skill-studio serve [--allow-writes] [--sandbox-home 目录]`，返回退出码。
/// 不清除 Agent 目录的环境变量：服务器上的会话一直按环境变量解析（`--sandbox-home` 除外）
pub fn main(args: &[OsString]) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("skill-studio serve: {error}");
            1
        }
    }
}

/// Explicit test fixture root, not a security sandbox. Paths in fixtures must
/// also point inside it. Never inherit live Agent directory overrides.
fn isolate_sandbox_agent_env(path: &std::path::Path) {
    std::env::set_var(paths::TEST_HOME_ENV, path);
    // Descriptor-driven so a new Agent root cannot be forgotten here.
    for name in AGENTS.iter().flat_map(|agent| agent.env_var_names()) {
        std::env::remove_var(name);
    }
    for key in ["XDG_CONFIG_HOME", "OPENCODE_CONFIG"] {
        std::env::remove_var(key);
    }
}

fn run(args: &[OsString]) -> Result<(), String> {
    let mut writable = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.to_str().unwrap_or_default() {
            "--allow-writes" => writable = true,
            "--sandbox-home" => {
                let path = PathBuf::from(args.next().ok_or("缺少 sandbox-home 路径")?);
                if !path.is_absolute() || !path.is_dir() {
                    return Err("sandbox-home 必须是已存在的绝对目录".into());
                }
                isolate_sandbox_agent_env(&path);
            }
            _ => return Err(format!("未知参数: {}", arg.to_string_lossy())),
        }
    }
    let helper = Helper::new(writable)?;
    serve(helper, io::stdin().lock(), io::stdout().lock()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn sandbox_home_clears_every_agent_directory_override() {
        let home = tempfile::tempdir().unwrap();
        let names = [
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "OPENCODE_CONFIG_DIR",
            "PI_CODING_AGENT_DIR",
            "GROK_HOME",
            "DSH_HOME",
            "DSH_AGENTS_HOME",
        ];
        for name in names {
            std::env::set_var(name, "/tmp/live-agent-root");
        }
        isolate_sandbox_agent_env(home.path());
        for name in names {
            assert!(
                std::env::var_os(name).is_none(),
                "--sandbox-home must clear {name}"
            );
        }
        assert_eq!(
            std::env::var_os(paths::TEST_HOME_ENV),
            Some(home.path().as_os_str().to_os_string())
        );
        std::env::remove_var(paths::TEST_HOME_ENV);
    }
}
