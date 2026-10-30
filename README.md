<p align="center">
  <img src="src/assets/skill-studio.png" width="96" alt="Skill Studio Logo" />
</p>

<h1 align="center">Skill Studio</h1>

<p align="center">集中管理 Claude Code、Codex、OpenCode、Pi 和 Grok Build 的 Skills，从本机到远程 Linux 服务器。</p>

<p align="center">
  <a href="https://github.com/tarnish233/skill-studio/releases/latest">下载安装</a> ·
  <a href="CHANGELOG.md">更新日志</a> ·
  <a href="https://github.com/tarnish233/skill-studio/issues">反馈问题</a>
</p>

## 安装

### macOS

支持 Apple Silicon 和 Intel，需 macOS 12 或更高版本。

```sh
brew install --cask tarnish233/tap/skill-studio
```

也可以下载 `.dmg`，打开后将 Skill Studio 拖入「应用程序」。

### Windows / Linux

前往 [最新版本](https://github.com/tarnish233/skill-studio/releases/latest) 下载安装包：

| 系统        | 安装包                |
| ----------- | --------------------- |
| Windows x64 | `.exe` 或 `.msi`      |
| Linux x64   | `.AppImage` 或 `.deb` |

AppImage 需先在文件属性中允许作为程序执行，再双击打开。

## 功能

- **Skill Hub**：集中查看、搜索和托管 Skills，预览内容与 Token 估算；支持在线搜索安装和本地目录导入。
- **MCP Hub**：自动扫描 Claude Code、Codex、OpenCode v2、Pi（原生 MCP，0.99.0+）与 Grok Build 配置，支持表单、JSON／TOML 和模板添加，一次同步多个 Agent；直连管理与共享授权网关可独立使用。[使用说明](docs/mcp.md)
- **Agent 管理**：为 Claude Code、Codex、OpenCode、Pi 和 Grok Build 配置 Skills，按使用场景分组，一键启用或停用。
- **项目配置**：为每个项目选择 Agent 和 Skills，让不同项目使用各自的配置。
- **远程服务器**：读取 SSH config，选择 Linux 服务器后管理它的 Skills、分组、项目与备份；MCP Hub 只读展示服务器已有 MCP，在名称后标注所属 Agent。支持密钥、密码和跳板机连接。
- **备份与恢复**：支持配置备份和已删除 Skill 的恢复；提供浅色、深色与跟随系统外观。
- **命令行**：随应用一起安装命令行程序（Homebrew 安装时链接为 `skill-studio`），Agent 和脚本可以查看、管理 Skill 与 MCP 的 Hub、分组与项目，改动实时出现在应用里；连接过的服务器上也能直接用。[用法](#命令行)

## 支持的 Skill 目录

| Agent       | 全局安装目录                | 项目安装目录       |
| ----------- | --------------------------- | ------------------ |
| Claude Code | `~/.claude/skills`          | `.claude/skills`   |
| Codex       | `~/.codex/skills`           | `.agents/skills`   |
| OpenCode    | `~/.config/opencode/skills` | `.opencode/skills` |
| Pi          | `~/.pi/agent/skills`        | `.pi/skills`       |
| Grok Build  | `~/.grok/skills`            | `.grok/skills`     |

Codex、OpenCode、Pi 和 Grok Build 同时扫描共享目录 `~/.agents/skills`，写入时使用各自的专属目录。支持环境变量指定路径，以及「设置 → 目录」中的手动覆盖。OpenCode 遵循 `XDG_CONFIG_HOME`，额外扫描 `OPENCODE_CONFIG_DIR/skills`；Pi 使用 `PI_CODING_AGENT_DIR`，Grok Build 使用 `GROK_HOME`。

本版为 OpenCode、Pi 和 Grok Build 接入标准目录中的 `SKILL.md` Skill 管理，包括托管、安装、移除、备份恢复、分组和项目部署；Skill 管理不读取或修改它们的原生启停／权限配置。MCP 接入方式见 [MCP Hub](docs/mcp.md)。分组停用只撤回 Studio 部署的副本，手动 Skill 始终保留。修改后在对应 Agent 中重新加载 Skill 或重启会话。新 Agent 的跨厂商兼容加载、配置中额外声明的资源和插件目录不在本次接入范围内。

## 界面预览

以下为当前界面的示例数据截图。

**Skill Hub · 集中查看与托管**

![Skill Hub：来源筛选、托管状态与 Skill 内容概览](docs/images/skill-hub.png)

**Agent · 按场景组织分组**

![Agent 管理：Skill 分组与启用状态](docs/images/agent-groups.png)

**项目 · 独立配置 Skills**

![项目管理：为不同项目配置 Agent 与 Skills](docs/images/projects.png)

## 开始使用

1. 打开应用，在 Hub 查看已有 Skills，或点击右上角 `+` 安装、导入。
2. 进入对应 Agent 页面，创建并启用需要的分组。
3. 需要项目配置时，进入「项目」添加目录，选择 Skills 后保存并写入。

远程管理可在「设置 → 服务器」中启用并添加连接。切换到服务器后，Skills、项目、目录和备份均属于该服务器；切回「本机」即可继续本地管理。

服务器上的 MCP 与本机一样管理：Hub、托管与还原、分组、项目、备份都可用，连接测试在服务器上进行。服务器上只支持 Agent 直连，Studio 网关和浏览器授权仍只在本机运行。更新应用后，已有服务器连接需重新连接以加载新组件。

## 命令行

用 Homebrew 安装或升级时会同时装好 `skill-studio` 命令；Linux 的 deb 装好后是 `skill-studio-cli`，Windows 在安装目录里有 `skill-studio-cli.exe`，其他情况可在「设置 → 关于 → 命令行工具」查看位置。它和应用共用同一份配置，适合交给 Agent 或脚本：

```sh
skill-studio status                                   # 总览：Agent、分组与启用情况、项目、MCP、需要检查的问题
skill-studio groups create 笔记 --skills obsidian-markdown,json-canvas
skill-studio groups enable 笔记 --agent codex          # 为 Codex 启用，与它已启用的其他分组叠加
skill-studio projects add . --skills pdf-tools         # 「.」表示当前目录所在的项目，保存即写入已启用的 Agent
skill-studio --json skills list --agent claude-code    # 输出 JSON
skill-studio skills adopt ~/.claude/skills/rex --yes  # 搬原件到 Hub 并留入口，多来源按路径选
skill-studio skills delete ~/.claude/skills/rex --yes # 只删所选来源，普通目录保留备份
skill-studio backups list skill
skill-studio backups restore skill <备份ID> --yes
skill-studio settings show
skill-studio settings set --preserve-manual-skills false --yes
skill-studio projects skills list .                  # 项目原件的单项启停/收录/删除
skill-studio projects set . --link-mode copy
skill-studio projects gitignore .

skill-studio mcp add github --env GITHUB_TOKEN=xxx -- npx -y @modelcontextprotocol/server-github
skill-studio mcp add context7 --url https://mcp.context7.com/mcp
skill-studio mcp groups create 常用 --mcp github,context7
skill-studio mcp groups enable 常用 --agent claude,codex
skill-studio mcp projects add . --mcp context7 --claude local   # Claude Code 写到只给自己用的项目配置
skill-studio mcp set github --env GITHUB_TOKEN=新的令牌            # 改完用 mcp groups apply 写进已启用的 Agent
skill-studio mcp set github --cwd ~/work/app                   # 完整定义可用 --config 替换
skill-studio mcp search context7 --json                       # 官方 Registry，支持 --cursor
skill-studio mcp test context7 --timeout 30 --json             # 一次性握手与工具列表验证
```

查看类命令只读，不写任何文件，在只允许读的 Agent 沙箱里也能用；输出里的 MCP 定义会隐藏密钥（环境变量、请求头、网址参数）。搬移式托管/还原、文件删除、备份恢复/删除和管理策略变更要加 `--yes` 确认。备份分为 `config`、`skill`、`hub`、`mcp`；使用中的完整快照不可删，恢复不会覆盖外部修改。和应用同时修改时会等对方写完，不会互相覆盖。退出码：0 成功，1 操作失败，2 用法错误，3 找不到或名称有歧义，4 别的程序正在修改（重试即可），5 需要 `--yes` 确认。运行 `skill-studio --help` 查看全部命令。

MCP 连接测试可由 CLI 执行，超时会取消；只验证 Studio 能握手和列出工具，不代表 Agent 已重新加载或完成登录。网关测试须已有运行中的本机网关，CLI 不自动启动网关或授权；网关启停与浏览器授权仍在应用里做，网关模式的 MCP 要在装有应用的电脑上才能写进 Agent。设置的管理开关需单独保存，CLI 被关闭后必须在桌面重新开启。配置损坏时只放行明确的配置备份查看/恢复，不放行其他管理命令。应用连接过的服务器上，同一个命令行在 `~/.skill-studio/bin/skill-studio`，可以把这个目录加进 `PATH`。

## 许可证

[MIT License](LICENSE) © 2026 tarnish233
