# Remote helper

通过 SSH 标准输入/输出执行远程 Agent / Skill 管理。桌面和辅助程序共用
`skill-studio-service`，后者复用 `skill-studio-core`；项目、分组、Hub、
原生启停、导入、预览、配置与备份使用同一套业务规则。

## 桌面使用

顶部「本机」菜单或「设置 → 服务器」添加 Linux 服务器：读取本机
`~/.ssh/config`（含 Include 文件）中的具体 Host，搜索选择后连接。
用户名、端口、私钥和跳板机沿用 SSH 配置；已保存的连接可编辑覆盖项。
首次主机确认、密码和密钥口令通过临时 SSH_ASKPASS 弹窗处理，不存储密码。
服务器记录保存在本机 `~/.skill-studio/servers.json`。

连接时检测 Linux 架构，分发应用内的对应辅助程序到远程
`~/.skill-studio/bin/`。文件名包含 SHA-256，上传后验证哈希和程序/协议版本。
远程无需 Rust 或外网。SSH 用户需有配置目录的读写权限。

选择服务器后，所有管理数据及项目路径属于服务器；主题、语言、版本和
连接记录仍属于桌面。本机与每台服务器使用独立查询缓存。远程目录由专用
目录选择器浏览，Skill 也可从本机上传。市场下载由桌面完成后上传，适用于
服务器不能访问外网的场景。连接失败不会回退到本机操作。

MCP 管理走 `mcp_request`（`{"method": …, "params": …}`），与桌面应用的同名命令共用
`skill_studio_service::mcp::requests` 的分派：Hub、托管与还原、分组、项目、备份和解析
与本机一致。服务器上只管理 Agent 直连的 MCP：网关的启停、授权和网关模式的保存都会被
拒绝并说明原因；`testDirect` 在服务器上启动服务完成握手，启动最多等待 75 秒，以免超过
桌面等一次响应的时限；官方目录由桌面搜索。查看和编辑需要完整配置，连接定义经 SSH
返回桌面，与本机一样只在内存中使用。

`scan_mcp` 仍是只读会话可用的名称清单：响应仅含 `servers: [{id, name, agents}]` 与
`warnings`，不返回原始连接配置、凭据和启动参数，也不进行配置恢复或启动第三方进程。

## 构建

在相应 Linux 环境执行：

```sh
cargo build --locked --release -p skill-studio-remote
```

本地开发时将二进制放入桌面资源目录，命名为
`src-tauri/resources/remote/skill-studio-remote-linux-x86_64` 或
`skill-studio-remote-linux-aarch64`。二进制不提交 Git，打包前需准备。
也可在连接的高级选项中选择匹配的本地 Linux 二进制。
正式发布流水线会在 Ubuntu 22.04 上构建并注入两个架构的辅助程序。

## 协议 v1

每行一个 JSON，请求包含字符串 `id`、整数 `version`、`method` 和可选 `params`。
单个请求最多 1 MiB；stdout 仅输出协议，启动错误写入 stderr。

```json
{"id":"1","version":1,"method":"hello"}
{"id":"2","version":1,"method":"list_agents"}
{"id":"3","version":1,"method":"scan_skills"}
{"id":"4","version":1,"method":"scan_mcp"}
```

响应为 `{"id":"...","result":...}` 或 `{"id":"...","error":{"message":"..."}}`。
`hello` 返回平台、HOME、可写状态和 `desktopServices: 1` 能力标识。
可写管理命令及 camelCase 参数见 `crates/service/src/lib.rs`。
另有 `list_directory`、`upload_begin`、`upload_file`、`upload_finish`。
上传分块最多 64 KiB，拒绝绝对路径和父目录穿越，安装复用核心校验。

手动启动默认只读，仅允许握手、扫描、Agent 探测和目录浏览，不执行配置迁移、
事务恢复或第三方 Agent CLI。`--allow-writes` 开启 Skill 与 Agent 直连 MCP 的管理。
写入会话持有进程锁，EOF 后退出。超时不会自动重试写入，需重连后确认结果。

`--sandbox-home /absolute/fixture/home` 为测试指定 HOME，并清除继承的
各 Agent 的配置目录环境变量。它不是安全沙箱：测试配置中的绝对路径
和链接也必须自行指向测试目录。

## 验证

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm test:unit
SKILL_STUDIO_LIVE_HOST=my-server \
SKILL_STUDIO_LIVE_BINARY=/absolute/path/skill-studio-remote-linux-x86_64 \
cargo test -p skill-studio --lib live_desktop_ssh_deploy_and_project_roundtrip -- --ignored --nocapture
```

实机测试使用实际桌面 SSH 后端部署组件，并在远程 `/tmp` 独立配置中验证
启停、Hub、分组、项目、备份、上传和重连。

只验证远程 MCP 时，将上述测试名替换为 `live_desktop_ssh_mcp_management_roundtrip`。
此测试仅在生成的临时 HOME 中创建 MCP 样例，验证名称清单不含凭据、直连 MCP 经分组
写入并撤回、网关请求被拒绝，以及未涉及的配置文件内容不变。
