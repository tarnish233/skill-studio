import { useState, type ReactNode } from "react";
import { Play, RefreshCw, Square } from "lucide-react";
import {
  SummaryBar,
  SummaryEnd,
  SummaryFilter,
  agentOption,
} from "@/components/common/SummaryBar";
import { Button } from "@/components/ui/button";

/**
 * 分组页「按 Agent 查看」的选择，Skill / MCP 分开记住，离开页面再回来仍停在上次的视角。
 * 所选 Agent 被关闭管理后回到「全部」。
 */
export function useGroupAgentView(
  resource: "skills" | "mcp",
  available: readonly string[],
  page: "groups" | "projects" = "groups",
) {
  const key = `skill-studio-${page}-agent-${resource}`;
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
 * 至少有一个 Agent 在用的分组 id，Skill 与 MCP 分组页共用。以配置里的全部启用记录
 * 为准，不只看显示出来的 Agent。
 */
export function groupsInUse(
  activeGroups: Record<string, readonly { groupId: string }[]>,
): Set<string> {
  return new Set(
    Object.values(activeGroups)
      .flat()
      .map((g) => g.groupId),
  );
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
  noun = "分组",
  children,
}: {
  /** 卡片是什么：「分组」或「项目」 */
  noun?: string;
  total: number;
  agents: { id: string; displayName: string }[];
  /** 该 Agent 当前叠加了几个分组 */
  current: (agentId: string) => number;
  selected: string | null;
  onSelect: (agentId: string | null) => void;
  /** 右侧统计 */
  children: ReactNode;
}) {
  return (
    <SummaryBar>
      <SummaryFilter
        label="按 Agent 查看"
        total={total}
        allTitle={`每张${noun}卡片列出所有 Agent 的启停`}
        options={agents.map((agent) => {
          // 数字是这个 Agent 叠加的分组数
          const count = current(agent.id);
          return agentOption({
            id: agent.id,
            label: agent.displayName,
            count,
            title: count
              ? `${agent.displayName} 正在使用 ${count} 个${noun}`
              : `${agent.displayName} 没有启用${noun}`,
          });
        })}
        selected={selected}
        onSelect={onSelect}
      />
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
