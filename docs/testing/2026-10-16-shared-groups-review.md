# 共享分组改造 Review

日期：2026-10-16。范围：`b659040..a0a4e70`，即 `cfdb710`（Skill / MCP 分组改为跨 Agent 共享、卡片上按 Agent 图标启停、侧栏收敛为 Hub / 分组 / 项目）与 `a0a4e70`（CI 修复），43 个文件，+1061 / −1087。

文中行号以 `a0a4e70` 为准。审查时未改代码；复现用的临时测试已删除。

**处理结果**：16 项 TODO 已全部完成，提交 `a357c7a`、`7f87f58`、`1cb4b58`；处理过程中另发现并修复一处启动预取回归（#9）。各条的实际做法与验证见「处理记录」。

**兼容性前提**：Skill Studio 目前只有作者本人使用，本轮允许破坏性更新，不为旧版本数据或旧导航状态做兼容。因此旧数据迁移相关的条目改为「不做」，并新增删除兼容代码的简化项 S8。真实数据已只读核对，见「真实数据核对」。

## 可信度标注

- **已复现** —— 用临时测试实际跑出结果，输出原样摘录在条目里。
- **已复核** —— 顺着代码链路逐段读过，未单独实跑。
- **外部依据** —— 引用规范原文，附链接。

## TODO

按优先级排列，编号对应下文详情。完成后勾选，并在条目后补提交号。

### 发布前必须

- [x] **#2** Agent 图标只做启用 / 停用，「应用修改」恢复为卡片上的独立按钮；删掉 `AgentToggleIcon` 的 `reapply` 参数；改写两条锁定旧行为的测试。（`1cb4b58`）

### 应修

- [x] **#3** 图标去掉 `aria-pressed`（或固定 label 后保留），label 带上分组名。（`1cb4b58`）
- [x] **#4** 「使用中」与删除保护按 `activeGroups` 全量判断；已关闭管理的 Agent 若仍在使用该 MCP 分组，保留一个只能停用的图标（或关闭管理时同步停用其 MCP 分组）。（`1cb4b58`）
- [x] **#T1** 补测试：已关闭管理的 Agent 不出现在分组卡片图标和 Agent 选择器里。（`1cb4b58`）

### 低优先级

- [x] **#5** Skill 分组页激活进行中时禁用全部图标。（`1cb4b58`）
- [x] **#6** 「有待应用修改」按集合比较成员，不比较顺序。（`1cb4b58`）
- [x] **#7** 未检测到安装的 Agent 图标调暗，并在提示中说明。（`1cb4b58`）
- [x] **#8** 新建分组的 `sortOrder` 用 `max + 1`。（`a357c7a`）

### 简化

- [x] **S1** 删除无界面调用的旧分组接口 `create_group` / `update_group` / `set_group_skills` / `apply_group`。（`a357c7a`）
- [x] **S2** 两个分组页的选中 Agent 改为派生值，抽出共用的 Agent 选择器。（`1cb4b58`）
- [x] **S3** 合并 `ViewId` 与 `StaticView`。（`7f87f58`）
- [x] **S4** 删除 `NavSwitcher` 已无调用方的 `badge`。（`7f87f58`）
- [x] **S5**（可选）侧栏三个单项分区合成一个。（`7f87f58`）
- [x] **S6** MCP `groups.rs` 内联 `fn agent()`，更新模块注释。（`a357c7a`）
- [x] **S7** `list_projects` 改用 `by_agent.values()`。（`a357c7a`）
- [x] **S8** 删除旧导航状态兼容：`initialView.ts` 中 `agent:<id>` 与旧 `library` / `mcp` 视图值的处理，`initialResource` 去掉 `view` 参数，相关测试改为直接设置新的存储键。（`7f87f58`）

### 处理中新发现

- [x] **#9** 恢复到 Skill 分组页时，启动阶段不预取分组和启用状态。（`7f87f58`）

### 不做（无需兼容）

- **#1** 旧版本同名分组的迁移 —— 真实数据没有重名分组。
- **发布说明** 项目分组语义变化、配置 v4 不能降级 —— 真实数据没有项目绑定分组，也不需要降级。

## 发布前必须

### 2. 分组「有待应用修改」时无法停用（已复现）

位置：`src/pages/GroupsPage.tsx:476`、`:492`；`src/pages/McpGroupsPage.tsx:201`、`:426`、`:447`；`src/components/common/AgentToggleIcon.tsx`（`reapply` 参数）

`changed` 为真时，图标点击固定发「重新应用」（`groupId = g.id`），卡片上再没有任何停用入口。

复现（临时测试，已删除）：

- Skill：Codex 的启用记录只含 `[a]`，分组成员已改成 `[a, b]`。卡片按钮为 `["为 Claude Code 启用","为 Codex 重新应用"]`。
- MCP：Claude 的启用记录引用的成员已不在目录中。卡片按钮为 `["为 Claude Code 重新应用","为 Codex 启用"]`。

后端在常见情况下会拒绝重新应用，而 `activate(None)` 能成功：

- Skill（`agent_groups.rs:169`、`:186`）：新加的成员在某个 Agent 上被手动停用、不可用或目录重名。分组共享后，为 Claude 改的成员会同时影响 Codex 的启用记录，这种情况更容易出现。
- MCP（`groups.rs:143`、`:161`）：从扫描结果引用的成员，其来源配置被手动改过或删掉，返回「来源已变化」或「分组包含已缺失的 MCP」。Hub 托管的条目在分组使用中不能删除（`management.rs:546`），所以缺失主要来自引用成员。

结果是用户只能先编辑分组才能把它关掉。旧界面（`b659040`）停用和应用是两个按钮：

- Skill：「停用」始终可点；`changed || unavailable` 时另有「应用修改 / 重新应用」。新界面里 `unavailable` 只能停用，这个重新应用入口也丢了。
- MCP：启用时始终有「重新应用」（有变化时叫「应用修改」），成员缺失时禁用。

另外，`McpGroupsPage.tsx:201` 的 `!found` 会让 `changed` 为真，与图标上方注释「issue 和成员已缺失只影响提醒点，点击仍是单纯停用」不符。

建议：图标只做启用 / 停用；任一启用中的 Agent 有待应用修改（Skill 还包括需要检查）时，卡片上显示「应用修改」按钮，依次重新应用这些 Agent，遇到第一个失败即停止并提示。`AgentToggleIcon` 随之去掉 `reapply`，提示文案只剩启用、停用、需要检查三种。

涉及测试：`tests/groupsPage.test.tsx:267`、`tests/mcpGroupsPage.test.tsx:143`，两条用例名就写着 "re-clicking the Agent icon re-applies instead of toggling off"，修复时需按新交互改写。

## 应修

### 3. 图标的 `aria-pressed` 与变化的 label 冲突（外部依据）

位置：`src/components/common/AgentToggleIcon.tsx:56`

按钮设置了 `aria-pressed`，而 `aria-label` 随状态在「启用 / 停用 / 重新应用」之间变化，读屏会读出「为 Codex 停用，切换按钮，已按下」这类重复且矛盾的信息。WAI-ARIA APG 按钮模式原文：

> Important: it is critical the label on a toggle does not change when its state changes.

同一段也说明，如果 label 随状态变化，就不需要 `aria-pressed`。来源：<https://www.w3.org/WAI/ARIA/apg/patterns/button/>

建议：最小改动是删掉 `aria-pressed`（现有测试按 label 查询，不受影响）；或固定 label 后保留 `aria-pressed`。此外每张卡片的图标 label 完全相同，按按钮列表浏览时分不清属于哪个分组，建议 label 带上分组名（测试里的查询需同步调整）。

### 4. 关闭 Agent 管理后，其 MCP 分组无法停用（已复核）

位置：`crates/service/src/state.rs:256`、`src-tauri/src/commands/mcp.rs:239`、`crates/mcp/src/management/groups.rs:93`、`src/pages/McpGroupsPage.tsx`（`agents` 过滤与 `activeStatuses`）

- 在设置里关闭某个 Agent 的管理会调用 `set_agent_management`，它只停用该 Agent 的 Skill 分组，不处理 MCP 分组。
- MCP 侧拒绝为已关闭的 Agent 启用分组（`commands/mcp.rs:239`），但允许停用。
- 新页面按 `disabledAgents` 过滤掉这个 Agent 的图标，`activeStatuses` 只看可见 Agent，于是卡片显示未使用、删除按钮可点；后端 `remove_group` 返回「请先停用分组」（`groups.rs:93`），界面上没有任何办法处理。

旧侧栏同样隐藏已关闭的 Agent，这条死路以前就有；新卡片额外给出了「未使用」的错误状态。真实数据中 MCP 分组 `LLM` 正对 Pi 启用，在设置里关闭 Pi 的管理就会遇到这条。

建议（二选一，或都做）：「使用中」与删除保护按 `activeGroups` 全量判断，正在使用该分组的已关闭 Agent 保留一个只能停用的图标；或在关闭管理时同步停用其 MCP 分组，与 Skill 侧「关闭管理即撤回部署」一致。

### T1. 补测试：已关闭管理的 Agent

旧的「hides disabled application navigation while keeping Hub available」随侧栏改造删除。目前没有用例覆盖「已关闭管理的 Agent 不出现在分组卡片图标和 Agent 选择器里」，`tests/groupsPage.test.tsx`、`tests/mcpGroupsPage.test.tsx` 中都没有 `disabledAgents` 场景。

## 低优先级

### 5. Skill 分组页允许并发激活（已复核）

位置：`src/pages/GroupsPage.tsx:470`

图标只在「同一个 Agent 正在操作」时禁用，可以同时为另一个 Agent 发请求。后端由 `config_mut()` 锁串行化（`crates/service/src/state.rs:261`），数据安全；但第一个请求结束时会清掉 `operationView` 快照，此时第二个请求仍在进行，卡片可能闪回中间状态。旧代码用 `activate.isPending` 禁用全部按钮，MCP 页也是全局 `busy`。建议改回全局禁用。

### 6. 「有待应用修改」比较了成员顺序（已复核）

位置：`src/pages/GroupsPage.tsx:182`、`src/pages/McpGroupsPage.tsx:197`

用 `JSON.stringify` 比较数组，取消再勾选同一成员会改变顺序，所有使用该分组的 Agent 都会提示有待应用修改，而部署结果与顺序无关。旧代码就有这个问题，共享后影响面变大。建议按集合比较。

### 7. 不再提示 Agent 未安装（已复核）

旧侧栏会为未检测到的 Agent 标注「未装 / 检测中 / 检测失败」（来自 `useAgents()` 的 `detected`、`isDetecting`、`detectionFailed`）。现在卡片上的图标看不出区别，点击会照常向该 Agent 的目录部署。建议调暗这些图标并在提示中说明，或只在已启用时显示。

### 8. 新建分组的 `sortOrder` 可能与已有值重复（已复核）

位置：`crates/core/src/services/agent_groups.rs:48`、`crates/mcp/src/management/groups.rs:83`

新分组用 `len()` 作为 `sortOrder`，删除过分组后可能与已有值相同，新分组不一定排在最后。旧代码就有。建议用 `max + 1`。

## 简化

### S1. 删除无界面调用的旧分组接口

`create_group`、`update_group`、`set_group_skills`、`apply_group` 已没有界面调用者，功能被 `save_group` 覆盖，而且各自维护一份重名校验。涉及：

- `src-tauri/src/commands/groups.rs`，以及 `src-tauri/src/lib.rs` 的命令注册；
- `crates/service/src/groups.rs`，以及 `crates/service/src/lib.rs` 的分发；
- `src/lib/api/groups.ts`、`tests/apiLayer.test.ts`、`tests/mocks/tauri.ts`；
- `Studio::apply_group`（`crates/core/src/services/studio.rs:760`）及 `crates/core/tests/studio.rs` 中的两处用例；删除后 `GroupApplyMode`（Rust 与 `src/types.ts`）也不再使用。

本轮去掉了 `apply_group` 里「Agent 分组请使用启用/切换操作」的限制，这个闲置接口现在能一次性部署任意分组，且不记入启用状态，与其留着不如删掉。

### S2. 选中 Agent 改为派生值

位置：`src/pages/GroupsPage.tsx:116`、`src/pages/McpGroupsPage.tsx:125`

用 `agents.find((a) => a.id === picked) ?? agents[0]` 直接计算，去掉 `useEffect` 和一次额外渲染，也不会再出现 `SkillBackups` 的 `scope="agent:undefined"`（`GroupsPage.tsx:334`）。两页重复的 `<select>` 及其注释可以抽成共用组件。

### S3. 合并 `ViewId` 与 `StaticView`

位置：`src/lib/initialView.ts:4`。两者已是同一类型，保留一个即可。

### S4. 删除 `NavSwitcher` 的 `badge`

位置：`src/components/common/NavSwitcher.tsx:14`、`:53`、`:94`。原用于 Agent 检测状态，侧栏改造后已无调用方。

### S5. 侧栏三个单项分区合成一个（可选）

位置：`src/App.tsx:144` 起。Hub、分组、项目各占一个只有一项的分区，各带一条分隔线，读屏会读出「分组 分组」。

### S6. MCP `groups.rs` 小整理

位置：`crates/mcp/src/management/groups.rs:1`、`:27`。`fn agent()` 只剩 `activate` 一个调用方，可以内联；模块注释仍写着 "Agent-local combinations"。

### S7. `list_projects` 简写

位置：`crates/service/src/projects.rs:93`。`by_agent.iter().any(|(_, skills)| …)` 可写成 `by_agent.values().any(|skills| …)`。

### S8. 删除旧导航状态兼容

位置：`src/lib/initialView.ts:15`–`:22`、`:31`–`:36`；`initialResource` 的调用方 `src/App.tsx:84`、`src/components/common/StartupBoundary.tsx:20`、`src/lib/initialView.ts:23`。

- `readInitialView` 只认 `"projects"` / `"groups"`，其他值一律回到 Hub，删掉 `agent:<id>` 映射到分组页的分支。
- `initialResource` 只读 `skill-studio-resource`，删掉 v0.5.8 留下的旧 `library` / `mcp` 视图值处理，`view` 参数随之去掉。
- 测试：
  - `tests/appShell.test.tsx:314` 是纯迁移用例，删除。
  - `tests/additionalAgents.test.tsx:67` 除迁移外还检查 Pi 的 MCP 使用说明，改为直接进入 MCP 分组页，保留说明的断言。
  - 其余用例只是借旧视图值进入 MCP 模式或分组页，改为直接设置 `skill-studio-resource` / `skill-studio-view`：`tests/appShell.test.tsx:327`、`tests/startup.test.tsx:66`、`:136`、`tests/remoteMcp.test.tsx:181`、`tests/mcpPage.test.tsx:332`、`tests/sidebar.test.tsx:36`、`:72`。
- 影响：本机 WebView 里若还存着旧值，更新后第一次启动会落在 Hub，此后正常记忆。

## 不做（无需兼容）

### 1. 旧版本同名分组升级后无法编辑（已复现）

旧版允许同一名称在每个 Agent 各存一份。改造后重名校验变成全局（`agent_groups.rs:31`、`crates/mcp/src/management/groups.rs:43`），而 v3→v4 迁移（`store.rs:80`）只把版本号加 1。复现结果：两个同名分组加载后都无法保存成员，分别返回 `Err("无效输入: 已存在同名分组")` 与 `Err("已存在同名分组")`；旧 MCP 分组的 `sortOrder` 按 Agent 各自编号，合并后出现并列值。

不做的原因：真实数据中 Skill 与 MCP 各只有一个分组，没有重名。以后若有其他用户，再补迁移：合并名称与成员都相同的分组，其余重名加 Agent 名后缀，MCP 分组重排 `sortOrder`。

### 发布说明

- 项目绑定的分组现在对项目里所有 Agent 生效；旧版中 Agent 专属分组只写入该 Agent 的项目目录。真实数据中 5 个项目都没有绑定分组，不受影响。
- 配置升级为 v4 后，0.5.8 及更早版本会拒绝启动（「配置版本 4 高于本程序支持的 3」）。本轮不考虑降级。

## 处理记录

2026-10-16 完成。以下记录与原建议不同或需要说明的做法。

| # | 提交 | 做法 |
| --- | --- | --- |
| #2 | `1cb4b58` | 图标只做启用 / 停用。「应用修改 / 重新应用」放在常显的图标行里（`RowActions` 只在悬停时显示），只在有待处理的 Agent 时出现，依次重新应用、遇到第一个失败即停；MCP 成员缺失时禁用。Skill 侧「需要检查」也恢复了重新应用入口。两页的图标行合并为共用的 `GroupAgentToggles`，取代 `AgentToggleIcon`。 |
| #3 | `1cb4b58` | 采用「去掉 `aria-pressed`、名称描述动作」：`为 Codex 停用「Dev」`。 |
| #4 | `1cb4b58` | 只做前端方案：「使用中」、绿色卡片和删除保护按全部启用记录判断；已关闭管理但仍在用该分组的 Agent 显示只能停用的图标，名称带「（已关闭管理）」。没有改成「关闭管理时自动停用 MCP 分组」：设置与 MCP 目录不在同一事务里，停用失败时会留下设置已改、分组仍在用的状态，界面出口更稳妥。 |
| #5 | `1cb4b58` | 任一启停或重新应用进行中，整行图标禁用。 |
| #6 | `1cb4b58` | 新增 `sameMembers`（`src/lib/utils.ts`），两页共用。 |
| #7 | `1cb4b58` | 新增 `agentDetection`（`src/lib/agents.ts`）：未检测到时名称和提示带「检测中 / 检测失败 / 未检测到安装」，只有确认未安装才调暗，仍可点击。 |
| #8 | `a357c7a` | core 与 MCP 都改为 `max + 1`。 |
| #9 | `7f87f58` | 处理 S8 时发现：`StartupBoundary` 仍以 `view.startsWith("agent:")` 决定是否预取分组与配置，`cfdb710` 之后这个分支不会再命中，恢复到 Skill 分组页时首帧没有分组和启用状态。改为对 `"groups"` 预取两者。 |
| S1 | `a357c7a` | 四个接口连同 `Studio::apply_group`、`GroupApplyMode` 一并删除；core 中原先经由 `apply_group` 的两个用例改为直接调用 `register` / `unregister`，覆盖不丢。 |
| S2 | `1cb4b58` | 选中 Agent 改为派生值；`<select>` 抽成 `AgentSelect`。MCP 页的 `run()` 改为接收一个操作，失败后也刷新一次状态（多步重新应用可能已部分生效）。 |
| S5 | `7f87f58` | `NavSwitcher` 改为接收平铺的 `items`，去掉分区、分隔线和 `NavSection` 类型。 |
| S8 | `7f87f58` | 两条纯迁移用例中，`appShell` 那条改为「恢复分组页与 MCP 模式」，`additionalAgents` 那条保留 Pi 说明的断言；其余借旧值进入 MCP 模式的用例改设新的存储键。 |

验证：

- Rust：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 全部通过。
- 前端：`pnpm typecheck`、`pnpm format:check`、`pnpm run build:renderer` 通过；`pnpm exec vitest run` 连续 3 次 31 个文件、244 项全部通过（处理前 238 项）。
- 「修复前失败」：#8 的 core 与 MCP 两条新用例、#9 的启动用例，都在临时退回修复后确认失败，恢复后通过。#2–#7 与 T1 的新用例针对旧实现的错误行为编写，但因图标名称同时改变，没有逐条在旧代码上回放。
- 没有在真实桌面应用中逐项操作；界面结论来自 jsdom 渲染测试与生产构建。

## 真实数据核对

2026-10-16 只读读取 `~/.skill-studio/config.json` 与 `~/.skill-studio/mcp/catalog.json`，未做任何修改：

- 配置仍是 v3，尚未被新版写回。
- Skill 分组 1 个：`LLM`（原属 Codex，1 个 skill），没有启用记录。
- MCP 分组 1 个：`LLM`（原属 Pi，1 个 MCP），正对 Pi 启用。
- 5 个项目都没有绑定分组。

## 已确认无问题

- 旧配置和旧 catalog 中残留的 `agentId` / `agent` 字段可以正常加载（相关结构体没有 `deny_unknown_fields`），下次保存时自动去掉；上一轮已用真实 `~/.skill-studio/config.json` 的副本验证。
- 远程 helper 随桌面端版本部署，连接时校验 `helperVersion` 与桌面端一致（`src-tauri/src/remote.rs:517`），命令改名不会出现新旧协议错配。
- 删除分组：Skill `delete_group` 检查全部 Agent 的启用记录并清理项目绑定；MCP `remove_group` 检查全部 `activeGroups`。
- 前端 `projectSkillIds` 与后端 `write_project` 口径一致，包括项目未勾选 Agent 时返回空。

## 方法与局限

- 逐文件审读 `b659040..a0a4e70` 的 diff，并与 `b659040` 的旧页面对照交互。
- #1、#2 用临时 Rust 集成测试和 vitest 用例实际复现，输出原样摘录；测试文件已删除。
- #3 依据 W3C APG 原文；#4–#8 为代码阅读结论，未实跑。
- 真实数据核对为只读。
- 没有在真实桌面应用中操作上述场景；涉及界面的结论来自 jsdom 渲染结果与代码阅读。
