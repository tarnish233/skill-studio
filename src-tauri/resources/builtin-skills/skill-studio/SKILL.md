---
name: skill-studio
description: 用 Skill Studio 命令行查看和管理 Skill 与 MCP：装进 Hub、按分组或项目部署到 Claude Code、Codex、OpenCode、Pi、Grok，检查「有待应用修改」与配置冲突。用户提到把这个 skill 分享给别的 Agent、给某个项目装 skill、管理 MCP、或问「哪些 Agent 在用」时使用。
---

# Skill Studio

Skill Studio 把 Skill 与 MCP 分成三层：**Hub（有什么）**、**分组**、**项目（给谁用）**。
桌面应用和命令行读写同一份配置（`~/.skill-studio/`），一边改动，另一边会实时刷新。

**先查看，再修改。** 不要手改 `~/.skill-studio/config.json`，也不要直接改各 Agent 的
`settings.json` / `config.toml` / `.mcp.json` —— 那会绕过配置锁、备份和冲突检查。

找不到 `skill-studio` 命令时，看 Skill Studio 设置里的「命令行工具」，按提示链接或使用内置程序。

## 三层

| 层 | 是什么 | 命令 |
| --- | --- | --- |
| Skill Hub | 有哪些 skill；真身在 `~/.skill-studio/skills/` 或某个 Agent 目录 | `skills` |
| 分组 | 一组 skill，每个 Agent 单独启用，可叠加多个分组 | `groups` |
| 项目 | 项目目录 + 成员，写进这个项目里各 Agent 的 skill 目录 | `projects` |

MCP 同理：`mcp`（Hub）、`mcp groups`、`mcp projects`。Hub 只负责「有什么」，
不启用任何 Agent —— 部署只通过分组或项目。

## 先看现状

```bash
skill-studio status            # 总览：Agent、分组、项目、需要检查/有待应用的修改
skill-studio doctor            # 自检：配置能否读取、锁、Agent 目录、问题清单
skill-studio agents            # 每个 Agent 是否安装、是否在管理中、skill 目录
skill-studio groups show 前端   # 成员、在哪些 Agent 上启用、问题
skill-studio projects show .    # 「.」表示当前目录所在的项目
skill-studio mcp list
```

加 `--json` 给脚本解析，错误也会输出 JSON（`error.code` / `error.message` /
`error.hint` / `error.candidates`）。

## 安装 skill

```bash
skill-studio skills search "pdf"              # 在 skills.sh 上搜
skill-studio skills install owner/repo@pdf    # 装进 Hub，不启用
skill-studio skills import ~/my-skill         # 本地目录导进 Hub
```

装完二选一：

**分组**：几个 Agent 或项目共用同一套 skill。

```bash
skill-studio groups create 前端 --skills pdf,react
skill-studio groups enable 前端 --agent claude-code
skill-studio groups enable 前端 --agent codex      # 与它已启用的其他分组叠加
```

**项目**：只在这个项目里用。

```bash
skill-studio projects create ~/work/app --name app
skill-studio projects add app --skills pdf --groups 前端
skill-studio projects enable app --agent codex
```

保存成员会立即写入正在使用的 Agent；`enable`/`disable` 只动这一个 Agent，
别的 Agent 和别的分组不受影响。

## 管理 MCP

```bash
skill-studio mcp add github --env GITHUB_TOKEN=xxx -- npx -y @modelcontextprotocol/server-github
skill-studio mcp add context7 --url https://mcp.context7.com/mcp
skill-studio mcp groups create 常用 --mcp context7,github
skill-studio mcp groups enable 常用 --agent claude
skill-studio mcp projects add app --mcp context7 --groups 常用
skill-studio mcp projects enable app --agent claude
```

- 换令牌：`mcp set github --env GITHUB_TOKEN=...`，再 `mcp groups apply 常用`
  （用到它的分组、项目会显示「有待应用修改」）。
- 密钥在输出和日志里一律隐藏成 `<已隐藏>`。
- `--gateway` 的 MCP 由桌面应用转发；命令行找不到桌面应用时只能管理直连 MCP。

## 有待应用修改

成员变了、副本过期，或别的程序改过接入时，卡片/`status` 会显示「有待应用修改」或
「需要检查」。按提示处理：

```bash
skill-studio groups apply 前端            # 按最新成员重新写入
skill-studio projects apply app
skill-studio mcp groups apply 常用
skill-studio mcp projects apply app
```

「原样留下」表示部署后被别的程序改写或替换：Studio 不覆盖也不删除，会在输出里列出，
需要人工核对。

## 停用与删除

```bash
skill-studio groups disable 前端 --agent codex   # 只让这个 Agent 不再使用
skill-studio projects disable app --agent codex
skill-studio groups delete 前端 --yes            # 只删记录，不动 Hub 里的 skill
skill-studio projects delete app --yes           # 只删记录，项目目录里的文件不动
skill-studio mcp remove github --yes             # 移出 Hub，并还原原来的配置
```

正在被 Agent 使用的分组/项目不能直接删除：先 `disable`，再删。删除、移除这类
不可撤销的操作必须加 `--yes`，并且要先向用户确认。

## 退出码与重试

| 码 | 含义 | 怎么办 |
| --- | --- | --- |
| 0 | 成功（包括「没有变化」） | 看 `changed` 判断是否真的改了 |
| 1 | 操作失败 | 读 `error.message` 和 `error.hint` |
| 2 | 用法错误 | 按提示改参数 |
| 3 | 找不到，或名称有歧义 | 从 `candidates` 里选，不要猜 |
| 4 | 别的程序正在修改配置 | 几秒后重跑同一条命令 |
| 5 | 需要 `--yes` 确认 | 先问用户，再加 |

`changed: false` 但 `pending: true`：记录没变，Agent 里的部署还欠着 —— 按提示跑 `apply`。

## 不要做

- 不要手改 Skill Studio 或 Agent 的配置文件；用命令行或桌面应用。
- 不要用 `--yes` 代替用户确认；删除前说清楚会影响哪些 Agent。
- 不要猜名称：同名时会列出候选，按候选或目录区分。
- 不要把令牌写进总结或提交；`--env` 的值仍会留在 shell 历史里。

完整参数见 `references/commands.md`（由 `skill-studio docs` 生成）。
