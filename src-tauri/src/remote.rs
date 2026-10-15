//! SSH transport. The webview receives structured results, never SSH credentials.
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    pub jump_host: Option<String>,
    #[serde(default)]
    pub password_auth: bool,
    pub helper_binary: Option<String>,
    /// Development/testing override only; never set by normal server creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_home: Option<String>,
}

impl ServerProfile {
    fn validate(&self) -> Result<(), String> {
        fn ssh_word(s: &str) -> bool {
            !s.is_empty()
                && !s.starts_with('-')
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-@[]:,".contains(c))
        }
        if self.id.is_empty()
            || self.id == "local"
            || self.name.trim().is_empty()
            || !ssh_word(&self.host)
            || self.user.as_deref().is_some_and(|v| !ssh_word(v))
            || self.jump_host.as_deref().is_some_and(|v| !ssh_word(v))
            || self.port == Some(0)
        {
            return Err("服务器名称、地址、用户名或端口无效".into());
        }
        Ok(())
    }
}

type SharedSession = Arc<Mutex<Session>>;
#[derive(Default)]
pub struct RemoteState {
    sessions: Mutex<HashMap<String, SharedSession>>,
    prompts: Mutex<HashMap<String, mpsc::Sender<Option<String>>>>,
}

fn profiles_path() -> PathBuf {
    skill_studio_core::fs::paths::config_dir().join("servers.json")
}
pub fn profiles() -> Result<Vec<ServerProfile>, String> {
    skill_studio_core::fs::atomic::read_json_file(&profiles_path())
        .map(|v| v.unwrap_or_default())
        .map_err(String::from)
}
pub fn save_profiles(servers: &[ServerProfile]) -> Result<(), String> {
    for s in servers {
        s.validate()?;
    }
    let mut ids = std::collections::HashSet::new();
    if servers.iter().any(|s| !ids.insert(&s.id)) {
        return Err("服务器 ID 重复".into());
    }
    let bytes = serde_json::to_vec_pretty(servers).map_err(|e| e.to_string())?;
    skill_studio_core::fs::atomic::atomic_write_private(&profiles_path(), &bytes)
        .map_err(String::from)
}

pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

struct AskPass {
    address: String,
    token: String,
    stop: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(unix)]
    control: tempfile::TempDir,
}
impl Drop for AskPass {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        #[cfg(unix)]
        {
            let _ = Command::new("ssh")
                .args(["-F", "/dev/null", "-S"])
                .arg(self.control.path().join("ssh"))
                .args(["-O", "exit", "localhost"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}
impl AskPass {
    fn new<R: tauri::Runtime>(app: tauri::AppHandle<R>, server_id: String) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let address = listener
            .local_addr()
            .map_err(|e| e.to_string())?
            .to_string();
        let token = uuid::Uuid::new_v4().to_string();
        let secret = token.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::spawn(move || {
            while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    }
                };
                if prepare_askpass_stream(&stream).is_err() {
                    continue;
                }
                let mut line = String::new();
                if BufReader::new(&mut stream)
                    .take(16384)
                    .read_line(&mut line)
                    .is_err()
                {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if v["token"] != secret {
                    continue;
                }
                let prompt = v["prompt"].as_str().unwrap_or("SSH 身份验证");
                let confirmation = v["confirm"] == true;
                let request_id = uuid::Uuid::new_v4().to_string();
                let (tx, rx) = mpsc::channel();
                let state = app.state::<RemoteState>();
                if let Ok(mut p) = state.prompts.lock() {
                    p.insert(request_id.clone(), tx);
                }
                let _=app.emit("ssh-prompt",json!({"requestId":request_id,"serverId":server_id,"prompt":prompt,"confirmation":confirmation}));
                let answer = rx.recv_timeout(Duration::from_secs(120)).ok().flatten();
                if let Ok(mut p) = state.prompts.lock() {
                    p.remove(&request_id);
                }
                let _ = writeln!(stream, "{}", json!({"answer":answer}));
            }
        });
        Ok(Self {
            address,
            token,
            stop,
            #[cfg(unix)]
            control: tempfile::Builder::new()
                .prefix("ss-")
                .tempdir_in("/tmp")
                .map_err(|e| e.to_string())?,
        })
    }
    fn configure(&self, cmd: &mut Command) -> Result<(), String> {
        #[cfg(unix)]
        cmd.args(["-o", "ControlMaster=auto", "-o", "ControlPersist=60", "-o"])
            .arg(format!(
                "ControlPath={}",
                self.control.path().join("ssh").display()
            ));
        cmd.env(
            "SSH_ASKPASS",
            std::env::current_exe().map_err(|e| e.to_string())?,
        )
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("DISPLAY", ":0")
        .env("SKILL_STUDIO_ASKPASS_ADDR", &self.address)
        .env("SKILL_STUDIO_ASKPASS_TOKEN", &self.token);
        Ok(())
    }
}

/// Runs before starting Tauri when OpenSSH invokes this executable as askpass.
pub fn run_askpass() -> bool {
    let Ok(address) = std::env::var("SKILL_STUDIO_ASKPASS_ADDR") else {
        return false;
    };
    let result = (|| -> Result<(), String> {
        let mut stream = TcpStream::connect(address).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(125)))
            .map_err(|e| e.to_string())?;
        let prompt = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
        let confirm = std::env::var("SSH_ASKPASS_PROMPT").as_deref() == Ok("confirm");
        writeln!(stream,"{}",json!({"token":std::env::var("SKILL_STUDIO_ASKPASS_TOKEN").unwrap_or_default(),"prompt":prompt,"confirm":confirm})).map_err(|e|e.to_string())?;
        let mut line = String::new();
        BufReader::new(stream)
            .take(65536)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let answer = value["answer"].as_str().ok_or("已取消")?;
        println!("{answer}");
        Ok(())
    })();
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}

fn ssh(profile: &ServerProfile, askpass: &AskPass) -> Result<Command, String> {
    profile.validate()?;
    let mut cmd = Command::new("ssh");
    cmd.args([
        "-T",
        "-o",
        "ClearAllForwardings=yes",
        "-o",
        "StrictHostKeyChecking=ask",
        "-o",
        "ConnectTimeout=12",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=2",
        "-o",
        "NumberOfPasswordPrompts=2",
    ]);
    if let Some(user) = &profile.user {
        cmd.args(["-l", user]);
    }
    if let Some(port) = profile.port {
        cmd.args(["-p", &port.to_string()]);
    }
    if let Some(key) = &profile.identity_file {
        cmd.arg("-i").arg(key);
    }
    if let Some(jump) = &profile.jump_host {
        cmd.args(["-J", jump]);
    }
    if profile.password_auth {
        cmd.args([
            "-o",
            "PreferredAuthentications=keyboard-interactive,password",
        ]);
    }
    askpass.configure(&mut cmd)?;
    cmd.arg(&profile.host);
    Ok(cmd)
}

fn captured(
    mut cmd: Command,
    payload: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let mut child = cmd
        .stdin(if payload.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法启动系统 SSH: {e}"))?;
    let out = child.stdout.take().ok_or("SSH stdout 不可用")?;
    let err = child.stderr.take().ok_or("SSH stderr 不可用")?;
    let stdout = std::thread::spawn(move || {
        let mut b = Vec::new();
        out.take(4 * 1024 * 1024).read_to_end(&mut b).map(|_| b)
    });
    let stderr = std::thread::spawn(move || {
        let mut b = Vec::new();
        err.take(65536).read_to_end(&mut b).map(|_| b)
    });
    let writer = payload.map(|bytes| {
        let input = child.stdin.take();
        std::thread::spawn(move || {
            input
                .ok_or_else(|| io_error("SSH stdin 不可用"))?
                .write_all(&bytes)
        })
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("SSH 操作超时，请检查连接后重试".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let output = stdout
        .join()
        .map_err(|_| "SSH 读取失败")?
        .map_err(|e| e.to_string())?;
    let error = stderr
        .join()
        .map_err(|_| "SSH 读取失败")?
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(String::from_utf8_lossy(&error).trim().to_string());
    }
    if let Some(writer) = writer {
        writer
            .join()
            .map_err(|_| "上传失败")?
            .map_err(|e| e.to_string())?;
    }
    Ok(output)
}
fn io_error(s: &str) -> std::io::Error {
    std::io::Error::other(s)
}

pub struct Session {
    child: Child,
    input: ChildStdin,
    responses: mpsc::Receiver<Result<Value, String>>,
    serial: u64,
    failed: bool,
    _askpass: AskPass,
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Session {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        if self.failed {
            return Err("REMOTE_DISCONNECTED:连接已断开，请重新连接".into());
        }
        self.serial += 1;
        let id = self.serial.to_string();
        let bytes =
            serde_json::to_vec(&json!({"version":1,"id":id,"method":method,"params":params}))
                .map_err(|e| e.to_string())?;
        if let Err(e) = self
            .input
            .write_all(&bytes)
            .and_then(|_| self.input.write_all(b"\n"))
            .and_then(|_| self.input.flush())
        {
            self.failed = true;
            return Err(format!("REMOTE_DISCONNECTED:连接已断开: {e}"));
        }
        let response = match self.responses.recv_timeout(Duration::from_secs(120)) {
            Ok(Ok(v)) => v,
            other => {
                self.failed = true;
                let _ = self.child.kill();
                return Err(format!(
                    "REMOTE_DISCONNECTED:服务器响应中断或超时；操作结果需重连后确认: {other:?}"
                ));
            }
        };
        if response["id"] != id {
            self.failed = true;
            return Err("REMOTE_DISCONNECTED:服务器响应序号不匹配，请重连".into());
        }
        if let Some(error) = response.get("error") {
            return Err(error["message"]
                .as_str()
                .unwrap_or("远程操作失败")
                .to_string());
        }
        Ok(response["result"].clone())
    }
}

pub fn connect<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    profile: ServerProfile,
) -> Result<Value, String> {
    profile.validate()?;
    disconnect(&app.state::<RemoteState>(), &profile.id)?;
    let askpass = AskPass::new(app.clone(), profile.id.clone())?;
    let progress = |stage: &str| {
        let _ = app.emit(
            "remote-progress",
            json!({"serverId":profile.id,"stage":stage}),
        );
    };
    progress("正在连接服务器");
    let mut probe = ssh(&profile, &askpass)?;
    probe.arg("uname -sm");
    let platform = String::from_utf8(captured(probe, None, Duration::from_secs(150))?)
        .map_err(|e| e.to_string())?;
    let arch = match platform.trim() {
        "Linux x86_64" => "x86_64",
        "Linux aarch64" => "aarch64",
        _ => return Err(format!("暂不支持该服务器系统: {}", platform.trim())),
    };
    let file_name = format!("skill-studio-remote-linux-{arch}");
    let binary = if let Some(path) = &profile.helper_binary {
        PathBuf::from(path)
    } else {
        let resource = app
            .path()
            .resource_dir()
            .map_err(|e| e.to_string())?
            .join("resources/remote")
            .join(&file_name);
        if resource.is_file() {
            resource
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources/remote")
                .join(&file_name)
        }
    };
    let data=std::fs::read(&binary).map_err(|_|format!("缺少 {arch} 远程组件，请在连接的高级选项中选择 Linux 辅助程序，或安装包含该组件的应用版本"))?;
    let machine = if arch == "x86_64" { 62u8 } else { 183u8 };
    if data.len() < 20
        || &data[..4] != b"\x7fELF"
        || data[5] != 1
        || data[18] != machine
        || data[19] != 0
    {
        return Err("辅助程序不是匹配服务器架构的 Linux 可执行文件".into());
    }
    let hash = format!("{:x}", Sha256::digest(&data));
    let filename = format!("{file_name}-{hash}");
    let remote_path = format!("\"$HOME/.skill-studio/bin/{filename}\"");
    progress("正在准备远程组件");
    let mut check = ssh(&profile, &askpass)?;
    check.arg(format!("test -f {remote_path} && sha256sum {remote_path}"));
    let installed = captured(check, None, Duration::from_secs(150))
        .ok()
        .is_some_and(|v| String::from_utf8_lossy(&v).starts_with(&hash));
    if !installed {
        let temp = format!(
            "\"$HOME/.skill-studio/bin/.upload-{}\"",
            uuid::Uuid::new_v4()
        );
        let script=format!("set -eu; umask 077; mkdir -p \"$HOME/.skill-studio/bin\"; trap 'rm -f {temp}' EXIT; cat > {temp}; test \"$(sha256sum {temp} | cut -d ' ' -f 1)\" = {hash}; chmod 700 {temp}; mv {temp} {remote_path}");
        let mut upload = ssh(&profile, &askpass)?;
        upload.arg(script);
        captured(upload, Some(data), Duration::from_secs(240))?;
    }
    progress("正在读取 Agent");
    let sandbox = profile
        .sandbox_home
        .as_ref()
        .map(|h| format!(" --sandbox-home {}", shell_quote(h)))
        .unwrap_or_default();
    let mut cmd = ssh(&profile, &askpass)?;
    cmd.arg(format!("exec {remote_path} --allow-writes{sandbox}"));
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let input = child.stdin.take().ok_or("SSH stdin 不可用")?;
    let output = child.stdout.take().ok_or("SSH stdout 不可用")?;
    let error = child.stderr.take().ok_or("SSH stderr 不可用")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let mut line = String::new();
            let result = reader.by_ref().take(32 * 1024 * 1024).read_line(&mut line);
            match result {
                Ok(0) => {
                    let _ = tx.send(Err("SSH 会话结束".into()));
                    break;
                }
                Ok(_) => {
                    if tx
                        .send(
                            serde_json::from_str(&line)
                                .map_err(|e| format!("辅助程序协议错误: {e}")),
                        )
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    break;
                }
            }
        }
    });
    // Drain stderr without forwarding raw remote configuration or credentials to logs.
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut BufReader::new(error), &mut std::io::sink());
    });
    let mut session = Session {
        child,
        input,
        responses: rx,
        serial: 0,
        failed: false,
        _askpass: askpass,
    };
    let hello = session.call("hello", Value::Null)?;
    if hello["protocolVersion"] != 1
        || hello["desktopServices"] != 1
        || hello["helperVersion"] != env!("CARGO_PKG_VERSION")
    {
        return Err("远程组件版本不兼容，请更新辅助程序".into());
    }
    session.call("get_settings", Value::Null)?;
    app.state::<RemoteState>()
        .sessions
        .lock()
        .map_err(|e| e.to_string())?
        .insert(profile.id, Arc::new(Mutex::new(session)));
    Ok(hello)
}

pub fn disconnect(state: &RemoteState, id: &str) -> Result<(), String> {
    state.sessions.lock().map_err(|e| e.to_string())?.remove(id);
    Ok(())
}
pub fn answer(state: &RemoteState, id: &str, value: Option<String>) -> Result<(), String> {
    let sender = state
        .prompts
        .lock()
        .map_err(|e| e.to_string())?
        .remove(id)
        .ok_or("身份验证请求已过期")?;
    sender.send(value).map_err(|_| "身份验证请求已过期".into())
}
pub fn request(
    state: &RemoteState,
    id: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let session = state
        .sessions
        .lock()
        .map_err(|e| e.to_string())?
        .get(id)
        .cloned()
        .ok_or("REMOTE_DISCONNECTED:服务器未连接")?;
    let mut session = session.lock().map_err(|e| e.to_string())?;
    match method {
        "install_catalog_skill" => {
            let source = params["source"].as_str().ok_or("缺少 source")?;
            let skill = params["skillId"].as_str().ok_or("缺少 skillId")?;
            use skill_studio_core::services::marketplace::{
                prepare_catalog_variants, CatalogPreparation,
            };
            let repository_path: Option<String> =
                serde_json::from_value(params["repositoryPath"].clone())
                    .map_err(|e| e.to_string())?;
            let repository_paths: Option<Vec<String>> =
                serde_json::from_value(params["repositoryPaths"].clone())
                    .map_err(|e| e.to_string())?;
            let selected =
                repository_paths.unwrap_or_else(|| repository_path.into_iter().collect());
            match prepare_catalog_variants(source, skill, &selected).map_err(String::from)? {
                CatalogPreparation::Ready(prepared) => {
                    let skill = upload_skill(&mut session, prepared)?;
                    Ok(serde_json::json!({ "status": "installed", "skill": skill }))
                }
                CatalogPreparation::SelectionRequired(candidates) => Ok(
                    serde_json::json!({ "status": "selectionRequired", "candidates": candidates }),
                ),
            }
        }
        "upload_local_skill" => {
            let path = params["path"].as_str().ok_or("缺少 path")?;
            let prepared = skill_studio_core::services::marketplace::prepare_local(Path::new(path))
                .map_err(String::from)?;
            upload_skill(&mut session, prepared)
        }
        _ => session.call(method, params),
    }
}
fn upload_skill(
    session: &mut Session,
    prepared: skill_studio_core::services::marketplace::PreparedSkill,
) -> Result<Value, String> {
    if !prepared.variants.is_empty()
        && session.call("hello", Value::Null)?["skillVariants"].as_u64() != Some(1)
    {
        return Err("远程 helper 不支持 Skill 变体，请重新连接以更新 helper".into());
    }
    session.call("upload_begin", Value::Null)?;
    fn walk(session: &mut Session, root: &Path, dir: &Path) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let metadata = entry.file_type().map_err(|e| e.to_string())?;
            if metadata.is_symlink() {
                return Err("上传不支持符号链接".into());
            }
            if metadata.is_dir() {
                walk(session, root, &path)?;
            } else if metadata.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
                #[cfg(unix)]
                let executable = {
                    use std::os::unix::fs::PermissionsExt;
                    file.metadata()
                        .map_err(|e| e.to_string())?
                        .permissions()
                        .mode()
                        & 0o111
                        != 0
                };
                #[cfg(not(unix))]
                let executable = false;
                upload_chunks(&mut file, |bytes| {
                    session.call(
                        "upload_file",
                        json!({"path":relative,"data":bytes,"executable":executable}),
                    )?;
                    Ok(())
                })?;
            }
        }
        Ok(())
    }
    walk(session, &prepared.directory, &prepared.directory)?;
    session.call("upload_finish",json!({"source":prepared.source,"skillId":prepared.skill_id,"repositoryPath":prepared.repository_path,"variants":prepared.variants}))
}

fn prepare_askpass_stream(stream: &TcpStream) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))
}

fn upload_chunks(
    reader: &mut impl Read,
    mut send: impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    loop {
        let mut bytes = vec![0; 64 * 1024];
        let len = match reader.read(&mut bytes) {
            Ok(len) => len,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.to_string()),
        };
        send(&bytes[..len])?;
        if len == 0 {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_arguments_are_quoted() {
        assert_eq!(shell_quote("a'b;$x"), "'a'\\''b;$x'");
    }
    #[test]
    fn rejects_ssh_option_injection() {
        let mut p = ServerProfile {
            id: "test".into(),
            name: "Test".into(),
            host: "-oProxyCommand=bad".into(),
            user: None,
            port: None,
            identity_file: None,
            jump_host: None,
            password_auth: false,
            helper_binary: None,
            sandbox_home: None,
        };
        assert!(p.validate().is_err());
        p.host = "192.168.1.2".into();
        assert!(p.validate().is_ok());
        p.jump_host = Some("x; touch file".into());
        assert!(p.validate().is_err());
    }

    /// Explicit opt-in live test. Only the generated /tmp fixture is managed.
    #[cfg(unix)]
    #[test]
    #[ignore = "requires SKILL_STUDIO_LIVE_HOST and SKILL_STUDIO_LIVE_BINARY"]
    fn live_desktop_ssh_mcp_inventory_is_read_only() {
        let app = tauri::test::mock_builder()
            .manage(RemoteState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let mut profile = ServerProfile {
            id: uuid::Uuid::new_v4().to_string(),
            name: "MCP inventory fixture".into(),
            host: std::env::var("SKILL_STUDIO_LIVE_HOST").expect("live host"),
            user: None,
            port: None,
            identity_file: None,
            jump_host: None,
            password_auth: false,
            helper_binary: Some(std::env::var("SKILL_STUDIO_LIVE_BINARY").expect("Linux helper")),
            sandbox_home: None,
        };
        let ask = AskPass::new(app.handle().clone(), profile.id.clone()).unwrap();
        let mut setup = ssh(&profile, &ask).unwrap();
        setup.arg("python3 -");
        let script = br#"import json, pathlib, tempfile
p=pathlib.Path(tempfile.mkdtemp(prefix='skill-studio-mcp-live-', dir='/tmp'))
(p/'.codex').mkdir()
(p/'.claude.json').write_text(json.dumps({'mcpServers':{'shared':{'url':'https://fixture.invalid/mcp','headers':{'Authorization':'fixture-private-token'}}}}))
(p/'.codex/config.toml').write_text("[mcp_servers.shared]\nurl='https://fixture.invalid/mcp'\nhttp_headers={Authorization='fixture-private-token'}\n")
print(p, end='')
"#;
        let fixture = String::from_utf8(
            captured(setup, Some(script.to_vec()), Duration::from_secs(60)).unwrap(),
        )
        .unwrap();
        assert!(fixture.starts_with("/tmp/skill-studio-mcp-live-"));
        profile.sandbox_home = Some(fixture.clone());
        let fingerprints = || {
            let mut command = ssh(&profile, &ask).unwrap();
            command.arg(format!(
                "sha256sum -- {} {}",
                shell_quote(&format!("{fixture}/.claude.json")),
                shell_quote(&format!("{fixture}/.codex/config.toml"))
            ));
            captured(command, None, Duration::from_secs(60)).unwrap()
        };
        let before = fingerprints();
        let hello = connect(app.handle(), profile.clone()).unwrap();
        assert!(hello["methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m == "scan_mcp"));
        let state = app.state::<RemoteState>();
        let inventory = request(&state, &profile.id, "scan_mcp", json!({})).unwrap();
        assert_eq!(inventory["servers"].as_array().unwrap().len(), 1);
        assert_eq!(inventory["servers"][0]["name"], "shared");
        assert_eq!(
            inventory["servers"][0]["agents"],
            json!(["claude", "codex"])
        );
        assert!(!inventory.to_string().contains("fixture-private-token"));
        assert!(request(
            &state,
            &profile.id,
            "mcp_request",
            json!({"method":"start"})
        )
        .unwrap_err()
        .contains("只读"));
        assert_eq!(
            request(&state, &profile.id, "scan_mcp", json!({})).unwrap(),
            inventory
        );
        disconnect(&state, &profile.id).unwrap();
        assert_eq!(fingerprints(), before);
        assert!(request(&state, &profile.id, "scan_mcp", json!({}))
            .unwrap_err()
            .starts_with("REMOTE_DISCONNECTED:"));
        println!("Read-only MCP desktop SSH integration passed. Isolated fixture: {fixture}");
    }

    /// Explicit opt-in live test. Only the generated /tmp fixture is managed.
    #[cfg(unix)]
    #[test]
    #[ignore = "requires SKILL_STUDIO_LIVE_HOST and SKILL_STUDIO_LIVE_BINARY"]
    fn live_desktop_ssh_deploy_and_project_roundtrip() {
        let host = std::env::var("SKILL_STUDIO_LIVE_HOST").expect("live host");
        let binary = std::env::var("SKILL_STUDIO_LIVE_BINARY").expect("Linux helper");
        let app = tauri::test::mock_builder()
            .manage(RemoteState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let mut profile = ServerProfile {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Integration fixture".into(),
            host,
            user: None,
            port: None,
            identity_file: None,
            jump_host: None,
            password_auth: false,
            helper_binary: Some(binary),
            sandbox_home: None,
        };
        let ask = AskPass::new(app.handle().clone(), profile.id.clone()).unwrap();
        let mut setup = ssh(&profile, &ask).unwrap();
        setup.arg("set -eu; d=$(mktemp -d /tmp/skill-studio-desktop-XXXXXXXX); mkdir -p \"$d/.claude/skills/probe\" \"$d/project\"; printf '%s\\n' '---' 'name: probe' 'description: fixture' '---' 'Test' > \"$d/.claude/skills/probe/SKILL.md\"; printf '%s' \"$d\"");
        let fixture =
            String::from_utf8(captured(setup, None, Duration::from_secs(60)).unwrap()).unwrap();
        assert!(fixture.starts_with("/tmp/skill-studio-desktop-"));
        profile.sandbox_home = Some(fixture.clone());
        let hello = connect(app.handle(), profile.clone()).unwrap();
        assert_eq!(hello["desktopServices"], 1);
        let state = app.state::<RemoteState>();
        let call =
            |method: &str, params: Value| request(&state, &profile.id, method, params).unwrap();
        let scan = call("scan_skills", json!({}));
        let skill_id = &scan[0]["id"];
        call(
            "set_skill_enabled",
            json!({"skillId":skill_id,"agentId":"claude-code","enabled":false}),
        );
        assert_eq!(
            call("scan_skills", json!({}))[0]["agents"]["claude-code"]["disabled"],
            true
        );
        call(
            "set_skill_enabled",
            json!({"skillId":skill_id,"agentId":"claude-code","enabled":true}),
        );
        let skill = call("adopt_to_hub", json!({"skillId":skill_id}));
        let group = call(
            "save_group",
            json!({"name":"Fixture group","skillIds":[skill["id"]]}),
        );
        let project = call(
            "create_project",
            json!({"name":"Fixture project","root":format!("{fixture}/project")}),
        );
        call(
            "apply_project",
            json!({"projectId":project["id"],"selection":{"agentIds":["claude-code"],"skillIds":[skill["id"]],"groupIds":[],"linkMode":"copy"}}),
        );
        assert_eq!(
            call("list_projects", json!({}))[0]["root"],
            format!("{fixture}/project")
        );
        assert_eq!(call("list_groups", json!({}))[0]["id"], group["id"]);
        assert!(!call("list_backups", json!({}))
            .as_array()
            .unwrap()
            .is_empty());
        let upload = tempfile::Builder::new()
            .prefix("skill-studio-upload-test-")
            .tempdir()
            .unwrap();
        std::fs::write(
            upload.path().join("SKILL.md"),
            "---\nname: uploaded\ndescription: fixture\n---\nUpload\n",
        )
        .unwrap();
        // Exercise multiple chunks and a non-aligned final chunk over real SSH.
        let payload: Vec<u8> = (0..150_123).map(|i| (i % 251) as u8).collect();
        std::fs::write(upload.path().join("payload.bin"), &payload).unwrap();
        let uploaded = call("upload_local_skill", json!({"path":upload.path()}));
        assert_eq!(uploaded["displayName"], "uploaded");
        let remote_payload = format!("{}/payload.bin", uploaded["sourcePath"].as_str().unwrap());
        assert!(remote_payload.starts_with(&format!("{fixture}/")));
        let mut verify = ssh(&profile, &ask).unwrap();
        verify.arg(format!("sha256sum -- {}", shell_quote(&remote_payload)));
        let digest =
            String::from_utf8(captured(verify, None, Duration::from_secs(60)).unwrap()).unwrap();
        assert_eq!(
            digest.split_whitespace().next().unwrap(),
            format!("{:x}", Sha256::digest(&payload))
        );

        disconnect(&state, &profile.id).unwrap();
        assert!(request(&state, &profile.id, "list_projects", json!({}))
            .unwrap_err()
            .starts_with("REMOTE_DISCONNECTED:"));
        connect(app.handle(), profile.clone()).unwrap();
        assert_eq!(call("list_projects", json!({}))[0]["id"], project["id"]);
        disconnect(&state, &profile.id).unwrap();
        println!("Desktop SSH integration passed. Isolated remote fixture: {fixture}");
    }
}

#[cfg(test)]
mod audit_regressions {
    use super::*;
    #[test]
    fn upload_short_reads_keep_all_bytes() {
        struct Short(std::io::Cursor<Vec<u8>>);
        impl Read for Short {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(&mut out[..7])
            }
        }
        let expected = vec![42; 150_000];
        let mut reader = Short(std::io::Cursor::new(expected.clone()));
        let mut actual = Vec::new();
        upload_chunks(&mut reader, |bytes| {
            actual.extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();
        assert_eq!(actual.len(), expected.len());
        assert_eq!(actual, expected);
    }
    #[test]
    fn accepted_askpass_socket_waits_for_client_message() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(value) => break value,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) => panic!("accept failed: {e}"),
            }
        };
        prepare_askpass_stream(&stream).unwrap();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            client.write_all(b"hello\n").unwrap();
        });
        let mut line = String::new();
        let result = BufReader::new(&mut stream).read_line(&mut line);
        writer.join().unwrap();
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(line, "hello\n");
    }
}
