import {
  SummaryBar,
  SummaryStart,
  SummaryEnd,
  summaryPillClass,
  summaryTabClass,
} from "@/components/common/SummaryBar";
import { HelpPopover } from "@/components/common/HelpPopover";
import { SkillBackups } from "@/components/common/SkillBackups";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { PageTools } from "@/components/common/PageTools";
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
import { Pencil, Trash2, Layers, GripVertical } from "lucide-react";
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
import { AgentSkills } from "@/pages/AgentPage";
import { GroupAgentToggles } from "@/components/common/GroupAgentToggles";
import { AgentSelect } from "@/components/common/AgentSelect";
import { agentDetection } from "@/lib/agents";
import { sameMembers } from "@/lib/utils";
import { formatTokens, sumTokens, tokenIndex, tokenTitle } from "@/lib/tokens";
import { isCopyDrifted, isRegistered, needsManualFix } from "@/lib/linkStatus";
import type { Group } from "@/types";

/**
 * 分组不再归属某个 Agent：名称和成员只保存一次，是否对某个 Agent 生效完全
 * 由卡片上的 Agent 图标独立决定（见 `GroupAgentToggles`），同一分组可以同时
 * 是多个 Agent 的当前组合。「已安装 / 未托管 skill」两个标签仍是单 Agent
 * 视角，用顶部的 Agent 选择器切换查看对象。
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
  const [tab, setTab] = useState("groups");
  const showInstalled = tab !== "groups";
  const [addingSkill, setAddingSkill] = useState(false);
  // 「已安装 / 未托管」是单 Agent 视角；选中的 Agent 被关闭管理后回到第一个
  const [pickedAgentId, setPickedAgentId] = useState<string>();
  const agent = agents.find((a) => a.id === pickedAgentId) ?? agents[0];
  const managedIds = new Set(agents.map((a) => a.id));
  const activeGroups = config?.activeGroups ?? {};

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
    const current = config?.activeGroups?.[agentId];
    const active = current?.groupId === g.id;
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

  // 「已安装 / 未托管」统计只看当前选中的 Agent；没有可用 Agent 时都是 0
  const enabledSkills = agent
    ? skills.filter((s) => {
        const state = s.agents[agent.id];
        return !!state && !state.disabled && isRegistered(state.status);
      })
    : [];
  const enabledTokens = sumTokens(
    tokens,
    enabledSkills.map((s) => s.id),
  );
  const driftedCount = agent
    ? enabledSkills.filter((s) => isCopyDrifted(s.agents[agent.id].status))
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
      agentIds: string[];
      groupId: string | null;
    }) => {
      // 依次切换：遇到第一个失败就停下报错，已经成功的保持生效
      for (const agentId of v.agentIds)
        await groupsApi.activate(agentId, v.groupId);
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
        placeholder={showInstalled ? "搜索已安装 skill…" : "搜索分组…"}
        createLabel={showInstalled ? "添加 skill" : "新建分组"}
        onCreate={() => (showInstalled ? setAddingSkill(true) : edit())}
      />
      <Tabs
        value={tab}
        onValueChange={(value) => {
          setTab(value);
          setQuery("");
        }}
        className="flex min-h-0 flex-1 flex-col"
      >
        {/* 分页与统计合成一行：左边切页（+ 单 Agent 标签的 Agent 选择器），右边统计 */}
        <SummaryBar>
          <SummaryStart>
            <TabsList className="h-9 shrink-0" aria-label="分组内容">
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
                已安装 skill
                <span className="text-xs opacity-60">
                  {agent
                    ? skills.filter(
                        (s) =>
                          s.agents[agent.id] &&
                          s.agents[agent.id].status !== "notLinked",
                      ).length
                    : 0}
                </span>
              </TabsTrigger>
              <TabsTrigger
                value="manual"
                className={`${summaryTabClass} gap-2 data-[state=active]:bg-background data-[state=active]:text-foreground`}
              >
                未托管 skill
                <span className="text-xs opacity-60">
                  {agent
                    ? skills.filter((s) => s.agents[agent.id]?.manual).length
                    : 0}
                </span>
              </TabsTrigger>
            </TabsList>
            {showInstalled && agent && (
              <AgentSelect
                agents={agents}
                value={agent.id}
                onChange={setPickedAgentId}
              />
            )}
          </SummaryStart>
          {/*
            这些统计胶囊只报数、这一行没有筛选语义，所以是不可聚焦的 Badge
            而不是按钮。用 Badge 的 outline 变体而不是手抄它的类名 —— 裸 `border`
            会吃到 preflight 推出的 #e4e4e7，深色下就是近白边框套在已经变暗的卡片里。
          */}
          <SummaryEnd>
            {showInstalled ? (
              <>
                {agent && (
                  <SkillBackups
                    scope={`agent:${agent.id}`}
                    className={summaryPillClass}
                  />
                )}
                <Badge
                  variant="outline"
                  className={summaryPillClass}
                  title={
                    agent?.supportsNativeToggle
                      ? "文件在位、且没被 agent 的原生开关停用的 skill。目录里有痕迹但用不上的（外来占位、悬空链接）不计。"
                      : "按标准目录中的 Skill 文件统计；实际加载状态由 Agent 自身配置决定。"
                  }
                >
                  {agent?.supportsNativeToggle ? "已启用" : "已安装"}{" "}
                  {enabledSkills.length} 个 skill
                </Badge>
                <Badge
                  variant="outline"
                  className={summaryPillClass}
                  title={tokenTitle(
                    enabledTokens,
                    agent?.supportsNativeToggle
                      ? "当前已启用的 skill"
                      : "当前已安装的 skill",
                    driftedCount,
                  )}
                >
                  ≈ {formatTokens(enabledTokens.total)} tokens
                </Badge>
                {agent && !agent.supportsNativeToggle && (
                  <HelpPopover label={`${agent.displayName} Skill 使用说明`}>
                    {agent.displayName} 本版支持 Skill
                    安装、托管和分组部署，暂不读取或修改原生启停配置。
                    分组停用只撤回 Studio 部署的副本，手动安装的 Skill
                    始终保留。 修改后请在 Agent 中重新加载 Skill 或重启会话。
                  </HelpPopover>
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
                          activeGroups[a.id]?.groupId === g.id,
                      )
                      .map((a) => ({
                        agent: a,
                        managed: managedIds.has(a.id),
                        ...groupStatus(g, a.id),
                      }));
                    const activeStatuses = statuses.filter((s) => s.active);
                    // 是否在用以配置里的全部启用记录为准，不只看显示出来的 Agent
                    const inUse = Object.values(activeGroups).some(
                      (v) => v.groupId === g.id,
                    );
                    const unavailable = activeStatuses.some(
                      (s) => s.unavailable,
                    );
                    const changed = activeStatuses.some((s) => s.changed);
                    const reapplyTargets = activeStatuses.filter(
                      (s) => s.managed && (s.changed || s.unavailable),
                    );
                    return (
                      <SortableGroupCard
                        key={g.id}
                        group={g}
                        disabled={reorder.isPending}
                        className={
                          inUse
                            ? "border-emerald-500/40 bg-card bg-gradient-to-r from-emerald-500/10 to-transparent shadow-sm shadow-emerald-500/5 hover:border-emerald-500/60 hover:bg-card dark:from-emerald-500/15"
                            : undefined
                        }
                      >
                        <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-muted">
                          <Layers className="h-5 w-5" />
                        </div>
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
                            {inUse && (
                              <Badge
                                variant={unavailable ? "warning" : "success"}
                                title={activeStatuses
                                  .map((s) => s.agent.displayName)
                                  .join("、")}
                              >
                                {unavailable ? "需要检查" : "使用中"}
                              </Badge>
                            )}
                            {changed && (
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
                                canActivate: managed && g.skillIds.length > 0,
                              };
                            },
                          )}
                          onToggle={(agentId, on) =>
                            activate.mutate({
                              kind: on ? "enable" : "disable",
                              agentIds: [agentId],
                              groupId: on ? g.id : null,
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
                                      agentIds: reapplyTargets.map(
                                        (t) => t.agent.id,
                                      ),
                                      groupId: g.id,
                                    }),
                                }
                              : undefined
                          }
                        />
                        <RowActions
                          busy={remove.isPending && deleting?.id === g.id}
                        >
                          <Button
                            variant="ghost"
                            size="icon"
                            title="编辑分组"
                            aria-label={`编辑 ${g.name}`}
                            onClick={() => edit(g)}
                          >
                            <Pencil className="h-4 w-4" />
                          </Button>
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
                      </SortableGroupCard>
                    );
                  })}
              </ListContainer>
            </SortableContext>
          </DndContext>
        </TabsContent>
        {["installed", "manual"].map((value) => (
          <TabsContent
            key={value}
            value={value}
            className="min-h-0 flex-1 overflow-y-auto"
          >
            {agent ? (
              <AgentSkills
                agentId={agent.id}
                searchQuery={query}
                manualOnly={value === "manual"}
                addOpen={addingSkill}
                onAddOpenChange={setAddingSkill}
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
  className,
  children,
}: {
  group: Group;
  disabled: boolean;
  className?: string;
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
      >
        <button
          ref={setActivatorNodeRef}
          type="button"
          {...attributes}
          {...listeners}
          aria-label={`拖拽排序 ${group.name}`}
          title="拖拽排序（也可按空格后用方向键移动）"
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
