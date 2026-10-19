import { useState, type ReactNode } from "react";
import { Input } from "@/components/ui/input";
import { DetailHeader } from "@/components/common/DetailHeader";
import { GroupAgentToggles } from "@/components/common/GroupAgentToggles";
import { useUnsavedProject } from "@/components/common/NavigationGuard";
import {
  MemberChecklist,
  type ChecklistItem,
} from "@/components/projects/MemberChecklist";
import type { GroupCardState } from "./groupCardState";

export interface GroupDraft {
  name: string;
  memberIds: string[];
}

/** 保存的结果。`saved`：分组本身已经存下，之后写入某个 Agent 失败时也为 true */
export interface GroupSaveResult {
  saved: boolean;
  error?: string;
}

/** 成员顺序不影响部署，不算修改 */
const fingerprint = (d: GroupDraft) =>
  JSON.stringify({ name: d.name, memberIds: [...d.memberIds].sort() });

/**
 * Skill 与 MCP 分组共用的整页编辑，与项目详情同一套：标题栏是返回、名称、各 Agent 的启停
 * （与卡片同一个）和保存，下面是分组名称和成员清单。保存即写入正在使用这个分组的 Agent，
 * 还没有 Agent 在用时只保存；有未保存的修改时 Agent 图标先停用，离开前会确认。
 */
export function GroupDetail({
  group,
  noun,
  hub,
  searchLabel,
  options,
  emptyHub,
  note,
  describe,
  state,
  writesTo,
  busy,
  loading,
  onToggle,
  onReapply,
  onBack,
  onSave,
}: {
  /** 新建时为 null */
  group: GroupDraft | null;
  /** 成员的叫法与所在 Hub：「skill」/「Skill Hub」，「MCP」/「MCP Hub」 */
  noun: string;
  hub: string;
  searchLabel: string;
  options: ChecklistItem[];
  /** Hub 里还没有可选成员时的提示 */
  emptyHub: string;
  /** 补充在说明后面的一句话 */
  note?: string;
  /** 标题栏第二行：按当前勾选的成员说明，如数量与 token 体量 */
  describe: (memberIds: string[]) => ReactNode;
  /** 与卡片同一份状态：标题栏的 Agent 图标与「应用修改」 */
  state: GroupCardState;
  /** 保存会写入的 Agent：正在使用这个分组、仍在管理中的 */
  writesTo: string[];
  /** 页面上的启停、重新应用等操作正在进行 */
  busy: boolean;
  /** 数据还没读完或读取失败：不能保存 */
  loading?: boolean;
  onToggle: (agentId: string, activate: boolean) => void;
  onReapply: (agentIds: string[]) => void;
  onBack: () => void;
  onSave: (draft: GroupDraft) => Promise<GroupSaveResult>;
}) {
  const [value, setValue] = useState<GroupDraft>(() => ({
    name: group?.name ?? "",
    memberIds: [...(group?.memberIds ?? [])],
  }));
  const [saved, setSaved] = useState(value);
  // 打开时已有的成员排在最前，勾选过程中不跳动
  const [pinned] = useState(value.memberIds);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const isNew = !group;
  const dirty = fingerprint(value) !== fingerprint(saved);
  const locked = saving || busy;
  useUnsavedProject(dirty, locked);

  // 已不在 Hub 里的成员仍列出来：取消勾选即移除，之后不能再选回
  const items: ChecklistItem[] = [
    ...value.memberIds
      .filter((id) => !options.some((o) => o.id === id))
      .map((id) => ({
        id,
        name: `已缺失：${id}`,
        disabledReason: `已不在 ${hub} 里，取消勾选即可移除`,
      })),
    ...options,
  ];
  const toggleMember = (id: string) =>
    setValue((prev) => ({
      ...prev,
      memberIds: prev.memberIds.includes(id)
        ? prev.memberIds.filter((x) => x !== id)
        : [...prev.memberIds, id],
    }));
  async function save() {
    setSaving(true);
    setError(null);
    const draft = value;
    const result = await onSave(draft);
    if (result.saved) setSaved(draft);
    if (result.error) setError(result.error);
    setSaving(false);
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <DetailHeader
        title={value.name.trim() || (isNew ? "新建分组" : saved.name)}
        subtitle={describe(value.memberIds)}
        onBack={onBack}
        agents={
          <GroupAgentToggles
            groupName={saved.name || "新分组"}
            busy={locked}
            locked={
              isNew
                ? "先保存分组，再为 Agent 启用"
                : dirty
                  ? "先保存修改，再为 Agent 启停"
                  : undefined
            }
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
        }
        save={{
          onClick: () => void save(),
          pending: saving,
          disabled: locked || loading || !dirty || !value.name.trim(),
          title: writesTo.length
            ? `保存并写入 ${writesTo.join("、")}`
            : "保存成员；还没有 Agent 在用，不写入",
        }}
      />
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      {/* Leave room for focus rings inside the scroll viewport without shifting the fields. */}
      <div className="-mx-1 min-h-0 flex-1 space-y-6 overflow-y-auto px-1 pb-6 pt-1">
        <section className="space-y-2">
          <h3 className="text-sm font-semibold">分组名称</h3>
          <Input
            aria-label="分组名称"
            placeholder="例如：前端开发"
            className="max-w-md"
            value={value.name}
            disabled={locked}
            onChange={(e) => setValue({ ...value, name: e.target.value })}
          />
          <p className="text-xs text-muted-foreground">
            选择 {noun} 组合，在标题栏或卡片上为需要的 Agent
            启用。保存会立即写入正在使用这个分组的 Agent。
            {note}
          </p>
        </section>
        <MemberChecklist
          title={`选择 ${noun}`}
          items={items}
          selected={value.memberIds}
          pinned={pinned}
          onToggle={toggleMember}
          disabled={locked}
          searchPlaceholder={`搜索 ${hub}…`}
          searchLabel={searchLabel}
          emptyText={emptyHub}
        />
      </div>
    </div>
  );
}
