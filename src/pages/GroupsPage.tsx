import { PageTools } from "@/components/common/PageTools";
import { NameAvatar } from "@/components/common/NameAvatar";
import { useMemo, useState, type ReactNode } from "react";
import {
  DndContext,
  PointerSensor,
  KeyboardSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  arrayMove,
  sortableKeyboardCoordinates,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { queryKeys, NAVIGATION_STALE_TIME } from "@/lib/queryKeys";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Trash2, GripVertical } from "lucide-react";
import { toast } from "sonner";
import { groupsApi, settingsApi } from "@/lib/api";
import { useAgents, useGroups, useSkills, useSettings } from "@/hooks/useData";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import {
  ListContainer,
  ListItemRow,
  RowActions,
} from "@/components/common/ListItemRow";
import { GroupAgentToggles } from "@/components/common/GroupAgentToggles";
import {
  AgentGroupAction,
  GroupAgentFilter,
  useGroupAgentView,
} from "@/components/common/GroupAgentView";
import { HelpPopover } from "@/components/common/HelpPopover";
import { summaryPillClass } from "@/components/common/SummaryBar";
import { agentDetection } from "@/lib/agents";
import { sameMembers } from "@/lib/utils";
import { formatTokens, sumTokens, tokenIndex, tokenTitle } from "@/lib/tokens";
import { isCopyDrifted, isRegistered, needsManualFix } from "@/lib/linkStatus";
import type { Group } from "@/types";

/**
 * 分组不再归属某个 Agent：名称和成员只保存一次，是否对某个 Agent 生效完全
 * 由卡片上的 Agent 图标独立决定（见 `GroupAgentToggles`），同一分组可以同时
 * 是多个 Agent 的当前组合。部署到 Agent 只通过分组；顶部胶囊可切到单个
 * Agent 的视角（见 `GroupAgentFilter`）。
 */
export function GroupsPage() {
  const { data: allAgents = [], isDetecting, detectionFailed } = useAgents();
  const { data: liveSettings } = useSettings();
  const { data: liveGroups = [] } = useGroups();
  const { data: liveSkills = [] } = useSkills();
  const { data: liveConfig } = useQuery({
    queryKey: queryKeys.config,
    queryFn: settingsApi.getConfig,
    staleTime: NAVIGATION_STALE_TIME,
  });
  // Keep one consistent render while config and filesystem queries refresh independently.
  const [operationView, setOperationView] = useState<{
    settings: typeof liveSettings;
    groups: typeof liveGroups;
    skills: typeof liveSkills;
    config: typeof liveConfig;
  } | null>(null);
  const {
    settings,
    groups: rawGroups,
    skills,
    config,
  } = operationView ?? {
    settings: liveSettings,
    groups: liveGroups,
    skills: liveSkills,
    config: liveConfig,
  };
  const agents = useMemo(
    () => allAgents.filter((a) => !settings?.disabledAgents?.includes(a.id)),
    [allAgents, settings?.disabledAgents],
  );
  const groups = useMemo(
    () => [...rawGroups].sort((a, b) => a.sortOrder - b.sortOrder),
    [rawGroups],
  );
  const client = useQueryClient();
  const [editing, setEditing] = useState<{
    id: string | null;
    name: string;
    skillIds: string[];
  } | null>(null);
  const [query, setQuery] = useState("");
  const [skillSearch, setSkillSearch] = useState("");
  const [deleting, setDeleting] = useState<Group | null>(null);
  const [error, setError] = useState<string | null>(null);
  const managedIds = new Set(agents.map((a) => a.id));
  const activeGroups = config?.activeGroups ?? {};
  const [viewAgentId, setViewAgentId] = useGroupAgentView(
    "skills",
    agents.map((a) => a.id),
  );
  const viewAgent = agents.find((a) => a.id === viewAgentId);
  const activeCount = (agentId: string) => (activeGroups[agentId] ?? []).length;
  const activeIds = (agentId: string) =>
    (activeGroups[agentId] ?? []).map((g) => g.groupId);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );
  const reorder = useMutation({
    mutationFn: (next: Group[]) => groupsApi.reorder(next.map((g) => g.id)),
    onMutate: async (next) => {
      await client.cancelQueries({ queryKey: queryKeys.groups });
      const previous = client.getQueryData<Group[]>(queryKeys.groups);
      client.setQueryData(queryKeys.groups, next);
      return { previous };
    },
    onSuccess: (saved) => client.setQueryData(queryKeys.groups, saved),
    onError: (e, _, context) => {
      if (context?.previous)
        client.setQueryData(queryKeys.groups, context.previous);
      toast.error(`排序保存失败：${String(e)}`);
    },
    onSettled: () => client.invalidateQueries({ queryKey: queryKeys.groups }),
  });
  const onDragEnd = ({ active, over }: DragEndEvent) => {
    if (!over || active.id === over.id || reorder.isPending) return;
    const from = groups.findIndex((g) => g.id === active.id);
    const to = groups.findIndex((g) => g.id === over.id);
    if (from < 0 || to < 0) return;
    reorder.mutate(
      arrayMove(groups, from, to).map((g, sortOrder) => ({
        ...g,
        sortOrder,
      })),
    );
  };
  // 索引跟着 skills 走：与 ProjectsPage 同一口径，别在每次重渲染时重建一遍
  const tokens = useMemo(() => tokenIndex(skills), [skills]);
  const groupTokens = (g: Group) => sumTokens(tokens, g.skillIds);

  /**
   * 一个分组对某个 Agent 当前是什么状态：
   * - active：该 Agent 的启用记录正指向这个分组
   * - changed：分组定义已变化（成员、保留策略）或某个成员副本已过期，与「有待应用修改」同义
   * - unavailable：启用记录里的某个成员在这个 Agent 上缺失/被停用/需要人工处理，与「需要检查」同义
   */
  function groupStatus(g: Group, agentId: string) {
    const current = (config?.activeGroups?.[agentId] ?? []).find(
      (record) => record.groupId === g.id,
    );
    const active = !!current;
    if (!active || !current)
      return { active, changed: false, unavailable: false };
    const changed =
      (current.entries?.some(
        (entry) =>
          skills.find((s) => s.id === entry.skillId)?.agents[agentId]
            ?.status === "copyStale",
      ) ??
        false) ||
      !sameMembers(current.skillIds, g.skillIds) ||
      (current.preserveManualSkills ?? true) !==
        (settings?.preserveManualSkills ?? true);
    const unavailable = current.skillIds.some((id) => {
      const state = skills.find((s) => s.id === id)?.agents[agentId];
      return !state || state.disabled || needsManualFix(state.status);
    });
    return { active, changed, unavailable };
  }

  // 单个 Agent 视角的统计：文件在位、且没被原生开关停用的 skill；
  // 目录里有痕迹但用不上的（外来占位、悬空链接）不计，见 isRegistered
  const enabledSkills = viewAgent
    ? skills.filter((s) => {
        const state = s.agents[viewAgent.id];
        return !!state && !state.disabled && isRegistered(state.status);
      })
    : [];
  const enabledTokens = sumTokens(
    tokens,
    enabledSkills.map((s) => s.id),
  );
  const driftedCount = viewAgent
    ? enabledSkills.filter((s) => isCopyDrifted(s.agents[viewAgent.id].status))
        .length
    : 0;

  const refresh = () => client.invalidateQueries();
  const save = useMutation({
    mutationFn: async () => {
      if (!editing) throw new Error("没有正在编辑的分组");
      return groupsApi.save(editing.id, editing.name, editing.skillIds);
    },
    onSuccess: () => {
      setEditing(null);
      toast.success("分组已保存，可在卡片上为需要的 Agent 启用");
    },
    onError: (e) => setError(String(e)),
    onSettled: refresh,
  });
  const activate = useMutation({
    mutationFn: async (v: {
      kind: "enable" | "disable" | "reapply";
      /** agent → 操作完成后它要叠加的分组列表 */
      next: Record<string, string[]>;
    }) => {
      // 依次写：遇到第一个失败就停下报错，已经成功的保持生效
      for (const [agentId, ids] of Object.entries(v.next))
        await groupsApi.activate(agentId, ids);
    },
    onMutate: () =>
      setOperationView({ settings, groups: rawGroups, skills, config }),
    onSuccess: (_, v) =>
      toast.success(
        v.kind === "enable"
          ? "分组已启用"
          : v.kind === "disable"
            ? "分组已停用"
            : "修改已应用",
      ),
    onError: (e) => toast.error(String(e)),
    onSettled: async () => {
      try {
        await refresh();
      } finally {
        setOperationView(null);
      }
    },
  });
  const remove = useMutation({
    mutationFn: (id: string) => groupsApi.remove(id),
    onSuccess: () => setDeleting(null),
    onError: (e) => toast.error(String(e)),
    onSettled: refresh,
  });
  const edit = (g?: Group) => {
    setError(null);
    setSkillSearch("");
    setEditing({
      id: g?.id ?? null,
      name: g?.name ?? "",
      skillIds: [...(g?.skillIds ?? [])],
    });
  };
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageTools
        query={query}
        onQueryChange={setQuery}
        placeholder="搜索分组…"
        createLabel="新建分组"
        onCreate={() => edit()}
      />
      <GroupAgentFilter
        total={groups.length}
        agents={agents}
        current={activeCount}
        selected={viewAgent?.id ?? null}
        onSelect={setViewAgentId}
      >
        {viewAgent ? (
          <>
            <Badge
              variant="outline"
              className={summaryPillClass}
              title={
                viewAgent.supportsNativeToggle
                  ? "文件在位、且没被 agent 的原生开关停用的 skill。目录里有痕迹但用不上的（外来占位、悬空链接）不计。"
                  : "按标准目录中的 Skill 文件统计；实际加载状态由 Agent 自身配置决定。"
              }
            >
              {viewAgent.supportsNativeToggle ? "已启用" : "已安装"}{" "}
              {enabledSkills.length} 个 skill
            </Badge>
            <Badge
              variant="outline"
              className={summaryPillClass}
              title={tokenTitle(
                enabledTokens,
                viewAgent.supportsNativeToggle
                  ? "当前已启用的 skill"
                  : "当前已安装的 skill",
                driftedCount,
              )}
            >
              ≈ {formatTokens(enabledTokens.total)} tokens
            </Badge>
            {!viewAgent.supportsNativeToggle && (
              <HelpPopover label={`${viewAgent.displayName} Skill 使用说明`}>
                {viewAgent.displayName} 本版支持 Skill
                安装、托管和分组部署，暂不读取或修改原生启停配置。
                分组停用只撤回 Studio 部署的副本，手动安装的 Skill
                始终保留。修改后请在 Agent 中重新加载 Skill 或重启会话。
                {viewAgent.id === "grok" &&
                  " Grok 默认还会读取 ~/.claude/skills（Claude Code 兼容），给 Claude Code 部署的 skill 也会被它加载：管理期间 Studio 在 ~/.grok/config.toml 写入 [compat.claude] skills = false 关闭这项读取，退出管理时恢复。"}
                {viewAgent.id === "opencode" &&
                  " OpenCode 还会读取 ~/.claude/skills（Claude Code 兼容），给 Claude Code 部署的 skill 也会被它加载；这项读取只能通过环境变量 OPENCODE_DISABLE_CLAUDE_CODE_SKILLS=1 关闭。"}
              </HelpPopover>
            )}
          </>
        ) : (
          <Badge variant="outline" className={summaryPillClass}>
            共 {groups.length} 个分组
          </Badge>
        )}
      </GroupAgentFilter>
      <div className="min-h-0 flex-1 overflow-y-auto pb-6">
        {groups.length === 0 && (
          <div className="p-8 text-center text-sm text-muted-foreground">
            还没有分组。新建一个组合，从 Skill Hub 选择需要的 skill。
          </div>
        )}
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={onDragEnd}
        >
          <SortableContext
            items={groups.map((g) => g.id)}
            strategy={verticalListSortingStrategy}
          >
            <ListContainer cards>
              {groups
                .filter((g) =>
                  `${g.name} ${g.skillIds.map((id) => skills.find((s) => s.id === id)?.name ?? "").join(" ")}`
                    .toLowerCase()
                    .includes(query.trim().toLowerCase()),
                )
                .map((g) => {
                  // 一行只遍历一次 agents：下面的徽标、图标行、删除保护共用。
                  // 已关闭管理的 Agent 不显示图标，但若它仍在用这个分组，保留一个只能停用的图标。
                  const rowTokens = groupTokens(g);
                  const statuses = allAgents
                    .filter(
                      (a) =>
                        managedIds.has(a.id) ||
                        (activeGroups[a.id] ?? []).some(
                          (v) => v.groupId === g.id,
                        ),
                    )
                    .map((a) => ({
                      agent: a,
                      managed: managedIds.has(a.id),
                      ...groupStatus(g, a.id),
                    }));
                  const activeStatuses = statuses.filter((s) => s.active);
                  // 是否在用以配置里的全部启用记录为准，不只看显示出来的 Agent
                  const inUse = Object.values(activeGroups)
                    .flat()
                    .some((v) => v.groupId === g.id);
                  const unavailable = activeStatuses.some((s) => s.unavailable);
                  const changed = activeStatuses.some((s) => s.changed);
                  const reapplyTargets = activeStatuses.filter(
                    (s) => s.managed && (s.changed || s.unavailable),
                  );
                  // 单个 Agent 视角：徽标与高亮只说这个 Agent 的情况
                  const view = viewAgent && groupStatus(g, viewAgent.id);
                  const shown = view ?? {
                    active: inUse,
                    changed,
                    unavailable,
                  };
                  return (
                    <SortableGroupCard
                      key={g.id}
                      group={g}
                      disabled={reorder.isPending || !!viewAgent}
                      dragHint={
                        viewAgent ? "在「全部」视角下拖动排序" : undefined
                      }
                      onOpen={() => edit(g)}
                      className={
                        shown.active
                          ? "border-emerald-500/40 bg-card bg-gradient-to-r from-emerald-500/10 to-transparent shadow-sm shadow-emerald-500/5 hover:border-emerald-500/60 hover:bg-card dark:from-emerald-500/15"
                          : undefined
                      }
                    >
                      <NameAvatar name={g.name} />
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-medium">{g.name}</span>
                          <Badge variant="outline">
                            {g.skillIds.length} 个 skill
                          </Badge>
                          {rowTokens.total > 0 && (
                            <Badge
                              variant="outline"
                              title={tokenTitle(rowTokens, "组内 skill")}
                            >
                              ≈ {formatTokens(rowTokens.total)} tokens
                            </Badge>
                          )}
                          {shown.active && (
                            <Badge
                              variant={
                                shown.unavailable ? "warning" : "success"
                              }
                              title={
                                viewAgent?.displayName ??
                                activeStatuses
                                  .map((s) => s.agent.displayName)
                                  .join("、")
                              }
                            >
                              {shown.unavailable ? "需要检查" : "使用中"}
                            </Badge>
                          )}
                          {shown.changed && (
                            <Badge variant="warning">有待应用修改</Badge>
                          )}
                        </div>
                        <p className="mt-1 truncate text-xs text-muted-foreground">
                          {g.skillIds
                            .map(
                              (id) =>
                                skills.find((s) => s.id === id)?.name ??
                                `已缺失：${id}`,
                            )
                            .join(" · ") || "尚未选择 skill"}
                        </p>
                      </div>
                      <RowActions
                        busy={remove.isPending && deleting?.id === g.id}
                      >
                        <Button
                          variant="ghost"
                          size="icon"
                          title={inUse ? "请先停用分组" : "删除分组"}
                          aria-label={`删除 ${g.name}`}
                          disabled={inUse || activate.isPending}
                          onClick={() => setDeleting(g)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </RowActions>
                      {viewAgent && view ? (
                        <AgentGroupAction
                          agent={viewAgent.displayName}
                          groupName={g.name}
                          active={view.active}
                          blocked={
                            g.skillIds.length ? undefined : "分组还没有成员"
                          }
                          reapply={
                            view.changed || view.unavailable
                              ? {
                                  label: view.changed ? "应用修改" : "重新应用",
                                }
                              : undefined
                          }
                          busy={activate.isPending}
                          onActivate={() =>
                            activate.mutate({
                              kind: "enable",
                              next: {
                                [viewAgent.id]: [
                                  ...activeIds(viewAgent.id),
                                  g.id,
                                ],
                              },
                            })
                          }
                          onStop={() =>
                            activate.mutate({
                              kind: "disable",
                              next: {
                                [viewAgent.id]: activeIds(viewAgent.id).filter(
                                  (id) => id !== g.id,
                                ),
                              },
                            })
                          }
                          onReapply={() =>
                            activate.mutate({
                              kind: "reapply",
                              next: { [viewAgent.id]: activeIds(viewAgent.id) },
                            })
                          }
                        />
                      ) : (
                        <GroupAgentToggles
                          groupName={g.name}
                          busy={activate.isPending}
                          agents={statuses.map(
                            ({
                              agent: a,
                              managed,
                              active,
                              changed,
                              unavailable,
                            }) => {
                              const detection = agentDetection(a, {
                                isDetecting,
                                detectionFailed,
                              });
                              return {
                                agentId: a.id,
                                label: a.displayName,
                                active,
                                attention: changed
                                  ? "有待应用修改"
                                  : unavailable
                                    ? "需要检查"
                                    : undefined,
                                note: managed ? detection.note : "已关闭管理",
                                dimmed: managed && detection.notInstalled,
                                blocked: !managed
                                  ? "已关闭管理"
                                  : g.skillIds.length
                                    ? undefined
                                    : "分组还没有成员",
                              };
                            },
                          )}
                          onToggle={(agentId, on) =>
                            activate.mutate({
                              kind: on ? "enable" : "disable",
                              next: {
                                [agentId]: on
                                  ? [...activeIds(agentId), g.id]
                                  : activeIds(agentId).filter(
                                      (id) => id !== g.id,
                                    ),
                              },
                            })
                          }
                          reapply={
                            reapplyTargets.length > 0
                              ? {
                                  label: reapplyTargets.some((t) => t.changed)
                                    ? "应用修改"
                                    : "重新应用",
                                  targets: reapplyTargets.map(
                                    (t) => t.agent.displayName,
                                  ),
                                  disabled: !g.skillIds.length,
                                  onClick: () =>
                                    activate.mutate({
                                      kind: "reapply",
                                      next: Object.fromEntries(
                                        reapplyTargets.map((t) => [
                                          t.agent.id,
                                          activeIds(t.agent.id),
                                        ]),
                                      ),
                                    }),
                                }
                              : undefined
                          }
                        />
                      )}
                    </SortableGroupCard>
                  );
                })}
            </ListContainer>
          </SortableContext>
        </DndContext>
      </div>
      <Dialog
        open={editing !== null}
        onOpenChange={(open) => {
          if (!open && !save.isPending) setEditing(null);
        }}
      >
        <DialogContent className="max-h-[85vh] overflow-hidden sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>{editing?.id ? "编辑分组" : "新建分组"}</DialogTitle>
            <DialogDescription>
              选择 skill 组合，保存后可在卡片上为需要的 Agent 启用。
            </DialogDescription>
          </DialogHeader>
          <div className="min-h-0 space-y-3 overflow-y-auto px-6 py-4">
            <p className="text-sm font-medium">分组名称</p>
            <Input
              aria-label="分组名称"
              placeholder="例如：前端开发"
              value={editing?.name ?? ""}
              disabled={save.isPending}
              onChange={(e) =>
                setEditing((prev) => prev && { ...prev, name: e.target.value })
              }
            />
            <p className="text-sm font-medium">
              选择 skill{" "}
              <span className="font-normal text-muted-foreground">
                · 已选 {editing?.skillIds.length ?? 0} 个
              </span>
            </p>
            <Input
              aria-label="搜索 Hub skill"
              placeholder="搜索 Skill Hub…"
              value={skillSearch}
              onChange={(e) => setSkillSearch(e.target.value)}
            />
            <div className="max-h-72 space-y-2 overflow-y-auto">
              {skills
                .filter((s) =>
                  `${s.name} ${s.description ?? ""}`
                    .toLowerCase()
                    .includes(skillSearch.toLowerCase()),
                )
                .map((s) => (
                  <label
                    key={s.id}
                    className="flex cursor-pointer items-center gap-3 rounded-lg border p-3"
                  >
                    <Checkbox
                      aria-label={`选择 ${s.name}`}
                      checked={editing?.skillIds.includes(s.id) ?? false}
                      disabled={save.isPending}
                      onCheckedChange={(on) =>
                        setEditing(
                          (prev) =>
                            prev && {
                              ...prev,
                              skillIds: on
                                ? [...prev.skillIds, s.id]
                                : prev.skillIds.filter((id) => id !== s.id),
                            },
                        )
                      }
                    />
                    <div className="min-w-0">
                      <p className="text-sm font-medium">{s.name}</p>
                      <p className="truncate text-xs text-muted-foreground">
                        {s.description}
                      </p>
                      <p
                        className="mt-1 truncate font-mono text-[10px] text-muted-foreground"
                        title={s.sourcePath}
                      >
                        {s.sourcePath}
                      </p>
                    </div>
                  </label>
                ))}
              {editing?.skillIds
                .filter((id) => !skills.some((s) => s.id === id))
                .map((id) => (
                  <label
                    key={id}
                    className="flex items-center gap-3 text-sm text-red-600"
                  >
                    <Checkbox
                      checked
                      aria-label={`移除缺失成员 ${id}`}
                      disabled={save.isPending}
                      onCheckedChange={() =>
                        setEditing(
                          (prev) =>
                            prev && {
                              ...prev,
                              skillIds: prev.skillIds.filter((x) => x !== id),
                            },
                        )
                      }
                    />
                    已缺失：{id}
                  </label>
                ))}
            </div>
            {error && (
              <p role="alert" className="text-sm text-red-600">
                {error}
              </p>
            )}
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={save.isPending}
              onClick={() => setEditing(null)}
            >
              取消
            </Button>
            <Button
              disabled={save.isPending || !editing?.name.trim()}
              onClick={() => save.mutate()}
            >
              {save.isPending ? "保存中…" : "保存"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(open) => {
          if (!open) setDeleting(null);
        }}
        title={`删除分组「${deleting?.name ?? ""}」`}
        description="删除组合记录，不删除 Skill Hub 中的源文件。"
        confirmText="删除"
        pending={remove.isPending}
        onConfirm={() => {
          if (deleting) remove.mutate(deleting.id);
        }}
      />
    </div>
  );
}

function SortableGroupCard({
  group,
  disabled,
  dragHint,
  className,
  onOpen,
  children,
}: {
  group: Group;
  disabled: boolean;
  /** 拖动不可用的原因（比如在单个 Agent 视角下） */
  dragHint?: string;
  className?: string;
  /** 点整张卡片时打开的管理界面（编辑分组） */
  onOpen?: () => void;
  children: ReactNode;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: group.id, disabled });
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
        className={`${className ?? ""} ${isDragging ? "shadow-lg ring-1 ring-primary/30" : ""}`}
        onPreview={onOpen}
        previewLabel={onOpen ? `编辑 ${group.name}` : undefined}
      >
        <button
          ref={setActivatorNodeRef}
          type="button"
          {...attributes}
          {...listeners}
          aria-label={`拖拽排序 ${group.name}`}
          title={dragHint ?? "拖拽排序（也可按空格后用方向键移动）"}
          disabled={disabled}
          className="touch-none shrink-0 cursor-grab rounded p-1 text-muted-foreground/50 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring active:cursor-grabbing disabled:cursor-wait"
        >
          <GripVertical className="h-4 w-4" />
        </button>
        {children}
      </ListItemRow>
    </div>
  );
}
