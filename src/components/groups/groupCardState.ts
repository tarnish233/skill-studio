import type { AgentToggleState } from "@/components/common/GroupAgentToggles";

/** 一个分组对某个 Agent 的状态，由 Skill / MCP 各自按自己的口径算好 */
export interface AgentGroupStatus {
  /** Skill 侧的 Agent id（决定品牌图标）与展示名 */
  agentId: string;
  label: string;
  /** 这个 Agent 仍在 Studio 管理中；关闭管理但仍在用这个分组时为 false，只能停用 */
  managed: boolean;
  /** 未检测到安装等补充说明 */
  note?: string;
  /** 确认未安装时调暗 */
  dimmed?: boolean;
  active: boolean;
  /** 启用后分组又改过（成员、定义），与「有待应用修改」同义 */
  changed: boolean;
  /** 部署出了问题，需要人工处理：写明原因，与「需要检查」同义 */
  issue?: string;
  /** 写到哪里，只出现在图标提示里 */
  where?: string;
}

/** 卡片上一枚状态徽标 */
export interface GroupBadge {
  label: "使用中" | "需要检查" | "有待应用修改" | "成员已缺失";
  tone: "success" | "warning";
  title?: string;
}

export interface GroupCardState {
  /** 至少一个 Agent 在用（不论是否还在管理中）：卡片高亮、不能删除 */
  used: boolean;
  /** 当前视角下是否高亮：「全部」看有没有 Agent 在用，单个 Agent 只看它自己 */
  highlighted: boolean;
  badges: GroupBadge[];
  /** 不能启用的原因（已关闭管理另算）；停用始终可以 */
  blocked?: string;
  toggles: AgentToggleState[];
  /** 「全部」视角卡片上的「应用修改 / 重新应用」 */
  reapply?: {
    label: string;
    agentIds: string[];
    targets: string[];
    disabled: boolean;
  };
  /** 单个 Agent 视角 */
  view?: {
    active: boolean;
    reapply?: { label: string; disabled: boolean };
  };
}

/** 卡片提示里的说法：分组页和项目页各一套 */
export interface CardLabels {
  /** 「分组」或「项目」 */
  subject: string;
  /** 没有成员时不能启用的原因 */
  empty: string;
  /** 「有待应用修改」悬停提示的开头，后面接 Agent 名 */
  changed: string;
}
export const GROUP_CARD_LABELS: CardLabels = {
  subject: "分组",
  empty: "分组还没有成员",
  changed: "分组在启用后改过，还没应用到：",
};

const reapplyLabel = (changed: boolean) => (changed ? "应用修改" : "重新应用");
const describe = (statuses: AgentGroupStatus[]) =>
  statuses.map((s) => s.label).join("、");

/**
 * Skill 与 MCP 分组卡片共用的状态换算：徽标、高亮、Agent 图标、重新应用。
 * 两边只需要算好每个 Agent 的 `AgentGroupStatus`，以及分组本身是否为空、是否有成员已缺失。
 */
export function groupCardState({
  statuses,
  used,
  memberCount,
  missingMembers,
  viewAgentId,
  labels = GROUP_CARD_LABELS,
}: {
  statuses: AgentGroupStatus[];
  /** 以配置里的全部启用记录为准（见 `groupsInUse`），不只看显示出来的 Agent */
  used: boolean;
  memberCount: number;
  missingMembers: boolean;
  /** 单个 Agent 视角时选中的 Agent */
  viewAgentId?: string;
  labels?: CardLabels;
}): GroupCardState {
  const blocked = missingMembers
    ? `成员已缺失，请先编辑${labels.subject}`
    : memberCount
      ? undefined
      : labels.empty;
  const active = statuses.filter((s) => s.active);
  const view = viewAgentId
    ? statuses.find((s) => s.agentId === viewAgentId)
    : undefined;
  // 徽标只说当前视角：「全部」汇总在用的 Agent，单个 Agent 只看它自己
  const shown = viewAgentId ? (view?.active ? [view] : []) : active;
  const highlighted = viewAgentId ? !!view?.active : used;
  const issues = shown.filter((s) => s.issue);
  const changed = shown.filter((s) => s.changed);
  const badges: GroupBadge[] = [];
  if (highlighted)
    badges.push(
      issues.length
        ? {
            label: "需要检查",
            tone: "warning",
            title: issues.map((s) => `${s.label}：${s.issue}`).join("\n"),
          }
        : { label: "使用中", tone: "success", title: describe(shown) },
    );
  if (changed.length)
    badges.push({
      label: "有待应用修改",
      tone: "warning",
      title: `${labels.changed}${describe(changed)}`,
    });
  if (missingMembers)
    badges.push({
      label: "成员已缺失",
      tone: "warning",
      title: `有成员已不在 Hub 里，请编辑${labels.subject}移除或重新添加`,
    });
  const targets = active.filter((s) => s.managed && (s.changed || s.issue));
  return {
    used,
    highlighted,
    badges,
    blocked,
    toggles: statuses.map((s) => ({
      agentId: s.agentId,
      label: s.label,
      active: s.active,
      attention: s.changed
        ? "有待应用修改"
        : s.issue
          ? `需要检查（${s.issue}）`
          : s.active && missingMembers
            ? "成员已缺失"
            : undefined,
      note: s.managed ? s.note : "已关闭管理",
      dimmed: s.managed && s.dimmed,
      blocked: s.managed ? blocked : "已关闭管理",
      where: s.where,
    })),
    reapply: targets.length
      ? {
          label: reapplyLabel(targets.some((t) => t.changed)),
          agentIds: targets.map((t) => t.agentId),
          targets: targets.map((t) => t.label),
          // 成员缺失或为空时重新应用必然失败，先编辑分组
          disabled: !!blocked,
        }
      : undefined,
    view: view && {
      active: view.active,
      reapply:
        view.active && (view.changed || view.issue)
          ? { label: reapplyLabel(view.changed), disabled: !!blocked }
          : undefined,
    },
  };
}
