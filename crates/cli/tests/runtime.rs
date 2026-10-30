//! 通过本机 HTTP 假服务验证真实 MCP 握手/工具列表，不访问外网或用户配置。
use serde_json::{json, Value};
use serial_test::serial;
use skill_studio_core::fs::paths;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

struct Home(tempfile::TempDir);
impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(paths::TEST_HOME_ENV, dir.path());
        for name in [
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "XDG_CONFIG_HOME",
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_CONFIG",
            "PI_CODING_AGENT_DIR",
            "GROK_HOME",
            "SKILL_STUDIO_APP_EXECUTABLE",
        ] {
            std::env::remove_var(name);
        }
        Self(dir)
    }
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let mut out = vec![];
        let mut err = vec![];
        let code = skill_studio_cli::run(
            std::iter::once("skill-studio").chain(args.iter().copied()),
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }
    fn add(&self, url: &str) {
        let (code, _, err) = self.run(&[
            "mcp",
            "add",
            "test",
            "--url",
            url,
            "--header",
            "Authorization: Bearer sk-runtime-secret",
        ]);
        assert_eq!(code, 0, "{err}");
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        std::env::remove_var(paths::TEST_HOME_ENV);
    }
}
#[derive(Clone, Copy)]
enum Behavior {
    Ok,
    Auth,
    Hang,
}
struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new(behavior: Behavior) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !done.load(Ordering::Relaxed) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let request = read_request(&mut stream);
                        if matches!(behavior, Behavior::Hang) {
                            thread::sleep(Duration::from_secs(2));
                            continue;
                        }
                        if matches!(behavior, Behavior::Auth) {
                            respond(&mut stream, 401, json!({"error":"unauthorized"}));
                            continue;
                        }
                        let Some(request) = request else {
                            respond(&mut stream, 405, json!({}));
                            continue;
                        };
                        if request.get("id").is_none() {
                            respond(&mut stream, 202, Value::Null);
                            continue;
                        }
                        let result = match request["method"].as_str().unwrap_or("") {
                            "initialize" => {
                                json!({"protocolVersion":request["params"]["protocolVersion"],"capabilities":{"tools":{}},"serverInfo":{"name":"local-test","version":"1"}})
                            }
                            "tools/list" => {
                                json!({"tools":[{"name":"echo","description":"sk-runtime-secret","inputSchema":{"type":"object","properties":{}}}]})
                            }
                            _ => json!({}),
                        };
                        respond(
                            &mut stream,
                            200,
                            json!({"jsonrpc":"2.0","id":request["id"],"result":result}),
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url: format!("http://{address}/mcp?token=sk-query-only"),
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}
fn read_request(stream: &mut TcpStream) -> Option<Value> {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut data = vec![];
    let mut buffer = [0u8; 4096];
    loop {
        let n = stream.read(&mut buffer).ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&buffer[..n]);
        if data.len() > 1024 * 1024 {
            return None;
        }
        if let Some(end) = data.windows(4).position(|v| v == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&data[..end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if data.len() >= end + 4 + length {
                return serde_json::from_slice(&data[end + 4..end + 4 + length]).ok();
            }
        }
    }
}
fn respond(stream: &mut TcpStream, status: u16, value: Value) {
    let body = if value.is_null() {
        String::new()
    } else {
        value.to_string()
    };
    let extra = if status == 401 {
        "WWW-Authenticate: Bearer\r\n"
    } else {
        ""
    };
    let response=format!("HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
    let _ = stream.write_all(response.as_bytes());
}

#[test]
#[serial]
fn direct_test_really_initializes_and_lists_tools_without_writing_configuration() {
    let home = Home::new();
    let server = Server::new(Behavior::Ok);
    home.add(&server.url);
    let catalog = home.0.path().join(".skill-studio/mcp/catalog.json");
    let before = std::fs::read(&catalog).unwrap();
    let (code, out, err) = home.run(&["--json", "mcp", "test", "test", "--timeout", "3"]);
    assert_eq!(code, 0, "{err}");
    let result: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["tools"][0]["name"], "echo");
    assert!(!out.contains("sk-runtime-secret"), "{out}");
    assert_eq!(std::fs::read(catalog).unwrap(), before);
}

#[test]
#[serial]
fn unauthorized_and_hung_services_are_reported_with_nonzero_exit_codes() {
    for behavior in [Behavior::Auth, Behavior::Hang] {
        let home = Home::new();
        let server = Server::new(behavior);
        home.add(&server.url);
        let started = Instant::now();
        let (code, _, err) = home.run(&["--json", "mcp", "test", "test", "--timeout", "1"]);
        assert_eq!(code, 1, "{err}");
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(!err.contains("sk-runtime-secret"), "{err}");
        let error: Value = serde_json::from_str(&err).unwrap();
        assert_eq!(error["error"]["code"], "failed");
    }
}
