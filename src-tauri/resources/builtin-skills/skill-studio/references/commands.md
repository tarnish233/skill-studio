# 命令行参考

由 `skill-studio docs` 生成，与当前版本一致；不要手改。

## skill-studio

Skill Studio 命令行：查看和管理 Skill 与 MCP 的 Hub、分组与项目，按 Agent 启用。
与桌面应用共用同一份配置，改动会实时出现在 Skill Studio 里。

用法：skill-studio [选项] <命令>

命令：
  status    总览：Agent、分组与各 Agent 的启用、项目、需要检查和有待应用的修改
  agents    列出 Agent：是否安装、是否在管理中、skill 目录
  skills    Skill Hub 里的 skill：列出、查看、搜索、安装、导入
  groups    分组：一组 skill，可为每个 Agent 单独启用，多个分组可以叠加
  projects  项目：把 skill 和分组写进项目目录，按 Agent 启用
  mcp       MCP：Hub 里的 MCP、MCP 分组与项目里的 MCP
  doctor    自检：配置能否读取、有没有未完成的写入、Agent 目录、需要检查的问题

选项：
      --json     输出 JSON，给脚本和 Agent 解析
  -y, --yes      确认删除这类不可撤销的操作
  -h, --help     显示帮助
  -V, --version  显示版本


加 --json 输出 JSON。退出码：0 成功，1 操作失败，2 用法错误，3 找不到或名称有歧义，4 别的程序正在修改（重试即可），5 需要加 --yes 确认。
名称可以写 skill、MCP、分组或项目的名字；项目也可以写目录，「.」表示当前目录所在的项目。
Agent 写 claude-code（或 claude）、codex、opencode、pi、grok。

## skill-studio status

总览：Agent、分组与各 Agent 的启用、项目、需要检查和有待应用的修改

用法：skill-studio status [选项]

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio agents

列出 Agent：是否安装、是否在管理中、skill 目录

用法：skill-studio agents [选项]

选项：
      --json   输出 JSON，给脚本和 Agent 解析
      --probe  同时检查各 Agent 的命令行能否运行（较慢）
  -y, --yes    确认删除这类不可撤销的操作
  -h, --help   显示帮助

## skill-studio skills

Skill Hub 里的 skill：列出、查看、搜索、安装、导入

用法：skill-studio skills [选项] <命令>

命令：
  list     列出 skill
  show     查看一个 skill：来源、所在分组与项目、在各 Agent 上的状态
  search   在 skills.sh 上搜索
  install  从 GitHub 安装到 Skill Hub；只放进 Hub，不会启用到任何 Agent
  import   把本地的 skill 目录导入 Skill Hub

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio skills list

列出 skill

用法：skill-studio skills list [选项]

选项：
  -a, --agent <Agent>  只列这个 Agent 上已启用的
      --json           输出 JSON，给脚本和 Agent 解析
  -g, --group <分组>     只列这个分组的成员
  -y, --yes            确认删除这类不可撤销的操作
  -h, --help           显示帮助
      --hub            只列托管在 Skill Hub 里的
  -q, --query <词>      按名称或说明筛选

## skill-studio skills show

查看一个 skill：来源、所在分组与项目、在各 Agent 上的状态

用法：skill-studio skills show [选项] <skill>

参数：
  <skill>  skill 名称、ID 或目录

选项：
      --content  同时输出 SKILL.md 全文
      --json     输出 JSON，给脚本和 Agent 解析
  -y, --yes      确认删除这类不可撤销的操作
  -h, --help     显示帮助

## skill-studio skills search

在 skills.sh 上搜索

用法：skill-studio skills search [选项] <搜索词>

参数：
  <搜索词>  搜索词

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio skills install

从 GitHub 安装到 Skill Hub；只放进 Hub，不会启用到任何 Agent

用法：skill-studio skills install [选项] <仓库>

参数：
  <仓库>  仓库，如 owner/repo，或 owner/repo@skill

选项：
      --json        输出 JSON，给脚本和 Agent 解析
      --skill <名称>  仓库里的 skill 名称（仓库里没写 @skill 时）
      --path <路径>   有多个版本时，选择仓库里的路径（可重复）
  -y, --yes         确认删除这类不可撤销的操作
  -h, --help        显示帮助

## skill-studio skills import

把本地的 skill 目录导入 Skill Hub

用法：skill-studio skills import [选项] <目录>

参数：
  <目录>  含 SKILL.md 的目录

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio groups

分组：一组 skill，可为每个 Agent 单独启用，多个分组可以叠加

用法：skill-studio groups [选项] <命令>

命令：
  list     列出分组
  show     查看一个分组：成员、在哪些 Agent 上启用、需要检查的问题
  create   新建分组
  rename   重命名分组
  add      加入成员，并写入正在使用这个分组的 Agent
  remove   移出成员，并写入正在使用这个分组的 Agent
  delete   删除分组（需要 --yes）：只删分组记录，不删 Hub 里的 skill
  enable   为 Agent 启用分组，与它已启用的其他分组叠加
  disable  为 Agent 停用分组，其他分组不受影响
  apply    应用修改：按最新成员重新写入正在使用这个分组的 Agent

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio groups list

列出分组

用法：skill-studio groups list [选项]

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio groups show

查看一个分组：成员、在哪些 Agent 上启用、需要检查的问题

用法：skill-studio groups show [选项] <分组>

参数：
  <分组>  分组名称或 ID

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio groups create

新建分组

用法：skill-studio groups create [选项] <名称>

参数：
  <名称>  分组名称

选项：
      --json            输出 JSON，给脚本和 Agent 解析
  -s, --skills <skill>  成员，逗号分隔
  -y, --yes             确认删除这类不可撤销的操作
  -h, --help            显示帮助

## skill-studio groups rename

重命名分组

用法：skill-studio groups rename [选项] <分组> <新名称>

参数：
  <分组>   分组名称或 ID
  <新名称>  新名称

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio groups add

加入成员，并写入正在使用这个分组的 Agent

用法：skill-studio groups add [选项] <分组> <skill>...

参数：
  <分组>        分组名称或 ID
  <skill>...  要加入的 skill

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio groups remove

移出成员，并写入正在使用这个分组的 Agent

用法：skill-studio groups remove [选项] <分组> <skill>...

参数：
  <分组>        分组名称或 ID
  <skill>...  要移出的 skill

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio groups delete

删除分组（需要 --yes）：只删分组记录，不删 Hub 里的 skill

用法：skill-studio groups delete [选项] <分组>

参数：
  <分组>  分组名称或 ID

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio groups enable

为 Agent 启用分组，与它已启用的其他分组叠加

用法：skill-studio groups enable [选项] --agent <Agent> <分组>

参数：
  <分组>  分组名称或 ID

选项：
  -a, --agent <Agent>  Agent，可重复或用逗号分隔
      --json           输出 JSON，给脚本和 Agent 解析
  -y, --yes            确认删除这类不可撤销的操作
  -h, --help           显示帮助

## skill-studio groups disable

为 Agent 停用分组，其他分组不受影响

用法：skill-studio groups disable [选项] --agent <Agent> <分组>

参数：
  <分组>  分组名称或 ID

选项：
  -a, --agent <Agent>  Agent，可重复或用逗号分隔
      --json           输出 JSON，给脚本和 Agent 解析
  -y, --yes            确认删除这类不可撤销的操作
  -h, --help           显示帮助

## skill-studio groups apply

应用修改：按最新成员重新写入正在使用这个分组的 Agent

用法：skill-studio groups apply [选项] <分组>

参数：
  <分组>  分组名称或 ID

选项：
  -a, --agent <Agent>  只写入这些 Agent
      --json           输出 JSON，给脚本和 Agent 解析
  -y, --yes            确认删除这类不可撤销的操作
  -h, --help           显示帮助

## skill-studio projects

项目：把 skill 和分组写进项目目录，按 Agent 启用

用法：skill-studio projects [选项] <命令>

命令：
  list     列出项目
  show     查看一个项目：成员、在哪些 Agent 上启用、需要检查的问题
  create   添加项目
  add      加入成员，并写入项目已启用的 Agent
  remove   移出成员，并写入项目已启用的 Agent
  enable   为 Agent 启用项目：把成员写进项目里这个 Agent 的 skill 目录
  disable  为 Agent 停用项目：撤回写进项目目录的 skill
  apply    应用修改：按最新成员重新写入项目已启用的 Agent
  delete   移除项目（需要 --yes）：只删记录，项目目录里的文件不动

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio projects list

列出项目

用法：skill-studio projects list [选项]

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio projects show

查看一个项目：成员、在哪些 Agent 上启用、需要检查的问题

用法：skill-studio projects show [选项] <项目>

参数：
  <项目>  项目名称、ID 或目录（「.」表示当前目录所在的项目）

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio projects create

添加项目

用法：skill-studio projects create [选项] <目录>

参数：
  <目录>  项目目录

选项：
      --json       输出 JSON，给脚本和 Agent 解析
      --name <名称>  项目名称，默认用目录名
  -y, --yes        确认删除这类不可撤销的操作
  -h, --help       显示帮助

## skill-studio projects add

加入成员，并写入项目已启用的 Agent

用法：skill-studio projects add [选项] <项目>

参数：
  <项目>  项目名称、ID 或目录（「.」表示当前目录所在的项目）

选项：
      --json            输出 JSON，给脚本和 Agent 解析
  -s, --skills <skill>  要加入的 skill，逗号分隔
  -g, --groups <分组>     要绑定的分组，逗号分隔
  -y, --yes             确认删除这类不可撤销的操作
  -h, --help            显示帮助

## skill-studio projects remove

移出成员，并写入项目已启用的 Agent

用法：skill-studio projects remove [选项] <项目>

参数：
  <项目>  项目名称、ID 或目录（「.」表示当前目录所在的项目）

选项：
      --json            输出 JSON，给脚本和 Agent 解析
  -s, --skills <skill>  要移出的 skill，逗号分隔
  -g, --groups <分组>     要解绑的分组，逗号分隔
  -y, --yes             确认删除这类不可撤销的操作
  -h, --help            显示帮助

## skill-studio projects enable

为 Agent 启用项目：把成员写进项目里这个 Agent 的 skill 目录

用法：skill-studio projects enable [选项] --agent <Agent> <项目>

参数：
  <项目>  项目名称、ID 或目录（「.」表示当前目录所在的项目）

选项：
  -a, --agent <Agent>  Agent，可重复或用逗号分隔
      --json           输出 JSON，给脚本和 Agent 解析
  -y, --yes            确认删除这类不可撤销的操作
  -h, --help           显示帮助

## skill-studio projects disable

为 Agent 停用项目：撤回写进项目目录的 skill

用法：skill-studio projects disable [选项] --agent <Agent> <项目>

参数：
  <项目>  项目名称、ID 或目录（「.」表示当前目录所在的项目）

选项：
  -a, --agent <Agent>  Agent，可重复或用逗号分隔
      --json           输出 JSON，给脚本和 Agent 解析
  -y, --yes            确认删除这类不可撤销的操作
  -h, --help           显示帮助

## skill-studio projects apply

应用修改：按最新成员重新写入项目已启用的 Agent

用法：skill-studio projects apply [选项] <项目>

参数：
  <项目>  项目名称、ID 或目录（「.」表示当前目录所在的项目）

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio projects delete

移除项目（需要 --yes）：只删记录，项目目录里的文件不动

用法：skill-studio projects delete [选项] <项目>

参数：
  <项目>  项目名称、ID 或目录（「.」表示当前目录所在的项目）

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp

MCP：Hub 里的 MCP、MCP 分组与项目里的 MCP

用法：skill-studio mcp [选项] <命令>

命令：
  list      列出 MCP：Hub 里的，以及各 Agent 配置里原有的
  show      查看一个 MCP：连接方式、所在分组与项目、配置在哪些地方（密钥一律隐藏）
  add       添加到 MCP Hub；只放进 Hub，不会写进任何 Agent，之后在分组或项目里选用
  set       修改 Hub 里的 MCP：改环境变量、请求头、网址、启动命令或名称。用到它的分组和项目 会显示有待应用修改
  remove    删除（需要 --yes）：Hub 里的 MCP 移出 Hub 并还原原来的配置；原有配置从 Agent 配置里删掉
  groups    MCP 分组：一组 MCP，可为每个 Agent 单独启用，多个分组可以叠加
  projects  项目里的 MCP：把 MCP 和 MCP 分组写进项目的配置，按 Agent 启用

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp list

列出 MCP：Hub 里的，以及各 Agent 配置里原有的

用法：skill-studio mcp list [选项]

选项：
  -a, --agent <Agent>  只列这个 Agent 配置里有的
      --json           输出 JSON，给脚本和 Agent 解析
  -q, --query <词>      按名称、命令或网址筛选
  -y, --yes            确认删除这类不可撤销的操作
  -h, --help           显示帮助

## skill-studio mcp show

查看一个 MCP：连接方式、所在分组与项目、配置在哪些地方（密钥一律隐藏）

用法：skill-studio mcp show [选项] <MCP>

参数：
  <MCP>  MCP 名称或 ID

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp add

添加到 MCP Hub；只放进 Hub，不会写进任何 Agent，之后在分组或项目里选用

用法：skill-studio mcp add [选项] [名称] [-- <命令>...]

参数：
  [名称]     MCP 名称（用 --config 时可省略，取配置里的名称）
  [命令]...  启动命令和参数，写在 -- 之后

选项：
      --json          输出 JSON，给脚本和 Agent 解析
      --url <网址>      远程 MCP 的网址
  -H, --header <请求头>  请求头，写成 "名称: 值" 或 名称=值，可重复
  -y, --yes           确认删除这类不可撤销的操作
  -e, --env <KEY=值>   环境变量 KEY=值，可重复
  -h, --help          显示帮助
      --config <配置>   从说明里复制的配置：JSON、TOML、codex / claude mcp add 命令或网址
      --gateway       由 Skill Studio 网关转发（需要桌面应用）；默认 Agent 直连


例子：
  skill-studio mcp add github --env GITHUB_TOKEN=xxx -- npx -y @modelcontextprotocol/server-github
  skill-studio mcp add context7 --url https://mcp.context7.com/mcp
  skill-studio mcp add --config '{"mcpServers":{"fetch":{"command":"uvx","args":["mcp-server-fetch"]}}}'

## skill-studio mcp set

修改 Hub 里的 MCP：改环境变量、请求头、网址、启动命令或名称。用到它的分组和项目 会显示有待应用修改

用法：skill-studio mcp set [选项] <MCP> [-- <命令>...]

参数：
  <MCP>    MCP 名称或 ID
  [命令]...  换启动命令和参数，写在 -- 之后（本地启动的 MCP）

选项：
      --json               输出 JSON，给脚本和 Agent 解析
      --name <新名称>         改名
  -e, --env <KEY=值>        设置环境变量 KEY=值，可重复
  -y, --yes                确认删除这类不可撤销的操作
  -h, --help               显示帮助
      --unset-env <KEY>    删掉环境变量，可重复
  -H, --header <请求头>       设置请求头，写成 "名称: 值" 或 名称=值，可重复
      --unset-header <名称>  删掉请求头，可重复
      --url <网址>           换网址（远程 MCP）


例子：
  skill-studio mcp set github --env GITHUB_TOKEN=新的令牌
  skill-studio mcp set context7 --header "Authorization: Bearer xxx"
  skill-studio mcp set fetch -- uvx mcp-server-fetch --ignore-robots-txt

## skill-studio mcp remove

删除（需要 --yes）：Hub 里的 MCP 移出 Hub 并还原原来的配置；原有配置从 Agent 配置里删掉

用法：skill-studio mcp remove [选项] <MCP>

参数：
  <MCP>  MCP 名称或 ID

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp groups

MCP 分组：一组 MCP，可为每个 Agent 单独启用，多个分组可以叠加

用法：skill-studio mcp groups [选项] <命令>

命令：
  list     列出 MCP 分组
  show     查看一个分组：成员、在哪些 Agent 上启用、需要检查的问题
  create   新建分组
  rename   重命名分组
  add      加入成员，并写入正在使用这个分组的 Agent
  remove   移出成员，并写入正在使用这个分组的 Agent
  delete   删除分组（需要 --yes）：只删分组记录，不删 Hub 里的 MCP
  enable   为 Agent 启用分组，与它已启用的其他分组叠加
  disable  为 Agent 停用分组，其他分组不受影响
  apply    应用修改：按最新成员重新写入正在使用这个分组的 Agent

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp groups list

列出 MCP 分组

用法：skill-studio mcp groups list [选项]

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp groups show

查看一个分组：成员、在哪些 Agent 上启用、需要检查的问题

用法：skill-studio mcp groups show [选项] <分组>

参数：
  <分组>  分组名称或 ID

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp groups create

新建分组

用法：skill-studio mcp groups create [选项] <名称>

参数：
  <名称>  分组名称

选项：
      --json       输出 JSON，给脚本和 Agent 解析
  -m, --mcp <MCP>  成员，逗号分隔
  -y, --yes        确认删除这类不可撤销的操作
  -h, --help       显示帮助

## skill-studio mcp groups rename

重命名分组

用法：skill-studio mcp groups rename [选项] <分组> <新名称>

参数：
  <分组>   分组名称或 ID
  <新名称>  新名称

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp groups add

加入成员，并写入正在使用这个分组的 Agent

用法：skill-studio mcp groups add [选项] <分组> <MCP>...

参数：
  <分组>      分组名称或 ID
  <MCP>...  要加入的 MCP

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp groups remove

移出成员，并写入正在使用这个分组的 Agent

用法：skill-studio mcp groups remove [选项] <分组> <MCP>...

参数：
  <分组>      分组名称或 ID
  <MCP>...  要移出的 MCP

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp groups delete

删除分组（需要 --yes）：只删分组记录，不删 Hub 里的 MCP

用法：skill-studio mcp groups delete [选项] <分组>

参数：
  <分组>  分组名称或 ID

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp groups enable

为 Agent 启用分组，与它已启用的其他分组叠加

用法：skill-studio mcp groups enable [选项] --agent <Agent> <分组>

参数：
  <分组>  分组名称或 ID

选项：
  -a, --agent <Agent>  Agent，可重复或用逗号分隔
      --json           输出 JSON，给脚本和 Agent 解析
  -y, --yes            确认删除这类不可撤销的操作
  -h, --help           显示帮助

## skill-studio mcp groups disable

为 Agent 停用分组，其他分组不受影响

用法：skill-studio mcp groups disable [选项] --agent <Agent> <分组>

参数：
  <分组>  分组名称或 ID

选项：
  -a, --agent <Agent>  Agent，可重复或用逗号分隔
      --json           输出 JSON，给脚本和 Agent 解析
  -y, --yes            确认删除这类不可撤销的操作
  -h, --help           显示帮助

## skill-studio mcp groups apply

应用修改：按最新成员重新写入正在使用这个分组的 Agent

用法：skill-studio mcp groups apply [选项] <分组>

参数：
  <分组>  分组名称或 ID

选项：
  -a, --agent <Agent>  只写入这些 Agent
      --json           输出 JSON，给脚本和 Agent 解析
  -y, --yes            确认删除这类不可撤销的操作
  -h, --help           显示帮助

## skill-studio mcp projects

项目里的 MCP：把 MCP 和 MCP 分组写进项目的配置，按 Agent 启用

用法：skill-studio mcp projects [选项] <命令>

命令：
  list     列出项目和它们的 MCP
  show     查看一个项目的 MCP：成员、写到哪里、在哪些 Agent 上启用
  add      加入成员（或改 Claude Code 的保存位置），并写入项目已启用的 Agent
  remove   移出成员，并写入项目已启用的 Agent
  enable   为 Agent 启用：把成员写进项目里这个 Agent 的 MCP 配置
  disable  为 Agent 停用：撤回写进项目的 MCP
  apply    应用修改：按最新成员重新写入项目已启用的 Agent

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp projects list

列出项目和它们的 MCP

用法：skill-studio mcp projects list [选项]

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp projects show

查看一个项目的 MCP：成员、写到哪里、在哪些 Agent 上启用

用法：skill-studio mcp projects show [选项] <项目>

参数：
  <项目>  项目名称、ID 或目录（「.」表示当前目录所在的项目）

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio mcp projects add

加入成员（或改 Claude Code 的保存位置），并写入项目已启用的 Agent

用法：skill-studio mcp projects add [选项] <项目>

参数：
  <项目>  项目名称、ID 或目录（「.」表示当前目录所在的项目）

选项：
      --json         输出 JSON，给脚本和 Agent 解析
  -m, --mcp <MCP>    要加入的 MCP，逗号分隔
  -g, --groups <分组>  要绑定的 MCP 分组，逗号分隔
  -y, --yes          确认删除这类不可撤销的操作
      --claude <位置>  Claude Code 写到哪里：project（项目里的 .mcp.json）或 local（只给自己用） [possible values: project, local]
  -h, --help         显示帮助

## skill-studio mcp projects remove

移出成员，并写入项目已启用的 Agent

用法：skill-studio mcp projects remove [选项] <项目>

参数：
  <项目>  项目名称、ID 或目录（「.」表示当前目录所在的项目）

选项：
      --json         输出 JSON，给脚本和 Agent 解析
  -m, --mcp <MCP>    要移出的 MCP，逗号分隔
  -g, --groups <分组>  要解绑的 MCP 分组，逗号分隔
  -y, --yes          确认删除这类不可撤销的操作
  -h, --help         显示帮助

## skill-studio mcp projects enable

为 Agent 启用：把成员写进项目里这个 Agent 的 MCP 配置

用法：skill-studio mcp projects enable [选项] --agent <Agent> <项目>

参数：
  <项目>  项目名称、ID 或目录（「.」表示当前目录所在的项目）

选项：
  -a, --agent <Agent>  Agent，可重复或用逗号分隔
      --json           输出 JSON，给脚本和 Agent 解析
  -y, --yes            确认删除这类不可撤销的操作
  -h, --help           显示帮助

## skill-studio mcp projects disable

为 Agent 停用：撤回写进项目的 MCP

用法：skill-studio mcp projects disable [选项] --agent <Agent> <项目>

参数：
  <项目>  项目名称、ID 或目录（「.」表示当前目录所在的项目）

选项：
  -a, --agent <Agent>  Agent，可重复或用逗号分隔
      --json           输出 JSON，给脚本和 Agent 解析
  -y, --yes            确认删除这类不可撤销的操作
  -h, --help           显示帮助

## skill-studio mcp projects apply

应用修改：按最新成员重新写入项目已启用的 Agent

用法：skill-studio mcp projects apply [选项] <项目>

参数：
  <项目>  项目名称、ID 或目录（「.」表示当前目录所在的项目）

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio doctor

自检：配置能否读取、有没有未完成的写入、Agent 目录、需要检查的问题

用法：skill-studio doctor [选项]

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助


