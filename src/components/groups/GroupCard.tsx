import type { ReactNode } from "react";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GripVertical, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ListItemRow, RowActions } from "@/components/common/ListItemRow";
import { NameAvatar } from "@/components/common/NameAvatar";
import { GroupAgentToggles } from "@/components/common/GroupAgentToggles";
import { AgentGroupAction } from "@/components/common/GroupAgentView";
import { cn } from "@/lib/utils";
import type { GroupCardState } from "./groupCardState";

const GROUP_REMOVAL = {
  verb: "删除",
  title: "删除分组",
  blocked: "请先停用分组",
};

/** 有 Agent 在用的分组：绿色描边与渐变，悬停加深 */
const HIGHLIGHT =
  "border-emerald-500/40 bg-card bg-gradient-to-r from-emerald-500/10 to-transparent shadow-sm shadow-emerald-500/5 hover:border-emerald-500/60 hover:bg-card dark:from-emerald-500/15";

/**
 * Skill 与 MCP 分组共用的卡片：拖拽把手、名字图标、名称与徽标、成员一行、悬停出现的删除，
 * 右侧是每个 Agent 的启停图标（单个 Agent 视角下换成这个 Agent 的启用 / 停用按钮）。
 * 点卡片本身打开编辑。
 */
export function GroupCard({
  id,
  name,
  noun,
  memberCount,
  members = [],
  subtitle,
  extraBadges,
  actions,
  removal = GROUP_REMOVAL,
  state,
  viewAgent,
  dragDisabled,
  dragHint,
  busy,
  deleting,
  onOpen,
  onDelete,
  onToggle,
  onReapply,
}: {
  id: string;
  name: string;
  /** 成员的叫法：「skill」或「MCP」 */
  noun: string;
  memberCount: number;
  /** 成员名，已不在 Hub 里的写成「已缺失：<id>」 */
  members?: string[];
  /** 第二行：默认是成员名，项目卡片换成项目路径 */
  subtitle?: ReactNode;
  /** 放在数量徽标后面的额外徽标，如 Skill 的 token 体量 */
  extraBadges?: ReactNode;
  /** 悬停时出现在删除按钮前的操作，如打开项目目录 */
  actions?: ReactNode;
  /** 删除按钮的说法 */
  removal?: { verb: string; title: string; blocked: string };
  state: GroupCardState;
  /** 单个 Agent 视角时选中的 Agent */
  viewAgent?: { id: string; label: string };
  dragDisabled: boolean;
  /** 不能拖动的原因，如单个 Agent 视角 */
  dragHint?: string;
  /** 任一操作进行中：Agent 图标、启停按钮和删除都禁用 */
  busy: boolean;
  /** 这张卡片正在删除 */
  deleting: boolean;
  onOpen: () => void;
  onDelete: () => void;
  onToggle: (agentId: string, activate: boolean) => void;
  /** 依次为这些 Agent 重新应用 */
  onReapply: (agentIds: string[]) => void;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id, disabled: dragDisabled });
  return (
    <div
      ref={setNodeRef}
      style={{
        transform: CSS.Transform.toString(transform),
        transition,
        position: "relative",
        zIndex: isDragging ? 1 : undefined,
      }}
    >
      <ListItemRow
        card
        className={cn(
          state.highlighted && HIGHLIGHT,
          isDragging && "shadow-lg ring-1 ring-primary/30",
        )}
        onPreview={onOpen}
        previewLabel={`编辑 ${name}`}
      >
        <button
          ref={setActivatorNodeRef}
          type="button"
          {...attributes}
          {...listeners}
          aria-label={`拖拽排序 ${name}`}
          title={dragHint ?? "拖拽排序（也可按空格后用方向键移动）"}
          disabled={dragDisabled}
          className="touch-none shrink-0 cursor-grab rounded p-1 text-muted-foreground/50 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring active:cursor-grabbing disabled:cursor-wait"
        >
          <GripVertical className="h-4 w-4" />
        </button>
        <NameAvatar name={name} />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium">{name}</span>
            <Badge variant="outline">
              {memberCount} 个 {noun}
            </Badge>
            {extraBadges}
            {state.badges.map((badge) => (
              <Badge key={badge.label} variant={badge.tone} title={badge.title}>
                {badge.label}
              </Badge>
            ))}
          </div>
          {subtitle ?? (
            <p className="mt-1 truncate text-xs text-muted-foreground">
              {members.join(" · ") || `尚未选择 ${noun}`}
            </p>
          )}
        </div>
        <RowActions busy={deleting}>
          {actions}
          <Button
            variant="ghost"
            size="icon"
            title={state.used ? removal.blocked : removal.title}
            aria-label={`${removal.verb} ${name}`}
            disabled={state.used || busy}
            onClick={onDelete}
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </RowActions>
        {viewAgent && state.view ? (
          <AgentGroupAction
            agent={viewAgent.label}
            groupName={name}
            active={state.view.active}
            blocked={state.blocked}
            reapply={state.view.reapply}
            busy={busy}
            onActivate={() => onToggle(viewAgent.id, true)}
            onStop={() => onToggle(viewAgent.id, false)}
            onReapply={() => onReapply([viewAgent.id])}
          />
        ) : (
          <GroupAgentToggles
            groupName={name}
            busy={busy}
            agents={state.toggles}
            onToggle={onToggle}
            reapply={
              state.reapply && {
                label: state.reapply.label,
                targets: state.reapply.targets,
                disabled: state.reapply.disabled,
                onClick: () => onReapply(state.reapply!.agentIds),
              }
            }
          />
        )}
      </ListItemRow>
    </div>
  );
}
