import { useState, type ReactNode } from "react";
import { Play, RefreshCw, Square } from "lucide-react";
import { AgentIcon } from "@/components/common/AgentIcon";
import {
  SummaryBar,
  SummaryEnd,
  SummaryStart,
  summaryPillClass,
} from "@/components/common/SummaryBar";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/**
 * 分组页「按 Agent 查看」的选择，Skill / MCP 分开记住，离开页面再回来仍停在上次的视角。
 * 所选 Agent 被关闭管理后回到「全部」。
 */
export function useGroupAgentView(
  resource: "skills" | "mcp",
  available: readonly string[],
) {
  const key = `skill-studio-groups-agent-${resource}`;
  const [picked, setPicked] = useState<string | null>(() =>
    localStorage.getItem(key),
  );
  const select = (id: string | null) => {
    setPicked(id);
    if (id) localStorage.setItem(key, id);
    else localStorage.removeItem(key);
  };
  return [
    picked && available.includes(picked) ? picked : null,
    select,
  ] as const;
}

/**
 * 分组页顶部：「全部」与每个 Agent 的视角胶囊（胶囊上带着它当前用的分组），右侧是当前视角的统计。
 * 「全部」是每张卡片一排 Agent 图标；选中某个 Agent 后，卡片只保留这个 Agent 的启停按钮。
 */
export function GroupAgentFilter({
  total,
  agents,
  current,
  selected,
  onSelect,
  children,
}: {
  total: number;
  agents: { id: string; displayName: string }[];
  /** 该 Agent 当前叠加了几个分组 */
  current: (agentId: string) => number;
  selected: string | null;
  onSelect: (agentId: string | null) => void;
  /** 右侧统计 */
  children: ReactNode;
}) {
  const pill = (pressed: boolean) =>
    cn(
      summaryPillClass,
      "border-border-default transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
      pressed
        ? "bg-muted text-foreground ring-1 ring-current"
        : "text-muted-foreground",
    );
  return (
    <SummaryBar>
      <SummaryStart role="group" aria-label="按 Agent 查看">
        <button
          type="button"
          aria-pressed={!selected}
          onClick={() => onSelect(null)}
          className={pill(!selected)}
        >
          全部 {total}
        </button>
        {agents.map((agent) => {
          const count = current(agent.id);
          return (
            <button
              key={agent.id}
              type="button"
              aria-pressed={selected === agent.id}
              title={
                count
                  ? `${agent.displayName} 正在使用 ${count} 个分组`
                  : `${agent.displayName} 没有启用分组`
              }
              onClick={() => onSelect(agent.id)}
              className={pill(selected === agent.id)}
            >
              <AgentIcon agentId={agent.id} size="sm" />
              {agent.displayName}
              {/* 前导空格让读屏读作「Codex · 2 个分组」；视觉上行首空格会被折叠掉 */}
              {count > 0 && (
                <span className="opacity-70">{` · ${count} 个分组`}</span>
              )}
            </button>
          );
        })}
      </SummaryStart>
      <SummaryEnd>{children}</SummaryEnd>
    </SummaryBar>
  );
}

/**
 * 单个 Agent 视角下分组卡片右侧的按钮：把这张卡片的分组加进 / 移出这个 Agent 的叠加组合。
 * 有待应用修改时旁边多一个「应用修改」/「重新应用」。
 */
export function AgentGroupAction({
  agent,
  groupName,
  active,
  blocked,
  reapply,
  busy,
  onActivate,
  onStop,
  onReapply,
}: {
  /** Agent 展示名 */
  agent: string;
  groupName: string;
  active: boolean;
  /** 不能启用的原因；停用始终可以 */
  blocked?: string;
  /** 「应用修改」或「重新应用」 */
  reapply?: { label: string; disabled?: boolean };
  busy: boolean;
  onActivate: () => void;
  onStop: () => void;
  onReapply: () => void;
}) {
  if (active)
    return (
      <div className="flex shrink-0 items-center gap-2">
        {reapply && (
          <Button
            size="sm"
            variant="outline"
            aria-label={`为 ${agent} ${reapply.label}「${groupName}」`}
            disabled={busy || reapply.disabled}
            onClick={onReapply}
          >
            <RefreshCw className="h-4 w-4" />
            {reapply.label}
          </Button>
        )}
        <Button
          size="sm"
          variant="outline"
          aria-label={`为 ${agent} 停用「${groupName}」`}
          disabled={busy}
          onClick={onStop}
        >
          <Square className="h-4 w-4" />
          停用
        </Button>
      </div>
    );
  return (
    <div className="flex shrink-0 items-center gap-2">
      {blocked && (
        <span className="text-xs text-muted-foreground">{blocked}</span>
      )}
      <Button
        size="sm"
        aria-label={`为 ${agent} 启用「${groupName}」`}
        disabled={busy || !!blocked}
        onClick={onActivate}
      >
        <Play className="h-4 w-4" />
        启用
      </Button>
    </div>
  );
}
