# Plugin Hub（本机首版）

Plugin 是完整能力包，不是另一种 Skill。Studio 只识别和管理原生包，Agent 负责加载、执行、信任与权限。

## 固定非目标

以下四项完全不做，不是后续待办，也不预留接口或配置：

- 自有插件运行时。
- Agent 间自动格式转换。
- 将现有分组自动导出成插件。
- 为支持标准而重构现有 Skill/MCP 管理。

不新增 Agent。插件分组、项目接入和远程管理不做（用户已确认，与固定非目标同级别）；管理动作为安装、启用、停用、更新、卸载及停用记录维护。

## 功能与支持范围

在顶部切换到 **Plugin**。默认只读扫描；「开启插件管理」或设置里的「启用插件管理」仅允许后续明确操作，不自动启用、卸载或迁移现有插件。关闭管理只阻止 Studio 写入，Agent 当前启停不变。

| Agent | 读取 | 写入 |
|---|---|---|
| Claude Code | 用户安装记录、配置、skills-dir 插件及组件 | 已验证接口版本 2.1.268+（2.x）：user 安装、启停、更新、卸载；项目、local、managed 只读。skills-dir 只启停 |
| OpenCode | v2 包声明（含旧版 plugin 键）、可定位 node_modules 或 npm 缓存、本地扩展 | 已验证接口版本 2.3.2+（2.x）：默认 XDG 全局配置中的包。原生安装、更新、移除；启停撤回/还原精确包声明（按声明所在键定位），不猜执行代码才能知道的插件 ID |
| Pi | Package 声明、托管 npm/Git/本地包、自动发现扩展 | 已验证接口版本 1.6.0+（1.x）：个人 Package 原生安装、更新、移除；整包停用用四类资源空筛选，启用恢复原筛选 |
| Codex | 个人插件配置、可唯一定位的缓存包 | 只读，不把多份缓存中的最新目录猜成当前版本 |
| Grok Build | 用户插件目录、显式配置路径和名称启停声明 | 只读；不猜完整 ID、信任、市场缓存或 Claude 兼容加载状态 |
| DeepSeek Harness | 用户 Profile 选中的 Bundle；同一 Bundle 被多个 Profile 选中时聚合为一行并列出 Profile | 只读；不挂载 Profile、不执行包代码，不把 Profile 当项目 |

旧版 CLI、CLI 缺失、原生帮助不匹配、新的未验证主版本、重复包声明，以及不能安全对应原生写入位置的目录覆盖均降级只读。OpenCode 自动发现目录、以及在 node_modules 和 npm 缓存都定位不到包文件的字符串规则（无法区分是包名还是 ID）只读；Pi 单文件扩展的整包过滤尚未验证，只允许原生移除其声明。原生可保留的数据尽量保留，但卸载可能删除选项或密钥；配置快照不等于插件或运行数据备份。

## 标准包和组件

支持识别 Agent Plugins 1.0.0：根 `plugin.json`、固定 `skills/` 和 `mcp.json`。格式版本、字段类型、组件边界和包内真实路径均检查；未知顶层字段按规范报告并忽略，客户端扩展不推断可用性，一个 MCP 错误不连带禁用独立 Skill。

标准包识别不代表所有 Agent 都支持运行它。首版不将本地标准包自动接入未验证标准兼容的写入 Agent，不自动生成清单或市场。

包内 Skill/MCP 在各 Hub 的「插件提供的…」区关联展示，始终只读，不计入独立部署。完整插件目录不能作为 Skill 迁移或同步源。预览只读清单和文件名，不 import JS/TS，不连接插件 MCP。

## 命令行

```sh
skill-studio plugins list --json
skill-studio plugins show <列表中的ID> --json
skill-studio plugins inspect /绝对路径 --agent pi --json
skill-studio plugins agents --json
skill-studio settings set --manage-plugins true --yes
skill-studio plugins install npm:@example/pi-tools --agent pi --yes
skill-studio plugins install name@marketplace --agent claude --marketplace owner/repo --yes
skill-studio plugins disable <ID> --yes
skill-studio plugins enable <ID> --yes
skill-studio plugins update <ID> --yes
skill-studio plugins uninstall <ID> --yes
skill-studio plugins forget <ID> --yes
skill-studio plugins recover --yes
```

修改前先说明影响并取得确认，`--yes` 不是授权来源。同名或多作用域使用具体 ID。CLI 安装相对本地路径按当前目录解析，桌面本地路径建议填写绝对路径；原生已有声明的相对路径仍按原生配置目录解释。不会把 Pi 的 `update pi/self` 当插件更新。

所有写入复用 Studio 跨进程配置锁，原生命令不占用内存读写锁。写前复核用户确认时的插件修订，外部改动拒绝覆盖。命令有总超时并清理自有进程树；不增加 `--trust`、`--yes` 或项目授权等原生跳过标志。需要原生交互授权时停止，须在 Agent 中完成。

## 状态、备份与失败

「配置启用」「文件可读」「原生命令完成」都不等于当前会话已加载。命令结束后重读原生声明；无法确认目标状态时返回 `pending: true`，CLI 非零退出。检查标准包失败也非零退出，但诊断仍在 JSON 正文。

写操作前在 `~/.skill-studio/plugins/backups/<ID>/` 保存 0600 的原生配置快照。停用记录保存在私有 `plugins/catalog.json`；不把原生选项、密钥或整份配置放进列表、组件详情或日志。

直接配置补丁用 `pending-toggle.json` 保护配置与停用记录的两步提交。`plugins recover` 只核对哈希并完成或清除管理记录，不改 Agent 配置；配置或记录已被外部修改则停止，保留诊断标记。`plugins forget` 只清除 Studio 的停用记录，原生配置不变。

原生命令可能部分完成。Studio 不伪造安装、更新和卸载的整体事务回滚，也不会为了回滚用整份旧配置覆盖外部变更。

## 验证边界

自动测试使用独立主目录和替身 CLI，验证真实命令参数、私有配置快照、启停/还原、跨模块配置保留、外部冲突、只读范围、凭据隐藏、进程超时、导航和组件关联。界面验证使用模拟 IPC，不修改本机现有插件。发布前仍须按要支持的 CLI 版本在隔离真实环境验证下载、交互授权和 Agent 会话重载。
