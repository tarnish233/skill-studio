# MCP Hub

MCP Hub 自动发现并管理 Claude Code、Codex、OpenCode v2、Pi（0.99.0+）和 Grok Build 的 MCP 配置。配置管理与后台网关独立：网关关闭时仍可扫描、编辑、保存和同步直连配置。

## 添加与接入

点击右上角 **添加 MCP**，打开统一安装页：

1. 粘贴 `codex mcp add …`、`claude mcp add …`、HTTP 网址、JSON／JSONC 或 TOML，识别名称、地址或命令；也可直接填写连接字段。OpenCode 配置只接受 v2 的 `mcp.servers` 格式。
2. 保存到 Hub，选择 **Agent 直连** 或 **Studio 代理**。服务需要登录时，在连接详情中进行授权。
3. 在对应 Agent 的 MCP 页面添加全局接入、启用 MCP 分组，或在项目页选择 Agent 添加项目接入。
4. 在 Agent 中重新加载 MCP。直连的登录由 Agent 引导；代理需要启动 Studio 网关，并由 Studio 管理授权。保存配置不等于已完成连接或登录。

例如可以直接粘贴：

```sh
codex mcp add hf-mcp-server --url "https://huggingface.co/mcp?login"
```

这会自动填好名称与 HTTP 地址。安装命令只作为配置数据解析，不交给 Shell 执行；推断出的 Agent／作用域不会自动创建接入。多个服务可选择其中一项。完整 JSON 与工作目录收在折叠区，并保留客户端扩展字段。

### 配置位置

| Agent               | 全局配置                              | 项目配置                                         |
| ------------------- | ------------------------------------- | ------------------------------------------------ |
| Claude Code         | `~/.claude.json`                      | `.mcp.json`，或全局文件中的项目本地条目          |
| Codex               | `~/.codex/config.toml`                | `.codex/config.toml`                             |
| OpenCode v2         | `~/.config/opencode/opencode.json(c)` | `opencode.json(c)`、`.opencode/opencode.json(c)` |
| Pi 0.99.0+ | `~/.pi/agent/mcp.json`                | `.pi/mcp.json`                                   |
| Grok Build          | `~/.grok/config.toml`                 | `.grok/config.toml`                              |

沿用设置中的 Agent 目录覆盖和 Agent 环境变量。OpenCode 同时扫描 `OPENCODE_CONFIG_DIR`、`OPENCODE_CONFIG`；写入优先选择已有配置，避免新建同级竞争文件。仅支持 OpenCode v2，v1 配置需先迁移。修改 JSONC 时保留其他属性与注释。

Pi 0.99.0 起原生支持 MCP，无需安装扩展。Studio 管理上述原生配置文件，使用 `enabled` 表示启停状态，并保留 `oauth`、`exposure`、`toolExposure` 等配置字段。保存后在 Pi 中运行 `/reload`，通过 `/mcp` 查看连接或登录状态；项目配置需先在 Pi 中信任该项目。旧 pi-mcp-adapter 的 `disabled` 不再作为原生启停依据；升级后请检查原先停用的服务并使用 `enabled: false`。适配器专属设置不会自动转换。

Pi 项目配置里没有 `command`、`url`、`type` 的条目不是独立服务。它只覆盖同名用户级服务在该项目中的 `enabled`、`exposure` 和 `toolExposure`，名称里的 `-` 与 `_` 视为相同。Hub 把这种覆盖记在用户级服务的来源上，不另显示一张 MCP。没有对应的用户级服务时，扫描给出提示，仍然不把它当成新服务。带有连接信息的项目条目会替换同名用户级服务，并按自己的连接单独显示。

| 连接方式    | 运行与登录                                          | 网关关闭时           |
| ----------- | --------------------------------------------------- | -------------------- |
| Agent 直连  | Agent 直接启动或连接 MCP，各自管理登录              | 可继续使用           |
| Studio 网关 | Studio 连接上游并管理共享 OAuth，Agent 连接本机网关 | 暂时无法使用这些工具 |

列表右侧按钮启停网关，详情中可测试连接或登录授权。普通保存和移出管理不会自动打开浏览器。

## Agent MCP 分组

在 Claude Code、Codex、OpenCode v2、Pi（0.99.0+）和 Grok Build 页面切换到 **MCP 分组**，操作与 Skill 分组一致：新建组合、从 MCP Hub 勾选服务、保存后点击 **启用**。每个 Agent 同时启用一个 MCP 分组，Skill 分组独立保留。支持搜索、编辑和拖动排序；使用中的组需要先停用才能删除。

同一个 MCP 可被多个组引用。保存组不会立即写入 Agent 配置；使用中的成员配置或成员列表改变后显示 **有待应用修改**，点击 **应用修改** 更新。缺失成员会保留提示，需要移除或补齐后才能启用。

分组应用到该 Agent 的用户全局配置，不改动项目作用域。自动发现的 MCP 可直接选入组，保存时保留其来源与原配置。启用会复用已有的同名匹配条目；同名但配置不同则停止操作。组外已有 MCP 保留，切换／停用只撤销组部署的条目并还原原配置。分组不会启动或关闭网关，网关 MCP 使用前仍需开启网关。

新部署的直连成员使用目标 Agent 的启用状态，不修改 Hub 模板中保存的停用标记；目标配置中已被手动停用的同名服务仍会阻止启用分组，需先在原 Agent 中恢复。

配置写入与分组状态在同一个事务中保存。检测到外部修改时显示 **需要检查** 并拒绝覆盖；使用中的 MCP 不能直接移出管理，需先停用引用它的组。

## 已有 MCP 与迁移

打开页面读取上述全局文件和 Studio 已登记项目的原生配置，Claude 还读取项目本地作用域。页面每 30 秒刷新，切换 Agent 复用缓存。Grok 的全局停用列表会反映在来源状态中，分组不会擅自解除原生停用设置。

相同连接合并显示来源，同名但配置不同的保留独立记录。Pi 的项目级覆盖沿用上一节的规则，不因同名而多出一张卡片。来源图标表示配置存在，不表示连接测试成功。扫描不启动程序、不修改原文件，也不导入 Agent 的登录缓存。

点击已有服务的 **管理配置与接入**：

- 默认保留 **Agent 直连**。客户端的超时、工具过滤、环境变量引用等字段可保留在原生配置中；跨 Agent 使用时由目标 Agent 决定是否支持。
- 选择 **Studio 网关**时先检查兼容性。当前不能由网关解释的字段会明确提示，但不会阻止继续使用直连。
- 默认选中扫描到的原接入位置，切换网关会替换这些位置的原条目，沿用其名称和作用域，不额外新增一条代理配置。可取消不希望管理的位置。
- 编辑已管理服务时，可同时修改配置、切换直连／网关、增加多个 Agent 的接入。取消现有接入会从对应 Agent 移除该条目。
- **移出 Studio**默认还原管理前的配置；原本不存在、由 Studio 新建的条目会移除。取消“还原管理前的原始配置”则删除受管理的接入条目。

每次写入前在目标配置旁创建 `.studio-backup-*` 私有备份。只修改对应 MCP 条目，保留 Provider、其他 MCP 和未修改 TOML 条目的注释。同名冲突、来源变化、编辑器打开后管理记录被修改，都会停止保存，不静默覆盖。

多文件操作先完成校验，再写入事务记录；写入失败时回滚。应用下次读取管理记录时也会恢复未完成的事务。若恢复时发现外部修改，会保留事务文件并报错，避免覆盖外部编辑。备份是完整文件；自动还原仅针对受管理条目，以保留其余后续修改。

## 网关与凭据

网关只监听 `127.0.0.1`，关闭窗口后继续运行；电脑重启后需手动开启，本版不注册系统开机服务。应用升级后若旧后台进程仍在运行，界面会提示关闭并重新开启网关。

网关支持 stdio 和 Streamable HTTP、静态请求头、公共客户端 OAuth、工具／资源／提示词请求。OAuth 在 Studio 中完成并协调刷新，不复制 Claude／Codex 的内部登录缓存。修改上游连接或认证参数会使旧授权失效；重命名不会清除授权。

Gateway 模式的 Agent 配置仅包含 Studio 启动命令和服务标识，不包含上游 API Key 或 OAuth token。直连模式按目标 Agent 的原生格式写入配置，包括用户配置的静态请求头或环境变量。网关入口包含本机应用和数据目录路径，不适合直接跨电脑共享。

管理数据位于 `~/.skill-studio/mcp/catalog.json`；`config.json` 保存网关运行配置；OAuth 凭据存于独立的 `oauth-<id>.json` 文件。Unix 上管理目录为 0700，管理文件、事务文件与备份使用 0600。文件未加密，未接入系统钥匙串，不应提交到版本库。旧版管理记录会在第一次保存时迁入新格式。

运行中的网关会读取已提交的管理配置；切换直连或移出管理后，相应网关服务会失效。网关停止期间对服务作出的更改会在下次启动时加载并清理旧授权。**清除授权**删除本机令牌，服务端撤销需到服务商账户页面操作。

## 当前范围

### 远程服务器只读展示

切换到 Linux 服务器后，MCP Hub 展示该服务器上五个 Agent 的已有 MCP 卡片，名称后标注所属 Agent。扫描全局配置、已登记项目配置及 Claude 用户配置中的项目本地条目，遵循 Agent 目录覆盖与展示偏好。相同连接合并来源；仅名称相同但连接不同的配置保留为不同卡片。已停用但仍在配置中的 MCP 也会显示，展示不代表连接可用。

远程卡片无点击、托管、编辑、删除、启停、登录或测试连接操作。远程 MCP 模式保留原有 Agent、项目导航及右上角「+」按钮，暂不支持的入口置灰并提供说明，切换 Hub 时布局保持一致。搜索与顶部重新扫描可用。切回 Skill Hub 后 Agent、项目和新增入口恢复可用；本机 MCP 的管理方式不变。

远程接口只返回名称与 Agent 归属，不回传原始定义、URL、命令参数、环境变量或请求头，不读取授权缓存，不启动 MCP 或恢复配置事务。读取失败会保留其他可读来源并显示提示。升级应用后重连服务器会按现有部署流程加载新辅助程序；自定义旧版组件需一并更新。

### 本机管理

- 本机 Claude Code、Codex、OpenCode v2、Pi（0.99.0+）和 Grok Build；不包含插件在运行时注入的 MCP，也不遍历未登记的项目目录。
- 自定义 Claude 配置目录暂不扫描或写入其全局 MCP，界面会提示；可使用项目作用域。
- 直连可保留扩展字段和相对工作目录。分发到其他位置后，相对路径的含义由目标 Agent 的工作目录决定。
- SSE 可以作为 Claude 直连配置管理；Codex、OpenCode v2、Pi、Grok 的本次 Studio 接入及 Studio 网关仅支持 stdio 和 Streamable HTTP。
- 网关不支持客户端专有扩展字段、变量引用、相对工作目录、OAuth confidential client／client secret、资源订阅、动态列表通知、sampling、elicitation、tasks。选择网关时会提示已知不兼容的配置。
- 连接测试确认 Studio 能访问上游，不代表 Agent 已重载配置。真实服务商 OAuth 仍需逐个验证；自动测试使用本机模拟服务。

## 开发验证

需要 Rust 1.88 或更新版本。管理、原生配置转换、事务恢复和网关位于 `crates/mcp`；新增编辑器位于 `src/components/mcp/McpEditor.tsx`。

```sh
cargo test -p skill-studio-mcp
cargo clippy -p skill-studio-mcp -p skill-studio --all-targets -- -D warnings
```

协议与认证采用 [官方 Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)。配置格式参考 [Claude Code MCP](https://code.claude.com/docs/en/mcp) 和 [Codex MCP](https://developers.openai.com/codex/mcp)。新增流程参考 [CC Switch 的 MCP 表单](https://github.com/farion1231/cc-switch/blob/06082e189d65e6d6dbadc35dacdac1ce6c79d89a/src/components/mcp/McpFormModal.tsx)。

新增 Agent 格式依据：[OpenCode v2 MCP](https://opencode.ai/v2/docs/mcp-servers)、[Pi 原生 MCP](https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/mcp.md)、[Grok Build MCP](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/07-mcp-servers.md)。
