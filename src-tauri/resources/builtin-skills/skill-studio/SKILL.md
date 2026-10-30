---
name: skill-studio
description: 用 Skill Studio 命令行管理 Skill 与 MCP：安装或托管到 Hub、管理备份与设置、按分组或项目部署到 Claude Code、Codex、OpenCode、Pi、Grok，检查待应用修改与配置冲突。
---

# Skill Studio

三层：**Hub（有什么）**、**分组**、**项目（给谁用）**。桌面和 CLI 共用
`~/.skill-studio/`，改动会实时刷新。Hub 不启用 Agent，部署只走分组或项目。

**先查看，再修改。** 不要手改 Studio 或 Agent 的配置，也不要直接搬删 Skill：
那会绕过配置锁、备份、来源记录和冲突检查。找不到命令时，在桌面设置里查看命令行位置。

## 先看现状

```bash
skill-studio status
skill-studio doctor
skill-studio agents
skill-studio skills list
skill-studio groups show 前端
skill-studio projects show .              # 当前目录所在项目
skill-studio projects skills list .       # 项目里已有的原件、部署副本与停用记录
skill-studio mcp list
skill-studio settings show
skill-studio backups list                 # config / skill / hub / mcp 分别列出
```

加 `--json` 给脚本解析，错误也有 `error.code/message/hint/candidates`。
同名时不要猜：Skill 文件操作需按候选路径或 ID 选具体来源。

## Skill 安装与生命周期

```bash
skill-studio skills search pdf
skill-studio skills install owner/repo@pdf
skill-studio skills discover ~/my-skills  # 只扫描候选，不导入
skill-studio skills import ~/my-skill     # 复制到 Hub，原件独立保留
skill-studio skills adopt ~/.claude/skills/rex --yes
skill-studio skills release ~/.skill-studio/skills/rex --yes
skill-studio skills delete ~/.claude/skills/rex --yes
skill-studio skills prune --yes           # 清理失效引用，不删除文件
```

`adopt` 搬原件到 Hub，并留下 Agent 入口；共享目录原件的入口改放所属 Agent 私有目录。
`release` 按托管来源记录还原，不覆盖外部修改。正在使用、内置或外部来源有保护检查。
删除按列表中的具体来源操作，不跟随列表外的末级软链接；目录别名仍可用。
普通目录保留备份；项目列表里的软链接只移除链接、不删除目标。

装完按需选分组或项目：

```bash
skill-studio groups create 前端 --skills pdf,react
skill-studio groups enable 前端 --agent claude-code
skill-studio groups enable 前端 --agent codex   # 与其他已启用分组叠加
skill-studio projects create ~/work/app --name app
skill-studio projects add app --skills pdf --groups 前端
skill-studio projects enable app --agent codex
skill-studio projects set app --link-mode copy
skill-studio projects gitignore app
```

成员保存会写入已启用的 Agent；启停只动所选 Agent，不影响其他分组。

## 项目原件

```bash
skill-studio projects skills list app
skill-studio projects skills disable app ./原路径
skill-studio projects skills enable app ./原路径
skill-studio projects skills adopt app ./原路径
skill-studio projects skills delete app ./原路径 --yes
```

原路径以列表为准。项目 `adopt` 是复制收录，保留项目文件，与全局搬移式托管不同。
部署副本不能逐项删改，须从项目/分组移出成员或停用。

## 管理 MCP

```bash
skill-studio mcp search context7 --json
skill-studio mcp add context7 --url https://mcp.context7.com/mcp
skill-studio mcp add github --env GITHUB_TOKEN=xxx -- npx -y @modelcontextprotocol/server-github
skill-studio mcp groups create 常用 --mcp context7,github
skill-studio mcp groups enable 常用 --agent claude
skill-studio mcp projects add app --mcp context7 --groups 常用
skill-studio mcp projects enable app --agent claude
skill-studio mcp set github --env GITHUB_TOKEN=新的令牌
skill-studio mcp set github --cwd ~/work/app
skill-studio mcp set context7 --config '{"type":"http","url":"https://mcp.context7.com/mcp"}'
skill-studio mcp test context7 --timeout 30 --json
```

搜索返回 `items/nextCursor` 和可用安装命令；需手动配置的候选不会猜参数。
完整编辑支持类型、直连/网关模式、工作目录、扩展字段、Client ID 与 scopes，见帮助。
连接测试有总超时，只验证 Studio 的握手/工具列表，不代表 Agent 已重载或登录。
网关测试须已有本机网关；CLI 不自动启动网关或打开授权。直连认证由 Agent 完成。
SSE 测试仍由 Agent 完成。服务器仍只支持 Agent 直连；网关启停/OAuth 由桌面处理。
密钥在输出和日志中隐藏；`--env/--config` 仍可能进入 shell 历史，避免放入分享记录。

## 设置与备份

```bash
skill-studio settings set --default-link-mode copy
skill-studio settings set --agent-dir codex=~/custom-codex
skill-studio settings set --preserve-manual-skills false --yes
skill-studio settings set --unmanage-agent pi --yes
skill-studio settings set --manage-mcp false --yes
skill-studio settings set --backup-keep 10 --yes
skill-studio backups list skill
skill-studio backups restore skill <ID> --yes
skill-studio backups restore config <ID> --yes
skill-studio backups delete hub --unused --yes
skill-studio backups delete mcp --unrestorable --yes
```

管理开关和手动 Skill 策略必须分别保存；目录不自动迁移，使用中的目录不可改。
CLI 关闭后不能通过 CLI 重新开启，必须回桌面。恢复网关接入须先在桌面开启管理。
备份类型：`config` 配置、`skill` 删除文件、`hub` 托管完整快照、`mcp` MCP 配置。
恢复不会覆盖同名文件或外部修改；使用中的完整快照不可删、不能单独恢复。
损坏配置时只允许显式 `backups list config` / `backups restore config`，不放行其他命令。

## 应用修改与删除

```bash
skill-studio groups apply 前端
skill-studio projects apply app
skill-studio mcp groups apply 常用
skill-studio mcp projects apply app
skill-studio groups disable 前端 --agent codex
skill-studio groups delete 前端 --yes
skill-studio projects delete app --yes       # 只移除记录，不动项目文件
skill-studio mcp remove context7 --yes        # 移出 Hub 并还原原配置
```

正在使用的分组/项目先停用再删。删除/移除/恢复或管理策略变更需 `--yes`，
**先向用户说明影响并取得确认**；不要把 `--yes` 当成用户授权。
「原样留下」表示 Studio 不覆盖外部修改，需人工核对。

## 退出码与重试

0 成功；1 操作失败；2 用法错误；3 找不到或有歧义；4 写入冲突，重试；5 需要 `--yes`。
`changed: false` 但 `pending: true`：记录没变，部署还欠着，运行 `apply`。
完整参数见 `references/commands.md`（由 `skill-studio docs` 生成）。
