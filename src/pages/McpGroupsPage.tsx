import { mcpAgentId, supportsMcp } from "@/lib/mcpAgents";
import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
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
import { Trash2, GripVertical } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { PageTools } from "@/components/common/PageTools";
import { NameAvatar } from "@/components/common/NameAvatar";
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
import { summaryPillClass } from "@/components/common/SummaryBar";
import { McpAgentNote } from "@/components/mcp/McpAgentNote";
import { agentDetection } from "@/lib/agents";
import { sameMembers } from "@/lib/utils";
import { useTarget } from "@/components/targets/TargetProvider";
import { useUnsavedProject } from "@/components/common/NavigationGuard";
import { useAgents, useSettings } from "@/hooks/useData";
import {
  managementApi,
  MCP_STATUS_STALE_TIME,
  MCP_STATUS_POLL_INTERVAL,
  rows,
  type McpGroup,
  type ManagedMcp,
} from "@/lib/api/mcpManagement";
import { mcpRequest } from "@/lib/api/mcp";

/**
 * 分组不再归属某个 Agent：名称和成员只保存一次，是否对某个 Agent 生效完全
 * 由卡片上的 Agent 图标独立决定，同一分组可以同时是多个 Agent 的当前组合。
 * 部署到 Agent 只通过分组；顶部胶囊可切到单个 Agent 的视角。
 */
export function McpGroupsPage() {
  const target = useTarget();
  if (target.id !== "local")
    return (
      <p className="p-6 text-sm text-muted-foreground">
        MCP 分组当前仅支持本机。
      </p>
    );
  return <LocalGroups />;
}
function snapshot(entry: ManagedMcp) {
  return JSON.stringify([
    entry.id,
    entry.name,
    entry.mode,
    entry.definition,
    entry.oauth,
    entry.clientId,
    entry.scopes,
  ]);
}
function LocalGroups() {
  const { data: allAgents = [], isDetecting, detectionFailed } = useAgents();
  const { data: settings } = useSettings();
  // Skill 侧的 Agent 列表（含品牌图标），过滤到真正支持 MCP 的；agents 再去掉关闭管理的
  const mcpAgents = useMemo(
    () => allAgents.filter((a) => supportsMcp(a.id)),
    [allAgents],
  );
  const agents = useMemo(
    () => mcpAgents.filter((a) => !settings?.disabledAgents?.includes(a.id)),
    [mcpAgents, settings?.disabledAgents],
  );
  const managedIds = new Set(agents.map((a) => a.id));
  const status = useQuery({
    queryKey: ["mcp", "local"],
    queryFn: managementApi.list,
    // Keep polling in the background without fetching again on every tab switch.
    staleTime: MCP_STATUS_STALE_TIME,
    refetchInterval: MCP_STATUS_POLL_INTERVAL,
  });
  const catalog = rows(status.data);
  const activeGroups = status.data?.activeGroups ?? {};
  const groups = [...(status.data?.groups ?? [])].sort(
    (a, b) => a.sortOrder - b.sortOrder,
  );
  const [viewAgentId, setViewAgentId] = useGroupAgentView(
    "mcp",
    agents.map((a) => a.id),
  );
  const viewAgent = agents.find((a) => a.id === viewAgentId);
  const viewMcpId = viewAgent ? mcpAgentId(viewAgent.id) : undefined;
  const activeCount = (agentId: string) =>
    (activeGroups[mcpAgentId(agentId)] ?? []).length;
  const activeIds = (mcpId: string) =>
    (activeGroups[mcpId] ?? []).map((g) => g.groupId);
  // 单个 Agent 视角的统计：用户全局配置里已启用的 MCP（与原「已配置 MCP」同口径）
  const enabledMcp = viewMcpId
    ? catalog.filter((r) =>
        r.sources.some(
          (s) => s.agent === viewMcpId && s.scope === "用户全局" && s.enabled,
        ),
      ).length
    : 0;
  const [query, setQuery] = useState("");
  const [search, setSearch] = useState("");
  const [editing, setEditing] = useState<McpGroup | null>(null);
  const [deleting, setDeleting] = useState<McpGroup | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useUnsavedProject(false, busy);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );
  async function run(
    action: () => Promise<unknown>,
    message: string,
    done?: () => void,
  ) {
    setBusy(true);
    setError(null);
    try {
      await action();
      await status.refetch();
      done?.();
      toast.success(message);
    } catch (e) {
      setError(String(e));
      toast.error(String(e));
      // 多步操作可能已部分生效（依次重新应用时后一个 Agent 失败），刷新出真实状态
      void status.refetch();
    } finally {
      setBusy(false);
    }
  }
  function edit(group?: McpGroup) {
    setSearch("");
    setError(null);
    setEditing(
      group
        ? { ...group, entryIds: [...group.entryIds] }
        : {
            id: crypto.randomUUID(),
            name: "",
            entryIds: [],
            sortOrder: groups.length,
          },
    );
  }
  function reorder({ active: dragged, over }: DragEndEvent) {
    if (!over || busy || dragged.id === over.id) return;
    const from = groups.findIndex((g) => g.id === dragged.id),
      to = groups.findIndex((g) => g.id === over.id);
    if (from < 0 || to < 0) return;
    const ids = arrayMove(groups, from, to).map((g) => g.id);
    void run(() => mcpRequest("reorderGroups", { ids }), "分组顺序已保存");
  }
  /**
   * 一个 MCP 分组对某个（MCP 后端）Agent id 当前是什么状态。
   * 成员是否已缺失是全局属性（与哪个 Agent 激活无关），调用方在卡片层统一计算，这里只管
   * 该 Agent 自己的激活记录是否跟得上分组定义的最新成员、是否被外部改动。
   */
  function groupStatus(g: McpGroup, mcpId: string) {
    const current = (status.data?.activeGroups?.[mcpId] ?? []).find(
      (record) => record.groupId === g.id,
    );
    const active = !!current;
    if (!active || !current)
      return { active, changed: false, issue: undefined };
    const changed =
      !sameMembers(
        current.entries.map((e) => e.id),
        g.entryIds,
      ) ||
      current.entries.some((e) => {
        const found = catalog.find((r) => r.entry.id === e.id)?.entry;
        return !found || snapshot(found) !== snapshot(e);
      });
    return { active, changed, issue: status.data?.groupIssues?.[mcpId] };
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageTools
        query={query}
        onQueryChange={setQuery}
        placeholder="搜索 MCP 分组…"
        createLabel="新建 MCP 分组"
        onCreate={() => !busy && edit()}
      />
      <GroupAgentFilter
        total={groups.length}
        agents={agents}
        current={activeCount}
        selected={viewAgent?.id ?? null}
        onSelect={setViewAgentId}
      >
        {viewAgent && viewMcpId ? (
          <>
            <Badge variant="outline" className={summaryPillClass}>
              已启用 {enabledMcp} 个 MCP
            </Badge>
            <McpAgentNote agent={viewMcpId} compact />
          </>
        ) : (
          <Badge variant="outline" className={summaryPillClass}>
            共 {groups.length} 个分组
          </Badge>
        )}
      </GroupAgentFilter>
      <div className="min-h-0 flex-1 overflow-y-auto pb-6">
        {status.error && (
          <p role="alert" className="mb-3 text-sm text-red-600">
            {String(status.error)}
          </p>
        )}
        {mcpAgents.map((a) => {
          const issue = status.data?.groupIssues?.[mcpAgentId(a.id)];
          return issue ? (
            <p key={a.id} role="alert" className="mb-3 text-sm text-amber-600">
              {a.displayName}：{issue}
            </p>
          ) : null;
        })}
        <div>
          {status.isPending ? (
            <p className="p-8 text-sm text-muted-foreground">正在扫描 MCP…</p>
          ) : (
            !groups.length && (
              <p className="p-8 text-center text-sm text-muted-foreground">
                还没有分组。新建一个组合，从 MCP Hub 选择需要的 MCP。
              </p>
            )
          )}
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={reorder}
          >
            <SortableContext
              items={groups.map((g) => g.id)}
              strategy={verticalListSortingStrategy}
            >
              <ListContainer cards>
                {groups
                  .filter((g) =>
                    `${g.name} ${g.entryIds.map((id) => catalog.find((r) => r.entry.id === id)?.entry.name ?? "").join(" ")}`
                      .toLowerCase()
                      .includes(query.trim().toLowerCase()),
                  )
                  .map((group) => {
                    const missingMembers = group.entryIds.some(
                      (id) => !catalog.some((r) => r.entry.id === id),
                    );
                    // 已关闭管理的 Agent 不显示图标，但若它仍在用这个分组，保留一个只能停用的图标
                    const statuses = mcpAgents
                      .filter(
                        (a) =>
                          managedIds.has(a.id) ||
                          (activeGroups[mcpAgentId(a.id)] ?? []).some(
                            (v) => v.groupId === group.id,
                          ),
                      )
                      .map((a) => {
                        const mcpId = mcpAgentId(a.id);
                        return {
                          agent: a,
                          mcpId,
                          managed: managedIds.has(a.id),
                          ...groupStatus(group, mcpId),
                        };
                      });
                    const activeStatuses = statuses.filter((s) => s.active);
                    // 是否在用以全部启用记录为准，不只看显示出来的 Agent
                    const inUse = Object.values(activeGroups)
                      .flat()
                      .some((g) => g.groupId === group.id);
                    const hasIssue = activeStatuses.some((s) => !!s.issue);
                    const changedForAny = activeStatuses.some((s) => s.changed);
                    const reapplyTargets = activeStatuses.filter(
                      (s) => s.managed && (s.changed || !!s.issue),
                    );
                    // 单个 Agent 视角：徽标与高亮只说这个 Agent 的情况
                    const view = viewMcpId && groupStatus(group, viewMcpId);
                    const shown = view
                      ? {
                          active: view.active,
                          changed: view.changed,
                          issue: !!view.issue,
                        }
                      : {
                          active: inUse,
                          changed: changedForAny,
                          issue: hasIssue,
                        };
                    return (
                      <GroupCard
                        key={group.id}
                        group={group}
                        disabled={busy || !!viewAgent}
                        dragHint={
                          viewAgent ? "在「全部」视角下拖动排序" : undefined
                        }
                        active={shown.active}
                        onOpen={() => edit(group)}
                      >
                        <NameAvatar name={group.name} />
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="font-medium">{group.name}</span>
                            <Badge variant="outline">
                              {group.entryIds.length} 个 MCP
                            </Badge>
                            {shown.active && (
                              <Badge
                                variant={shown.issue ? "warning" : "success"}
                                title={
                                  viewAgent?.displayName ??
                                  activeStatuses
                                    .map((s) => s.agent.displayName)
                                    .join("、")
                                }
                              >
                                {shown.issue ? "需要检查" : "使用中"}
                              </Badge>
                            )}
                            {shown.changed && (
                              <Badge variant="warning">有待应用修改</Badge>
                            )}
                            {missingMembers && (
                              <Badge variant="warning">成员已缺失</Badge>
                            )}
                          </div>
                          <p className="mt-1 truncate text-xs text-muted-foreground">
                            {group.entryIds
                              .map(
                                (id) =>
                                  catalog.find((r) => r.entry.id === id)?.entry
                                    .name ?? `已缺失：${id}`,
                              )
                              .join(" · ") || "尚未选择 MCP"}
                          </p>
                        </div>
                        <RowActions busy={busy}>
                          <Button
                            variant="ghost"
                            size="icon"
                            aria-label={`删除 ${group.name}`}
                            title={inUse ? "请先停用分组" : "删除分组"}
                            disabled={busy || inUse}
                            onClick={() => setDeleting(group)}
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </RowActions>
                        {viewAgent && viewMcpId && view ? (
                          <AgentGroupAction
                            agent={viewAgent.displayName}
                            groupName={group.name}
                            active={view.active}
                            blocked={
                              missingMembers
                                ? "成员已缺失，请先编辑分组"
                                : group.entryIds.length
                                  ? undefined
                                  : "分组还没有成员"
                            }
                            reapply={
                              view.changed || view.issue
                                ? {
                                    label: view.changed
                                      ? "应用修改"
                                      : "重新应用",
                                    // 成员缺失时重新应用必然失败，先编辑分组
                                    disabled: missingMembers,
                                  }
                                : undefined
                            }
                            busy={busy}
                            onActivate={() =>
                              void run(
                                () =>
                                  mcpRequest("activateGroup", {
                                    agent: viewMcpId,
                                    ids: [...activeIds(viewMcpId), group.id],
                                  }),
                                "分组已启用，请在 Agent 中重新加载 MCP",
                              )
                            }
                            onStop={() =>
                              void run(
                                () =>
                                  mcpRequest("activateGroup", {
                                    agent: viewMcpId,
                                    ids: activeIds(viewMcpId).filter(
                                      (id) => id !== group.id,
                                    ),
                                  }),
                                "分组已停用，请在 Agent 中重新加载 MCP",
                              )
                            }
                            onReapply={() =>
                              void run(
                                () =>
                                  mcpRequest("activateGroup", {
                                    agent: viewMcpId,
                                    ids: activeIds(viewMcpId),
                                  }),
                                "修改已应用，请在 Agent 中重新加载 MCP",
                              )
                            }
                          />
                        ) : (
                          <GroupAgentToggles
                            groupName={group.name}
                            busy={busy}
                            agents={statuses.map(
                              ({
                                agent: a,
                                managed,
                                active,
                                changed,
                                issue,
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
                                    : issue
                                      ? "需要检查"
                                      : active && missingMembers
                                        ? "成员已缺失"
                                        : undefined,
                                  note: managed ? detection.note : "已关闭管理",
                                  dimmed: managed && detection.notInstalled,
                                  blocked: !managed
                                    ? "已关闭管理"
                                    : missingMembers
                                      ? "成员已缺失，请先编辑分组"
                                      : group.entryIds.length
                                        ? undefined
                                        : "分组还没有成员",
                                };
                              },
                            )}
                            onToggle={(agentId, on) => {
                              const mcpId = mcpAgentId(agentId);
                              void run(
                                () =>
                                  mcpRequest("activateGroup", {
                                    agent: mcpId,
                                    ids: on
                                      ? [...activeIds(mcpId), group.id]
                                      : activeIds(mcpId).filter(
                                          (id) => id !== group.id,
                                        ),
                                  }),
                                on
                                  ? "分组已启用，请在 Agent 中重新加载 MCP"
                                  : "分组已停用，请在 Agent 中重新加载 MCP",
                              );
                            }}
                            reapply={
                              reapplyTargets.length > 0
                                ? {
                                    label: reapplyTargets.some((t) => t.changed)
                                      ? "应用修改"
                                      : "重新应用",
                                    targets: reapplyTargets.map(
                                      (t) => t.agent.displayName,
                                    ),
                                    // 成员缺失时重新应用必然失败，先编辑分组
                                    disabled:
                                      missingMembers || !group.entryIds.length,
                                    onClick: () =>
                                      void run(async () => {
                                        // 依次重新应用：遇到第一个失败就停下报错
                                        for (const t of reapplyTargets)
                                          await mcpRequest("activateGroup", {
                                            agent: t.mcpId,
                                            ids: activeIds(t.mcpId),
                                          });
                                      }, "修改已应用，请在 Agent 中重新加载 MCP"),
                                  }
                                : undefined
                            }
                          />
                        )}
                      </GroupCard>
                    );
                  })}
              </ListContainer>
            </SortableContext>
          </DndContext>
        </div>
      </div>
      <Dialog
        open={!!editing}
        onOpenChange={(open) => !open && !busy && setEditing(null)}
      >
        <DialogContent className="max-h-[85vh] overflow-hidden sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>
              {groups.some((g) => g.id === editing?.id)
                ? "编辑 MCP 分组"
                : "新建 MCP 分组"}
            </DialogTitle>
            <DialogDescription>
              选择 MCP 组合，保存后可在卡片上为需要的 Agent
              启用。保存只记录成员，不会托管或修改已有配置。
            </DialogDescription>
          </DialogHeader>
          <div className="min-h-0 space-y-3 overflow-y-auto px-6 py-4">
            <p className="text-sm font-medium">分组名称</p>
            <Input
              aria-label="MCP 分组名称"
              placeholder="例如：日常开发"
              disabled={busy}
              value={editing?.name ?? ""}
              onChange={(e) =>
                setEditing((v) => v && { ...v, name: e.target.value })
              }
            />
            <p className="text-sm font-medium">
              选择 MCP · 已选 {editing?.entryIds.length ?? 0} 个
            </p>
            <Input
              aria-label="搜索 Hub MCP"
              placeholder="搜索 MCP Hub…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
            <div className="max-h-72 space-y-2 overflow-y-auto">
              {catalog
                .filter((r) =>
                  `${r.entry.name} ${r.entry.definition.command ?? ""} ${r.entry.definition.url ?? ""}`
                    .toLowerCase()
                    .includes(search.toLowerCase()),
                )
                .map((row) => (
                  <label
                    key={row.entry.id}
                    className="flex cursor-pointer items-center gap-3 rounded-lg border border-border-default p-3"
                  >
                    <Checkbox
                      aria-label={`选择 ${row.entry.name}`}
                      disabled={
                        busy ||
                        (!row.managed && row.sources.some((s) => s.gateway))
                      }
                      checked={
                        editing?.entryIds.includes(row.entry.id) ?? false
                      }
                      onCheckedChange={(checked) =>
                        setEditing(
                          (v) =>
                            v && {
                              ...v,
                              entryIds: checked
                                ? [...v.entryIds, row.entry.id]
                                : v.entryIds.filter(
                                    (id) => id !== row.entry.id,
                                  ),
                            },
                        )
                      }
                    />
                    <div className="min-w-0">
                      <p className="text-sm font-medium">{row.entry.name}</p>
                      <p className="truncate text-xs text-muted-foreground">
                        {String(
                          row.entry.definition.command ??
                            row.entry.definition.url ??
                            "",
                        )}{" "}
                        · {row.entry.mode === "gateway" ? "网关" : "直连"}
                      </p>
                    </div>
                  </label>
                ))}
              {editing?.entryIds
                .filter((id) => !catalog.some((r) => r.entry.id === id))
                .map((id) => (
                  <label key={id} className="flex gap-3 text-sm text-red-600">
                    <Checkbox
                      checked
                      disabled={busy}
                      aria-label={`移除缺失成员 ${id}`}
                      onCheckedChange={() =>
                        setEditing(
                          (v) =>
                            v && {
                              ...v,
                              entryIds: v.entryIds.filter((x) => x !== id),
                            },
                        )
                      }
                    />
                    已缺失：{id}
                  </label>
                ))}
              {!catalog.length && (
                <p className="py-4 text-sm text-muted-foreground">
                  MCP Hub 暂无服务，请先添加 MCP。
                </p>
              )}
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
              disabled={busy}
              onClick={() => setEditing(null)}
            >
              取消
            </Button>
            <Button
              disabled={
                busy ||
                status.isPending ||
                !!status.error ||
                !editing?.name.trim()
              }
              onClick={() =>
                editing &&
                void run(
                  () =>
                    mcpRequest("saveGroup", {
                      group: editing,
                      imports: catalog
                        .filter(
                          (r) =>
                            !r.managed && editing.entryIds.includes(r.entry.id),
                        )
                        .map((r) => ({
                          id: r.entry.id,
                          definition: r.entry.definition,
                          sourceIds: r.sources.map((source) => source.id),
                        })),
                    }),
                  "组合已保存，可在卡片上为需要的 Agent 启用",
                  () => setEditing(null),
                )
              }
            >
              {busy ? "保存中…" : "保存"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={`删除分组「${deleting?.name ?? ""}」`}
        description="只删除组合记录，不删除 MCP Hub 中的服务。"
        confirmText="删除"
        pending={busy}
        onConfirm={() =>
          deleting &&
          void run(
            () => mcpRequest("removeGroup", { id: deleting.id }),
            "分组已删除",
            () => setDeleting(null),
          )
        }
      />
    </div>
  );
}
function GroupCard({
  group,
  disabled,
  dragHint,
  active,
  onOpen,
  children,
}: {
  group: McpGroup;
  disabled: boolean;
  /** 拖动不可用的原因（比如在单个 Agent 视角下） */
  dragHint?: string;
  active: boolean;
  /** 点整张卡片时打开的管理界面（编辑分组） */
  onOpen?: () => void;
  children: React.ReactNode;
}) {
  const { attributes, listeners, setNodeRef, transform, transition } =
    useSortable({ id: group.id, disabled });
  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
    >
      <ListItemRow
        card
        className={
          active
            ? "border-emerald-500/40 bg-card bg-gradient-to-r from-emerald-500/10 to-transparent"
            : undefined
        }
        onPreview={onOpen}
        previewLabel={onOpen ? `编辑 ${group.name}` : undefined}
      >
        <button
          type="button"
          {...attributes}
          {...listeners}
          disabled={disabled}
          aria-label={`拖动排序 ${group.name}`}
          title={dragHint ?? "拖动排序"}
          className="touch-none shrink-0 cursor-grab rounded p-1 text-muted-foreground/50 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring active:cursor-grabbing disabled:cursor-wait"
        >
          <GripVertical className="h-4 w-4" />
        </button>
        {children}
      </ListItemRow>
    </div>
  );
}
