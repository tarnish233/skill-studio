import {
  SummaryBar,
  SummaryStart,
  SummaryEnd,
  summaryPillClass,
  summaryTabClass,
} from "@/components/common/SummaryBar";
import { mcpAgentId, supportsMcp } from "@/lib/mcpAgents";
import { McpAgentNote } from "@/components/mcp/McpAgentNote";
import { McpAssignments } from "@/components/mcp/McpAssignments";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { useEffect, useMemo, useState } from "react";
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
import { Layers, Pencil, Trash2, GripVertical } from "lucide-react";
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
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import {
  ListContainer,
  ListItemRow,
  RowActions,
} from "@/components/common/ListItemRow";
import { AgentToggleIcon } from "@/components/common/AgentToggleIcon";
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
 * 「已配置 / 未托管 MCP」两个标签仍是单 Agent 视角，用顶部的 Agent 选择器
 * 切换查看对象。
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
  const { data: allAgents = [] } = useAgents();
  const { data: settings } = useSettings();
  // Skill 侧的 Agent 列表（含品牌图标），过滤到真正支持 MCP 且未关闭管理的
  const agents = useMemo(
    () =>
      allAgents.filter(
        (a) => !settings?.disabledAgents?.includes(a.id) && supportsMcp(a.id),
      ),
    [allAgents, settings?.disabledAgents],
  );
  const status = useQuery({
    queryKey: ["mcp", "local"],
    queryFn: managementApi.list,
    // Keep polling in the background without fetching again on every tab switch.
    staleTime: MCP_STATUS_STALE_TIME,
    refetchInterval: MCP_STATUS_POLL_INTERVAL,
  });
  const catalog = rows(status.data);
  const groups = [...(status.data?.groups ?? [])].sort(
    (a, b) => a.sortOrder - b.sortOrder,
  );
  const [query, setQuery] = useState("");
  const [tab, setTab] = useState("groups");
  const [adding, setAdding] = useState(false);
  const [search, setSearch] = useState("");
  const [editing, setEditing] = useState<McpGroup | null>(null);
  const [deleting, setDeleting] = useState<McpGroup | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useUnsavedProject(false, busy);
  // 「已配置 / 未托管」是单 Agent 视角，默认选第一个支持 MCP 的 Agent
  const [installedAgentId, setInstalledAgentId] = useState<string | undefined>(
    undefined,
  );
  useEffect(() => {
    if (installedAgentId && agents.some((a) => a.id === installedAgentId))
      return;
    setInstalledAgentId(agents[0]?.id);
  }, [agents, installedAgentId]);
  const agent = agents.find((a) => a.id === installedAgentId);
  const selectedAgent = agent ? mcpAgentId(agent.id) : undefined;
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );
  async function run(
    method: string,
    params: Record<string, unknown>,
    message: string,
    done?: () => void,
  ) {
    setBusy(true);
    setError(null);
    try {
      await mcpRequest(method, params);
      await status.refetch();
      done?.();
      toast.success(message);
    } catch (e) {
      setError(String(e));
      toast.error(String(e));
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
    void run(
      "reorderGroups",
      { ids: arrayMove(groups, from, to).map((g) => g.id) },
      "分组顺序已保存",
    );
  }
  /**
   * 一个 MCP 分组对某个（MCP 后端）Agent id 当前是什么状态。
   * 成员是否已缺失是全局属性（与哪个 Agent 激活无关），调用方在卡片层统一计算，这里只管
   * 该 Agent 自己的激活记录是否跟得上分组定义的最新成员、是否被外部改动。
   */
  function groupStatus(g: McpGroup, mcpId: string) {
    const current = status.data?.activeGroups?.[mcpId];
    const active = current?.groupId === g.id;
    if (!active || !current)
      return { active, changed: false, issue: undefined };
    const changed =
      JSON.stringify(current.entries.map((e) => e.id)) !==
        JSON.stringify(g.entryIds) ||
      current.entries.some((e) => {
        const found = catalog.find((r) => r.entry.id === e.id)?.entry;
        return !found || snapshot(found) !== snapshot(e);
      });
    return { active, changed, issue: status.data?.groupIssues?.[mcpId] };
  }
  const configured = selectedAgent
    ? catalog.filter((r) =>
        r.sources.some(
          (s) => s.agent === selectedAgent && s.scope === "用户全局",
        ),
      )
    : [];
  const installed = configured.filter((r) =>
    r.sources.some(
      (s) => s.agent === selectedAgent && s.scope === "用户全局" && s.enabled,
    ),
  );
  const manual = selectedAgent
    ? configured.filter(
        (r) =>
          !r.managed &&
          !r.sources.some((s) =>
            Object.values(status.data?.activeGroups ?? {}).some((g) =>
              g.bindings.some(
                (b) =>
                  b.path === s.path && b.key === s.key && b.agent === s.agent,
              ),
            ),
          ),
      )
    : [];
  const showInstalled = tab !== "groups";
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageTools
        query={query}
        onQueryChange={setQuery}
        placeholder={showInstalled ? "搜索已配置 MCP…" : "搜索 MCP 分组…"}
        createLabel={showInstalled ? "添加 MCP" : "新建 MCP 分组"}
        onCreate={() => !busy && (showInstalled ? setAdding(true) : edit())}
      />
      <Tabs
        value={tab}
        onValueChange={(value) => {
          setTab(value);
          setQuery("");
        }}
        className="flex min-h-0 flex-1 flex-col"
      >
        <SummaryBar>
          <SummaryStart>
            <TabsList className="h-9 shrink-0" aria-label="MCP 内容">
              <TabsTrigger
                value="groups"
                className={`${summaryTabClass} gap-2 data-[state=active]:bg-background data-[state=active]:text-foreground dark:data-[state=active]:bg-background`}
              >
                分组 <span className="text-xs opacity-60">{groups.length}</span>
              </TabsTrigger>
              <TabsTrigger
                value="installed"
                className={`${summaryTabClass} gap-2 data-[state=active]:bg-background data-[state=active]:text-foreground dark:data-[state=active]:bg-background`}
              >
                已配置 MCP{" "}
                <span className="text-xs opacity-60">{configured.length}</span>
              </TabsTrigger>
              <TabsTrigger
                value="manual"
                className={`${summaryTabClass} gap-2 data-[state=active]:bg-background data-[state=active]:text-foreground dark:data-[state=active]:bg-background`}
              >
                未托管 MCP{" "}
                <span className="text-xs opacity-60">{manual.length}</span>
              </TabsTrigger>
            </TabsList>
            {/*
              原生 <select> 而不是 Radix —— 本项目的 jsdom 测试约定不测 Radix 浮层的
              展开交互（floating-ui 在 jsdom 里单次展开要 3–50 秒，必然抖）。
            */}
            {showInstalled && agents.length > 0 && (
              <select
                aria-label="选择 Agent"
                className="h-9 shrink-0 rounded-md border border-border-default bg-background px-2 text-sm"
                value={installedAgentId ?? ""}
                onChange={(e) => setInstalledAgentId(e.target.value)}
              >
                {agents.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.displayName}
                  </option>
                ))}
              </select>
            )}
          </SummaryStart>
          <SummaryEnd>
            {showInstalled ? (
              <>
                <Badge variant="outline" className={summaryPillClass}>
                  已启用 {installed.length} 个 MCP
                </Badge>
                {selectedAgent && (
                  <McpAgentNote agent={selectedAgent} compact />
                )}
              </>
            ) : (
              <Badge variant="outline" className={summaryPillClass}>
                共 {groups.length} 个分组
              </Badge>
            )}
          </SummaryEnd>
        </SummaryBar>
        <TabsContent
          value="groups"
          className="min-h-0 flex-1 overflow-y-auto pb-6"
        >
          {status.error && (
            <p role="alert" className="mb-3 text-sm text-red-600">
              {String(status.error)}
            </p>
          )}
          {agents.map((a) => {
            const issue = status.data?.groupIssues?.[mcpAgentId(a.id)];
            return issue ? (
              <p
                key={a.id}
                role="alert"
                className="mb-3 text-sm text-amber-600"
              >
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
                      const statuses = agents.map((a) => {
                        const mcpId = mcpAgentId(a.id);
                        return {
                          agent: a,
                          mcpId,
                          ...groupStatus(group, mcpId),
                        };
                      });
                      const activeStatuses = statuses.filter((s) => s.active);
                      const hasIssue = activeStatuses.some((s) => !!s.issue);
                      const changedForAny = activeStatuses.some(
                        (s) => s.changed,
                      );
                      return (
                        <GroupCard
                          key={group.id}
                          group={group}
                          disabled={busy}
                          active={activeStatuses.length > 0}
                        >
                          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-muted">
                            <Layers className="h-5 w-5" />
                          </div>
                          <div className="min-w-0 flex-1">
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="font-medium">{group.name}</span>
                              <Badge variant="outline">
                                {group.entryIds.length} 个 MCP
                              </Badge>
                              {activeStatuses.length > 0 && (
                                <Badge
                                  variant={hasIssue ? "warning" : "success"}
                                  title={activeStatuses
                                    .map((s) => s.agent.displayName)
                                    .join("、")}
                                >
                                  {hasIssue ? "需要检查" : "使用中"}
                                </Badge>
                              )}
                              {changedForAny && (
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
                                    catalog.find((r) => r.entry.id === id)
                                      ?.entry.name ?? `已缺失：${id}`,
                                )
                                .join(" · ") || "尚未选择 MCP"}
                            </p>
                          </div>
                          <div className="flex shrink-0 items-center gap-1">
                            {statuses.map(
                              ({ agent: a, mcpId, active, changed, issue }) => {
                                // changed（成员与已部署的不一致）点击可以安全重新同步；
                                // issue 和成员已缺失只影响提醒点，点击仍是单纯停用 ——
                                // 不能让一个有问题的分组连关都关不掉。
                                const attention =
                                  changed ||
                                  !!issue ||
                                  (active && missingMembers);
                                const needsReapply = changed;
                                return (
                                  <AgentToggleIcon
                                    key={a.id}
                                    agentId={a.id}
                                    label={a.displayName}
                                    active={active}
                                    attention={attention}
                                    reapply={needsReapply}
                                    disabled={
                                      busy ||
                                      (!active &&
                                        (missingMembers ||
                                          !group.entryIds.length))
                                    }
                                    onClick={() =>
                                      void run(
                                        "activateGroup",
                                        {
                                          agent: mcpId,
                                          id:
                                            !active || needsReapply
                                              ? group.id
                                              : null,
                                        },
                                        !active || needsReapply
                                          ? "分组已启用，请在 Agent 中重新加载 MCP"
                                          : "分组已停用，请在 Agent 中重新加载 MCP",
                                      )
                                    }
                                  />
                                );
                              },
                            )}
                          </div>
                          <RowActions busy={busy}>
                            <Button
                              variant="ghost"
                              size="icon"
                              aria-label={`编辑 ${group.name}`}
                              disabled={busy}
                              onClick={() => edit(group)}
                            >
                              <Pencil className="h-4 w-4" />
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon"
                              aria-label={`删除 ${group.name}`}
                              title={
                                activeStatuses.length > 0
                                  ? "请先停用分组"
                                  : "删除分组"
                              }
                              disabled={busy || activeStatuses.length > 0}
                              onClick={() => setDeleting(group)}
                            >
                              <Trash2 className="h-4 w-4" />
                            </Button>
                          </RowActions>
                        </GroupCard>
                      );
                    })}
                </ListContainer>
              </SortableContext>
            </DndContext>
          </div>
        </TabsContent>
        {["installed", "manual"].map((value) => (
          <TabsContent
            key={value}
            value={value}
            className="min-h-0 flex-1 overflow-y-auto pb-6"
          >
            {selectedAgent ? (
              <McpAssignments
                agent={selectedAgent}
                searchQuery={query}
                manualOnly={value === "manual"}
                addOpen={adding}
                onAddOpenChange={setAdding}
              />
            ) : (
              <p className="p-8 text-center text-sm text-muted-foreground">
                暂无可用的 Agent
              </p>
            )}
          </TabsContent>
        ))}
      </Tabs>
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
                  "saveGroup",
                  {
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
                  },
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
          void run("removeGroup", { id: deleting.id }, "分组已删除", () =>
            setDeleting(null),
          )
        }
      />
    </div>
  );
}
function GroupCard({
  group,
  disabled,
  active,
  children,
}: {
  group: McpGroup;
  disabled: boolean;
  active: boolean;
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
      >
        <button
          type="button"
          {...attributes}
          {...listeners}
          disabled={disabled}
          aria-label={`拖动排序 ${group.name}`}
          className="touch-none text-muted-foreground"
        >
          <GripVertical className="h-4 w-4" />
        </button>
        {children}
      </ListItemRow>
    </div>
  );
}
