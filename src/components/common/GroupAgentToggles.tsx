import { RefreshCw } from "lucide-react";
import { AgentIcon } from "@/components/common/AgentIcon";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

/** 一张分组卡片上某个 Agent 的状态，由调用方按 Skill / MCP 各自的口径算好 */
export interface AgentToggleState {
  /** Skill 侧的 Agent id：决定品牌图标，也原样传回 `onToggle` */
  agentId: string;
  label: string;
  active: boolean;
  /** 已启用但需要关注的原因（如「有待应用修改」「需要检查」）：图标转琥珀色并带提醒点 */
  attention?: string;
  /** 附在可访问名称和提示里的补充说明，如「未检测到安装」「已关闭管理」 */
  note?: string;
  /** 确认未安装时调暗，仍可点击 */
  dimmed?: boolean;
  /** 能否启用（空分组、成员缺失、已关闭管理时不能）；停用始终可以 */
  canActivate: boolean;
}

/** 为卡片上需要重新应用的 Agent 依次重新应用分组 */
export interface ReapplyAction {
  /** 「应用修改」或「重新应用」 */
  label: string;
  /** 将被重新应用的 Agent 展示名 */
  targets: string[];
  disabled?: boolean;
  onClick: () => void;
}

/**
 * 分组卡片上的 Agent 开关行（参考 cc-switch 3.x）：每个 Agent 一个品牌图标，点击只在
 * 启用 / 停用之间切换，各 Agent 互不影响。重新应用是单独的按钮：有待应用修改时点图标
 * 照样是停用，后端拒绝重新应用（比如新成员在某个 Agent 上不可用）也不会让分组关不掉。
 *
 * Skill 分组与 MCP 分组共用这一行：两边的状态口径不同，由调用方算好传入。
 */
export function GroupAgentToggles({
  groupName,
  agents,
  busy,
  onToggle,
  reapply,
}: {
  groupName: string;
  agents: AgentToggleState[];
  /** 任一启停或重新应用进行中：整行禁用，不同时发多个切换 */
  busy: boolean;
  onToggle: (agentId: string, activate: boolean) => void;
  reapply?: ReapplyAction;
}) {
  return (
    <div className="flex shrink-0 items-center gap-1">
      {reapply && (
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              aria-label={`${reapply.label}「${groupName}」`}
              disabled={busy || reapply.disabled}
              onClick={reapply.onClick}
              className={cn(
                "mr-1 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-amber-600 transition-colors hover:bg-amber-500/10 disabled:opacity-60 dark:text-amber-400",
                busy ? "disabled:cursor-wait" : "disabled:cursor-not-allowed",
              )}
            >
              <RefreshCw className="h-4 w-4" />
            </button>
          </TooltipTrigger>
          <TooltipContent>
            {reapply.label}：{reapply.targets.join("、")}
          </TooltipContent>
        </Tooltip>
      )}
      {agents.map((agent) => (
        <AgentToggleIcon
          key={agent.agentId}
          {...agent}
          groupName={groupName}
          busy={busy}
          onClick={() => onToggle(agent.agentId, !agent.active)}
        />
      ))}
    </div>
  );
}

function AgentToggleIcon({
  agentId,
  label,
  groupName,
  active,
  attention,
  note,
  dimmed,
  canActivate,
  busy,
  onClick,
}: AgentToggleState & {
  groupName: string;
  busy: boolean;
  onClick: () => void;
}) {
  const action = active ? "停用" : "启用";
  const suffix = note ? `（${note}）` : "";
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        {/*
          名称描述的是点击后的动作，所以不加 aria-pressed：WAI-ARIA APG 要求切换按钮的
          名称不随状态变化，名称本身会变的按钮不需要再声明 pressed。
        */}
        <button
          type="button"
          aria-label={`为 ${label} ${action}「${groupName}」${suffix}`}
          disabled={busy || (!active && !canActivate)}
          onClick={onClick}
          className={cn(
            "relative flex h-7 w-7 shrink-0 items-center justify-center rounded-lg border transition-colors",
            busy ? "disabled:cursor-wait" : "disabled:cursor-not-allowed",
            dimmed ? "opacity-40" : "disabled:opacity-60",
            active
              ? attention
                ? "border-amber-500/50 bg-amber-500/10 text-amber-700 dark:text-amber-400"
                : "border-emerald-500/50 bg-emerald-500/10 text-emerald-700 dark:text-emerald-400"
              : "border-transparent text-muted-foreground/60 hover:border-border-default hover:bg-muted hover:text-foreground",
          )}
        >
          <AgentIcon agentId={agentId} size="sm" />
          {attention && (
            <span
              aria-hidden="true"
              className="absolute -right-0.5 -top-0.5 h-2 w-2 rounded-full bg-amber-500 ring-2 ring-card"
            />
          )}
        </button>
      </TooltipTrigger>
      <TooltipContent>
        {label}
        {suffix}：{attention ?? (active ? "已启用" : "未启用")}，点击{action}
      </TooltipContent>
    </Tooltip>
  );
}
