# 命令行参考

由 `skill-studio docs` 生成，与当前版本一致；不要手改。

## skill-studio

Skill Studio 命令行：查看和管理 Skill 与 MCP 的 Hub、分组与项目，按 Agent 启用。
与桌面应用共用同一份配置，改动会实时出现在 Skill Studio 里。

用法：skill-studio [选项] <命令>

命令：
  status    总览：Agent、分组与各 Agent 的启用、项目、需要检查和有待应用的修改
  agents    列出 Agent：是否安装、是否在管理中、skill 目录
  skills    Skill Hub：查看、搜索、安装、导入、托管与移出、文件删除
  groups    分组：一组 skill，可为每个 Agent 单独启用，多个分组可以叠加
  projects  项目：把 skill 和分组写进项目目录，按 Agent 启用
  mcp       MCP：Hub 里的 MCP、MCP 分组与项目里的 MCP
  settings  设置：查看与修改管理策略、目录和备份保留份数
  backups   备份：配置、删除的 skill、托管完整快照与 MCP 配置
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

Skill Hub：查看、搜索、安装、导入、托管与移出、文件删除

用法：skill-studio skills [选项] <命令>

命令：
  list      列出 skill
  show      查看一个 skill：来源、所在分组与项目、在各 Agent 上的状态
  search    在 skills.sh 上搜索
  install   从 GitHub 安装到 Skill Hub；只放进 Hub，不会启用到任何 Agent
  adopt     托管已有原件到 Hub 并在 Agent 私有目录留入口（需要 --yes）；多来源须指定路径或 ID
  release   移出 Hub 并还原原件（需要 --yes）
  delete    删除具体来源的文件（需要 --yes）；非软链接保留备份，内置和外部来源不可删
  prune     清理已不存在 skill 留下的分组与注册引用（需要 --yes）
  discover  扫描本地目录，列出候选与错误，不导入
  import    把本地的 skill 目录导入 Skill Hub

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

## skill-studio skills adopt

托管已有原件到 Hub 并在 Agent 私有目录留入口（需要 --yes）；多来源须指定路径或 ID

用法：skill-studio skills adopt [选项] <skill名称、ID或目录>

参数：
  <skill名称、ID或目录>

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio skills release

移出 Hub 并还原原件（需要 --yes）

用法：skill-studio skills release [选项] <skill名称、ID或目录>

参数：
  <skill名称、ID或目录>

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio skills delete

删除具体来源的文件（需要 --yes）；非软链接保留备份，内置和外部来源不可删

用法：skill-studio skills delete [选项] <skill名称、ID或目录>

参数：
  <skill名称、ID或目录>

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio skills prune

清理已不存在 skill 留下的分组与注册引用（需要 --yes）

用法：skill-studio skills prune [选项]

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio skills discover

扫描本地目录，列出候选与错误，不导入

用法：skill-studio skills discover [选项] <目录>

参数：
  <目录>

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

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
  skills     项目已有 skill：列出、单项启停、收录与删除
  set        修改项目写入方式，并应用到已启用的 Agent
  gitignore  把 Skill Studio 的本机溯源/事务文件写进项目 .gitignore
  list       列出项目
  show       查看一个项目：成员、在哪些 Agent 上启用、需要检查的问题
  create     添加项目
  add        加入成员，并写入项目已启用的 Agent
  remove     移出成员，并写入项目已启用的 Agent
  enable     为 Agent 启用项目：把成员写进项目里这个 Agent 的 skill 目录
  disable    为 Agent 停用项目：撤回写进项目目录的 skill
  apply      应用修改：按最新成员重新写入项目已启用的 Agent
  delete     移除项目（需要 --yes）：只删记录，项目目录里的文件不动

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio projects skills

项目已有 skill：列出、单项启停、收录与删除

用法：skill-studio projects skills [选项] <命令>

命令：
  list     列出项目里的原件、部署副本与停用记录
  adopt    收录一个项目原件到 Hub，保留项目文件（与全局 skills adopt 的搬移不同）
  enable   恢复项目里被 Studio 暂存停用的原件
  disable  暂存停用一个项目原件，不影响其他成员或 Agent
  delete   删除一个项目原件（需要 --yes）；非软链接保留可恢复备份

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio projects skills list

列出项目里的原件、部署副本与停用记录

用法：skill-studio projects skills list [选项] <项目>

参数：
  <项目>

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio projects skills adopt

收录一个项目原件到 Hub，保留项目文件（与全局 skills adopt 的搬移不同）

用法：skill-studio projects skills adopt [选项] <PROJECT> <skill名称或路径>

参数：
  <PROJECT>
  <skill名称或路径>

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio projects skills enable

恢复项目里被 Studio 暂存停用的原件

用法：skill-studio projects skills enable [选项] <PROJECT> <skill名称或路径>

参数：
  <PROJECT>
  <skill名称或路径>

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio projects skills disable

暂存停用一个项目原件，不影响其他成员或 Agent

用法：skill-studio projects skills disable [选项] <PROJECT> <skill名称或路径>

参数：
  <PROJECT>
  <skill名称或路径>

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio projects skills delete

删除一个项目原件（需要 --yes）；非软链接保留可恢复备份

用法：skill-studio projects skills delete [选项] <PROJECT> <skill名称或路径>

参数：
  <PROJECT>
  <skill名称或路径>

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio projects set

修改项目写入方式，并应用到已启用的 Agent

用法：skill-studio projects set [选项] --link-mode <LINK_MODE> <项目>

参数：
  <项目>

选项：
      --json                   输出 JSON，给脚本和 Agent 解析
      --link-mode <LINK_MODE>  [possible values: auto, symlink, copy]
  -y, --yes                    确认删除这类不可撤销的操作
  -h, --help                   显示帮助

## skill-studio projects gitignore

把 Skill Studio 的本机溯源/事务文件写进项目 .gitignore

用法：skill-studio projects gitignore [选项] <项目>

参数：
  <项目>

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
  search    搜索官方 MCP Registry；只查询，不安装，支持分页
  test      一次性测试握手与工具列表，不改配置；不代表 Agent 已重新加载或完成登录
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

## skill-studio mcp search

搜索官方 MCP Registry；只查询，不安装，支持分页

用法：skill-studio mcp search [选项] <搜索词>

参数：
  <搜索词>

选项：
      --cursor <CURSOR>
      --json             输出 JSON，给脚本和 Agent 解析
  -y, --yes              确认删除这类不可撤销的操作
  -h, --help             显示帮助

## skill-studio mcp test

一次性测试握手与工具列表，不改配置；不代表 Agent 已重新加载或完成登录

用法：skill-studio mcp test [选项] <MCP>

参数：
  <MCP>

选项：
      --json               输出 JSON，给脚本和 Agent 解析
      --timeout <TIMEOUT>  [default: 30]
  -y, --yes                确认删除这类不可撤销的操作
  -h, --help               显示帮助

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
      --config <CONFIG>        替换完整连接定义（JSON、TOML 或安装命令）；密钥不写入输出/日志
      --json                   输出 JSON，给脚本和 Agent 解析
      --transport <TRANSPORT>  服务类型；切换时清除原类型的专属连接字段 [possible values: stdio, http, sse]
  -y, --yes                    确认删除这类不可撤销的操作
  -h, --help                   显示帮助
      --mode <MODE>            修改直连/网关模式；网关需安装桌面应用，不自动启动或授权 [possible values: direct, gateway]
      --cwd <CWD>              本地 MCP 工作目录
      --clear-cwd              清除工作目录
      --client-id <CLIENT_ID>  OAuth Client ID（网关 HTTP 服务）
      --clear-client-id        清除 OAuth Client ID
      --scopes <SCOPES>        OAuth scopes，逗号分隔
      --clear-scopes           清除 OAuth scopes
      --oauth <OAUTH>          明确要求 OAuth true/false；未设置时保留自动检查授权需求 [possible values: true, false]
      --name <新名称>             改名
  -e, --env <KEY=值>            设置环境变量 KEY=值，可重复
      --unset-env <KEY>        删掉环境变量，可重复
  -H, --header <请求头>           设置请求头，写成 "名称: 值" 或 名称=值，可重复
      --unset-header <名称>      删掉请求头，可重复
      --url <网址>               换网址（远程 MCP）


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

## skill-studio settings

设置：查看与修改管理策略、目录和备份保留份数

用法：skill-studio settings [选项] <命令>

命令：
  show  查看完整设置（只读）
  set   修改设置；管理开关与手动 skill 策略必须单独保存

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio settings show

查看完整设置（只读）

用法：skill-studio settings show [选项]

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio settings set

修改设置；管理开关与手动 skill 策略必须单独保存

用法：skill-studio settings set [选项]

选项：
      --json
          输出 JSON，给脚本和 Agent 解析
      --manage-agent <MANAGE_AGENT>
          开启这个 Agent 的管理；每次只切换一个，不自动启用分组
      --unmanage-agent <UNMANAGE_AGENT>
          退出这个 Agent 的管理；每次只切换一个（需要 --yes）
  -y, --yes
          确认删除这类不可撤销的操作
  -h, --help
          显示帮助
      --manage-mcp <MANAGE_MCP>
          MCP 管理总开关 true/false（需要 --yes） [possible values: true, false]
      --preserve-manual-skills <PRESERVE_MANUAL_SKILLS>
          是否保留手动 skill true/false（需要 --yes） [possible values: true, false]
      --cli-enabled <CLI_ENABLED>
          CLI 与内置 skill 总开关（关闭需要 --yes；关闭后须在桌面重新开启） [possible values: true, false]
      --default-link-mode <DEFAULT_LINK_MODE>
          默认安装链接方式 [possible values: auto, symlink, copy]
      --agent-dir <AGENT_DIR>
          Agent 配置目录，写成 Agent=目录，可重复
      --clear-agent-dir <CLEAR_AGENT_DIR>
          取消这个 Agent 的目录覆盖，可重复
      --hub-dir <HUB_DIR>
          Hub 目录；不会迁移文件，非空 Hub 不能切换
      --clear-hub-dir
          恢复默认 Hub 目录
      --backup-keep <BACKUP_KEEP>
          配置备份保留份数 0–100；调小会清理备份，需要 --yes
      --show-codex-builtin-mcp <SHOW_CODEX_BUILTIN_MCP>
          显示 Codex App 内置 MCP true/false（仍只读） [possible values: true, false]
      --theme <THEME>
          桌面主题 [possible values: light, dark, system]
      --language <LANGUAGE>
          界面语言

## skill-studio backups

备份：配置、删除的 skill、托管完整快照与 MCP 配置

用法：skill-studio backups [选项] <命令>

命令：
  list     列出备份；不指定种类时分别列出全部，MCP 管理关闭时跳过 MCP
  restore  从备份恢复（需要 --yes）；hub 完整备份由移出 Hub 流程还原
  delete   删除备份（需要 --yes）；仍在使用的 hub 完整备份不能删

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio backups list

列出备份；不指定种类时分别列出全部，MCP 管理关闭时跳过 MCP

用法：skill-studio backups list [选项] [种类]

参数：
  [种类]  [possible values: config, skill, hub, mcp]

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio backups restore

从备份恢复（需要 --yes）；hub 完整备份由移出 Hub 流程还原

用法：skill-studio backups restore [选项] <种类> <备份ID>

参数：
  <种类>    [possible values: config, skill, hub, mcp]
  <备份ID>

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助

## skill-studio backups delete

删除备份（需要 --yes）；仍在使用的 hub 完整备份不能删

用法：skill-studio backups delete [选项] <种类> [备份ID]...

参数：
  <种类>       [possible values: config, skill, hub, mcp]
  [备份ID]...

选项：
      --all           删除这个种类的全部备份
      --json          输出 JSON，给脚本和 Agent 解析
      --unused        只删除未被引用的 hub 完整备份
  -y, --yes           确认删除这类不可撤销的操作
  -h, --help          显示帮助
      --unrestorable  只删除已无法恢复的 MCP 备份
      --legacy        清理旧版本写在 Agent 配置旁边的 MCP 备份

## skill-studio doctor

自检：配置能否读取、有没有未完成的写入、Agent 目录、需要检查的问题

用法：skill-studio doctor [选项]

选项：
      --json  输出 JSON，给脚本和 Agent 解析
  -y, --yes   确认删除这类不可撤销的操作
  -h, --help  显示帮助


