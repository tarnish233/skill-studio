import {
  AgentRowAction,
  AgentToggleRow,
} from "@/components/common/AgentToggleIcon";

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
  /** 不能启用的原因（空分组、成员缺失、已关闭管理）；已启用时忽略 —— 停用始终可以 */
  blocked?: string;
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
 * 分组卡片上的 Agent 开关行：每个 Agent 一个品牌图标，点击只在启用 / 停用之间切换，
 * 各 Agent 互不影响。重新应用是单独的按钮：有待应用修改时点图标照样是停用，后端拒绝
 * 重新应用（比如新成员在某个 Agent 上不可用）也不会让分组关不掉。
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
    <AgentToggleRow
      busy={busy}
      leading={
        reapply && (
          <AgentRowAction
            label={`${reapply.label}「${groupName}」`}
            hint={`${reapply.label}：${reapply.targets.join("、")}`}
            busy={busy}
            disabled={reapply.disabled}
            onClick={reapply.onClick}
          />
        )
      }
      items={agents.map((agent) => {
        const action = agent.active ? "停用" : "启用";
        const name = `${agent.label}${agent.note ? `（${agent.note}）` : ""}`;
        const blocked = agent.active ? undefined : agent.blocked;
        return {
          agentId: agent.agentId,
          label: `为 ${agent.label} ${action}「${groupName}」${agent.note ? `（${agent.note}）` : ""}`,
          hint: blocked
            ? `${name}：${blocked}`
            : `${name}：${agent.attention ?? (agent.active ? "已启用" : "未启用")}，点击${action}`,
          tone: agent.active ? "on" : "off",
          attention: !!agent.attention,
          dimmed: agent.dimmed,
          onClick: blocked
            ? undefined
            : () => onToggle(agent.agentId, !agent.active),
        };
      })}
    />
  );
}
