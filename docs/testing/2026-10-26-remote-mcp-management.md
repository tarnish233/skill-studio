# 服务器上的 MCP 管理验证 · 2026-10-26

## 实现范围

- 界面 MCP 请求的分派从桌面应用移入 `skill_studio_service::mcp::requests`，桌面应用、服务器上的辅助程序（`serve --allow-writes`）共用同一份 Hub、托管与还原、分组、项目、备份与解析规则；网关启停、连接测试、官方目录搜索仍由桌面应用处理。
- 服务器的 `mcp_request` 不再拒绝：直连 MCP 的添加、编辑、托管、删除、分组与项目启停、备份恢复和解析都可用。`saveEntry` 保存网关模式、网关启停与授权一律拒绝，并说明「网关和浏览器授权只在本机运行」；`testDirect` 在服务器上启动服务完成握手，启动等待上限 75 秒，以免超过桌面等一次响应的时限（120 秒）。
- 前端去掉只读页面：服务器与使用同一个 MCP Hub、分组页和项目页；导航不再禁用分组与项目，启动预取改走同一查询键。「Studio 代理」保留在原位但不可选并写明原因，列表上的网关开关置灰，连接测试结果显示「服务器连接已验证」。
- 官方 MCP 目录由桌面搜索（服务器可能连不上外网），选中的配置写入服务器的 Hub。查看与编辑需要的完整配置经 SSH 返回桌面，只在内存中使用，不落盘、不写日志；卡片和标题沿用本机的隐藏规则。

## 已通过的检查

- Rust 工作区：510 项通过、0 失败、5 项显式忽略（实机网络/SSH 用例）。
- 辅助程序协议：11 项通过，其中新增 `remote_mcp_is_managed_like_the_desktop_for_direct_connections` 覆盖只读会话拒绝、直连 MCP 进 Hub、经分组写入并撤回、项目部署与撤回、备份、网关与授权拒绝、断开重连后的托管状态、移出 Hub 并还原、关闭 MCP 管理后的拒绝；`remote_connection_test_starts_the_stdio_service_on_the_server` 用服务器上的 python 服务验证 `testDirect` 真实启动并列出工具，且服务输出不污染协议。
- 前端：405 项通过；新增服务器 MCP 页面、托管、添加、目录搜索、连接测试、分组与项目导航、断线不回落，以及真实 `TargetProvider` 在本机与服务器之间切换 MCP Hub 的用例。
- Clippy（`-D warnings`）、Rust 格式、TypeScript、Prettier 与渲染器构建通过。

## 实机验证

- 用 zig 在本机交叉编译 `skill-studio-remote` 0.6.11（x86_64，最高需要 GLIBC 2.17），服务器为 Ubuntu / GLIBC 2.35。组件放入 `src-tauri/resources/remote/` 供 Debug 版本连接使用。
- 运行 `live_desktop_ssh_mcp_management_roundtrip`（`SKILL_STUDIO_LIVE_HOST`、`SKILL_STUDIO_LIVE_BINARY`）通过：经真实桌面 SSH 后端部署组件，在服务器 `/tmp` 的隔离 HOME 中验证名称清单不含凭据、管理一览带完整配置、直连 MCP 经分组写入与撤回、网关请求被拒绝、未涉及的配置文件内容不变、断线后拒绝请求。测试只使用生成的临时目录，未改动服务器上的真实配置；结束后已清理临时文件。

## 已知限制

- 服务器上没有 Studio 网关和浏览器授权，网关模式在服务器上不能使用，本机不受影响。
- 服务器上 Agent 自己的启动等待超过 75 秒时，连接测试可能先报超时；在 Agent 中连接仍按它自己的设置进行。
- 未在 ARM64 服务器上实机验证；发布流水线另行构建该架构。
