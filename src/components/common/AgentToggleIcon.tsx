import { AgentIcon } from "@/components/common/AgentIcon";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

/**
 * 分组卡片上单个 Agent 的开关图标：点击在启用 / 停用 / 重新应用之间切换，
 * 状态靠颜色（未启用灰、启用绿、需要处理琥珀）和右上角提醒点区分，不需要
 * 分组归属某个 Agent —— 同一张卡片上每个 Agent 的状态互相独立。
 *
 * Skill 分组与 MCP 分组共用这一个组件：两边"是否生效""是否需要重新应用"的
 * 判定细节不同，但交互和视觉状态是同一套，由调用方算好 `active`/`attention`
 * 两个布尔值传入。
 */
export function AgentToggleIcon({
  agentId,
  label,
  active,
  attention,
  reapply = attention,
  disabled,
  onClick,
}: {
  agentId: string;
  /** Agent 展示名，用于 aria-label 和提示文案 */
  label: string;
  active: boolean;
  /** 已启用但需要人工关注（成员变化、副本漂移、缺失或外部修改）——只影响颜色和提醒点，不决定点击行为 */
  attention: boolean;
  /**
   * 点击是重新应用还是单纯停用。默认等于 attention，
   * 但调用方可以把它收窄到只在“安全重同步”的子集（比如成员列表变化）：
   * 某个成员需要人工处理时，点击仍然可以安全停用，不让一个有问题的分组连关都关不掉。
   */
  reapply?: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  const action = !active ? "启用" : reapply ? "重新应用" : "停用";
  const hint = !active
    ? "：未启用，点击启用"
    : reapply
      ? "：有待应用修改，点击重新应用"
      : attention
        ? "：需要检查，点击停用"
        : "：已启用，点击停用";
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={`为 ${label} ${action}`}
          aria-pressed={active}
          disabled={disabled}
          onClick={onClick}
          className={cn(
            "relative flex h-7 w-7 shrink-0 items-center justify-center rounded-lg border transition-colors disabled:cursor-wait disabled:opacity-60",
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
        {hint}
      </TooltipContent>
    </Tooltip>
  );
}
