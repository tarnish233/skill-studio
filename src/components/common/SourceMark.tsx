import { Bot } from "lucide-react";
import { AgentIcon } from "@/components/common/AgentIcon";
import { SkillStudioIcon } from "@/components/common/SkillStudioIcon";
import { Badge } from "@/components/ui/badge";

/** 不是 Agent 的来源：Skill Studio、共享 Agent 目录、无法确认的来源 */
export const SOURCE_LABELS: Record<string, string> = {
  studio: "Skill Studio",
  builtin: "Skill Studio 内置",
  agent: "Agent",
  unknown: "来源待确认",
  external: "外部来源",
};
/** 筛选胶囊的悬停提示：先报名称再解释（有图标的胶囊只显示图标） */
export const SOURCE_TITLES = {
  studio: "Skill Studio：通过 Skill Studio 安装的 skill",
  builtin:
    "Skill Studio 内置：随应用提供、说明怎么用命令行管理 Skill 与 MCP；跟着应用更新，自己改过就保留",
  agent:
    "Agent：共享 Agent 目录（~/.agents/skills）里的 skill。Codex、OpenCode、Pi、Grok Build 都会直接加载这里的 skill，分组管不住；Skill Studio 不会往里部署，纳入托管会把它移出共享目录。",
  unknown: "来源待确认：缺少托管记录，无法确认原始来源",
  external: "外部来源：不在任何 Agent 的 skill 目录里",
} as const;

/**
 * 来源图标，筛选胶囊和卡片上的来源标记共用。只给一眼认得出的：Agent 的品牌
 * 图标、Skill Studio，以及共享 Agent 目录的小机器人；待确认、外部来源没有，
 * 直接写名称。
 */
export function sourceIcon(id: string) {
  if (id === "studio") return <SkillStudioIcon className="h-4 w-4" />;
  if (id === "agent")
    return <Bot className="h-4 w-4 text-violet-500 dark:text-violet-400" />;
  if (id in SOURCE_LABELS) return undefined;
  return <AgentIcon agentId={id} size="sm" />;
}

/**
 * 卡片上的一枚来源标记：认得出的用图标（名称放进可访问名称和悬停提示），
 * 其余写出名称。Hub 卡片和分组编辑弹窗的成员行共用。
 */
export function SourceMark({
  id,
  label,
  title,
}: {
  id: string;
  /** 来源名称：Agent 的展示名或 `SOURCE_LABELS` 里的名称 */
  label: string;
  title?: string;
}) {
  const icon = sourceIcon(id);
  return icon ? (
    <span role="img" aria-label={`来源：${label}`} title={title ?? label}>
      {icon}
    </span>
  ) : (
    <Badge variant="outline" title={title ?? label}>
      <span className="sr-only">来源：</span>
      {label}
    </Badge>
  );
}
