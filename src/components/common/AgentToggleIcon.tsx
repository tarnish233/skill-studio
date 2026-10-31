import type { ReactNode } from "react";
import { RefreshCw } from "lucide-react";
import { AgentIcon } from "@/components/common/AgentIcon";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

/**
 * 分组卡片上一个 Agent 的状态图标（参考 cc-switch 3.x）：这个分组对该 Agent 是否生效。
 * 部署到 Agent 只通过分组，Hub 只展示有什么。
 */
export interface AgentToggleItem {
  agentId: string;
  /**
   * 可访问名称：有动作时描述点击后的结果（如「为 Codex 停用「Dev」」）；
   * 没有动作时描述状态。不加 aria-pressed —— WAI-ARIA APG 要求切换按钮的名称
   * 不随状态变化，名称本身会变的按钮不需要再声明 pressed。
   */
  label: string;
  /** 悬停提示 */
  hint: string;
  tone: "on" | "off";
  /** 需要人工关注：保留品牌色，仅用琥珀色提醒点标记 */
  attention?: boolean;
  /** 确认未安装时调暗，仍可点击 */
  dimmed?: boolean;
  /**
   * 点击要做的事。没有时按钮仍可聚焦、可看提示（aria-disabled），
   * 而不是 disabled —— 禁用的按钮收不到悬停，提示就没法解释为什么点不了。
   */
  onClick?: () => void;
}

export function AgentToggleIcon({
  item,
  busy,
}: {
  item: AgentToggleItem;
  /** 有操作进行中：整行真正禁用，避免同时发多个切换 */
  busy: boolean;
}) {
  const inert = !item.onClick;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={item.label}
          aria-disabled={inert || undefined}
          disabled={busy}
          onClick={item.onClick}
          className={cn(
            "relative flex h-7 w-7 shrink-0 items-center justify-center rounded-lg border border-transparent bg-transparent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60",
            busy ? "disabled:cursor-wait" : inert && "cursor-not-allowed",
            !inert && "hover:bg-muted",
            item.tone === "on" ? "text-foreground" : "text-muted-foreground",
          )}
        >
          {/* Only enabled Agents are lit. Hover never turns an inactive logo into brand colors. */}
          <AgentIcon
            agentId={item.agentId}
            size="sm"
            className={cn(
              item.tone === "off" && "grayscale",
              item.tone === "off" &&
                (item.dimmed ? "opacity-25" : "opacity-40"),
            )}
          />
          {item.attention && (
            <span
              aria-hidden="true"
              className="absolute -right-0.5 -top-0.5 h-2 w-2 rounded-full bg-amber-500 ring-2 ring-card"
            />
          )}
        </button>
      </TooltipTrigger>
      <TooltipContent>{item.hint}</TooltipContent>
    </Tooltip>
  );
}

/** 一排 Agent 图标；`leading` 放在图标前，如「部署到」标签或重新应用按钮 */
export function AgentToggleRow({
  items,
  busy,
  leading,
}: {
  items: AgentToggleItem[];
  busy: boolean;
  leading?: ReactNode;
}) {
  return (
    <div className="flex shrink-0 items-center gap-1">
      {leading}
      {items.map((item) => (
        <AgentToggleIcon key={item.agentId} item={item} busy={busy} />
      ))}
    </div>
  );
}

/** 图标行前的琥珀色操作按钮：分组的「应用修改 / 重新应用」 */
export function AgentRowAction({
  label,
  hint,
  busy,
  disabled,
  onClick,
}: {
  label: string;
  hint: string;
  busy: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={label}
          disabled={busy || disabled}
          onClick={onClick}
          className={cn(
            "mr-1 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-amber-600 transition-colors hover:bg-amber-500/10 disabled:opacity-60 dark:text-amber-400",
            busy ? "disabled:cursor-wait" : "disabled:cursor-not-allowed",
          )}
        >
          <RefreshCw className="h-4 w-4" />
        </button>
      </TooltipTrigger>
      <TooltipContent>{hint}</TooltipContent>
    </Tooltip>
  );
}
