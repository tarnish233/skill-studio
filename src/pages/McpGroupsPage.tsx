import { useMemo, useRef, useState } from "react";
import { usePageReset } from "@/components/common/PageReset";
import { useQuery } from "@tanstack/react-query";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import {
  DetailTransition,
  useDetailAppear,
} from "@/components/common/DetailTransition";
import {
  GroupAgentFilter,
  groupsInUse,
  useGroupAgentView,
} from "@/components/common/GroupAgentView";
import {
  NavigationGuard,
  useNavigationGuard,
} from "@/components/common/NavigationGuard";
import { PageTools } from "@/components/common/PageTools";
import { SourceMark } from "@/components/common/SourceMark";
import { SummaryStat } from "@/components/common/SummaryBar";
import { GroupBoard } from "@/components/groups/GroupBoard";
import { queryKeys } from "@/lib/queryKeys";
import { GroupCard } from "@/components/groups/GroupCard";
import {
  GroupDetail,
  type GroupDraft,
  type GroupSaveResult,
} from "@/components/groups/GroupDetail";
import {
  groupCardState,
  type AgentGroupStatus,
} from "@/components/groups/groupCardState";
import {
  sortedBy,
  useGroupReorder,
  useGroupRunner,
} from "@/components/groups/useGroupActions";
import { McpAgentNote } from "@/components/mcp/McpAgentNote";
import type { ChecklistItem } from "@/components/projects/MemberChecklist";
import { useAgents, useSettings } from "@/hooks/useData";
import { agentDetection } from "@/lib/agents";
import { mcpRequest } from "@/lib/api/mcp";
import {
  managementApi,
  MCP_STATUS_STALE_TIME,
  MCP_STATUS_POLL_INTERVAL,
  rows,
  entrySnapshot,
  type ManagementStatus,
  type McpGroup,
  type McpUntouched,
} from "@/lib/api/mcpManagement";
import { untouchedMcpNote } from "@/components/groups/untouched";
import {
  mcpAgentId,
  mcpAgentName,
  mcpAppId,
  supportsMcp,
} from "@/lib/mcpAgents";
import { sameMembers } from "@/lib/utils";
import { mcpTarget } from "@/lib/mcpDisplay";

const STATUS_KEY = queryKeys.mcpStatus;
/** 启停后的提示：MCP 要在 Agent 里重新加载才生效 */
const RELOAD = "请在 Agent 中重新加载 MCP";

/**
 * 分组不归属某个 Agent：名称和成员只保存一次，是否对某个 Agent 生效完全
 * 由卡片上的 Agent 图标独立决定，同一分组可以同时是多个 Agent 的当前组合。
 * 部署到 Agent 只通过分组；顶部胶囊可切到单个 Agent 的视角。
 * 页面骨架、卡片和整页编辑与 Skill 分组共用（见 `components/groups`）：点卡片或「新建分组」
 * 在页内展开，保存即写入正在使用这个分组的 Agent。
 */
export function McpGroupsPage() {
  return (
    <NavigationGuard>
      <McpGroups />
    </NavigationGuard>
  );
}
function McpGroups() {
  const requestNavigation = useNavigationGuard();
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
    queryKey: STATUS_KEY,
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
  const activeIds = (mcpId: string) =>
    (activeGroups[mcpId] ?? []).map((g) => g.groupId);
  const inUse = groupsInUse(activeGroups);
  const usedGroups = groups.filter((g) => inUse.has(g.id)).length;
  // 单个 Agent 视角的统计：用户全局配置里已启用的 MCP（与原「已配置 MCP」同口径）
  const enabledMcp = viewMcpId
    ? catalog.filter((r) =>
        r.sources.some(
          (s) => s.agent === viewMcpId && s.scope === "用户全局" && s.enabled,
        ),
      ).length
    : 0;
  const [query, setQuery] = useState("");
  /** 展开的整页编辑：id 为 null 是新建；key 区分每次打开，新建保存后换成真实 id 也不重新挂载 */
  const [detail, setDetail] = useState<{
    key: number;
    id: string | null;
  } | null>(null);
  const sessions = useRef(0);
  usePageReset(() => setDetail(null));
  const appear = useDetailAppear();
  const [deleting, setDeleting] = useState<McpGroup | null>(null);
  const { busy, run } = useGroupRunner(() => status.refetch());
  const reorder = useGroupReorder<ManagementStatus>({
    queryKey: STATUS_KEY,
    reorder: (data, ids) => ({
      ...data,
      groups: sortedBy(data.groups ?? [], ids),
    }),
    save: (ids) => mcpRequest("reorderGroups", { ids }),
  });
  const entryName = (id: string) =>
    catalog.find((r) => r.entry.id === id)?.entry.name;

  /**
   * 一个 MCP 分组对某个（MCP 后端）Agent id 当前是什么状态：启用记录是否跟得上分组的
   * 最新成员与定义（changed），以及后端核对出的、这个分组自己的部署问题（issue）。
   */
  function agentStatus(g: McpGroup, mcpId: string) {
    const current = (activeGroups[mcpId] ?? []).find(
      (record) => record.groupId === g.id,
    );
    if (!current) return { active: false, changed: false };
    const changed =
      !sameMembers(
        current.entries.map((e) => e.id),
        g.entryIds,
      ) ||
      current.entries.some((e) => {
        const found = catalog.find((r) => r.entry.id === e.id)?.entry;
        return !found || entrySnapshot(found) !== entrySnapshot(e);
      });
    return {
      active: true,
      changed,
      issue: status.data?.groupIssues?.[mcpId]?.[g.id],
    };
  }

  const keyword = query.trim().toLowerCase();
  const visible = groups.filter((g) =>
    `${g.name} ${g.entryIds.map((id) => entryName(id) ?? "").join(" ")}`
      .toLowerCase()
      .includes(keyword),
  );
  const open = (id: string | null) =>
    requestNavigation(() => setDetail({ key: ++sessions.current, id }));
  const options: ChecklistItem[] = catalog.map((row) => {
    const target = mcpTarget(row.entry.definition);
    const kind = row.managed
      ? row.entry.mode === "direct"
        ? "Agent 直连"
        : "Studio 网关"
      : "原有配置";
    return {
      id: row.entry.id,
      name: row.entry.name,
      detail: [kind, target].filter(Boolean).join(" · "),
      title: target || undefined,
      marks: [...new Set(row.sources.map((s) => s.agent))].map((agent) => (
        <SourceMark
          key={agent}
          id={mcpAppId(agent)}
          label={mcpAgentName(agent)}
        />
      )),
      disabledReason:
        !row.managed && row.sources.some((s) => s.gateway)
          ? "指向 Studio 网关的配置不能作为成员，请先在 MCP Hub 添加独立配置"
          : undefined,
    };
  });
  /** 卡片和整页编辑的标题栏共用：已关闭管理但仍在用的 Agent 保留一个只能停用的图标 */
  function cardState(g: McpGroup) {
    const statuses: AgentGroupStatus[] = mcpAgents
      .filter(
        (a) =>
          managedIds.has(a.id) ||
          (activeGroups[mcpAgentId(a.id)] ?? []).some(
            (v) => v.groupId === g.id,
          ),
      )
      .map((a) => {
        const detection = agentDetection(a, {
          isDetecting,
          detectionFailed,
        });
        return {
          agentId: a.id,
          label: a.displayName,
          managed: managedIds.has(a.id),
          note: detection.note,
          dimmed: detection.notInstalled,
          ...agentStatus(g, mcpAgentId(a.id)),
        };
      });
    return groupCardState({
      statuses,
      used: inUse.has(g.id),
      memberCount: g.entryIds.length,
      missingMembers: g.entryIds.some((id) => !entryName(id)),
      viewAgentId: viewAgent?.id,
    });
  }
  /** 只启停这一个分组：这个 Agent 叠加的其他分组由后端按最新状态保留，不会撤掉别处刚做的启用 */
  const toggle = (g: McpGroup, agentId: string, on: boolean) =>
    void run(
      () =>
        mcpRequest<McpUntouched[]>("setGroup", {
          agent: mcpAgentId(agentId),
          groupId: g.id,
          enabled: on,
        }),
      {
        success: `${on ? "分组已启用" : "分组已停用"}，${RELOAD}`,
        notice: untouchedMcpNote,
      },
    );
  /** 按最新成员重新写入这个 Agent 叠加的全部分组 */
  const reapplyAgent = async (agentId: string) =>
    (await mcpRequest<McpUntouched[] | null>("reapplyGroups", {
      agent: mcpAgentId(agentId),
    })) ?? [];
  const reapply = (agentIds: string[]) =>
    void run(
      async () => {
        // 依次重新应用：遇到第一个失败就停下报错
        const untouched: McpUntouched[] = [];
        for (const agentId of agentIds)
          untouched.push(...(await reapplyAgent(agentId)));
        return untouched;
      },
      { success: `修改已应用，${RELOAD}`, notice: untouchedMcpNote },
    );
  /**
   * 保存名称和成员，再依次重新写入正在使用这个分组的 Agent（与项目详情一致）。
   * 分组已经存下、写入某个 Agent 失败时，修改不丢：卡片和标题栏会显示「有待应用修改」。
   * `opened` 是打开编辑（或上次保存）时的样子，别处改过时后端拒绝保存，不覆盖。
   */
  async function save(
    id: string | null,
    draft: GroupDraft,
    opened: GroupDraft | null,
  ): Promise<GroupSaveResult> {
    const stored = groups.find((g) => g.id === id);
    const group: McpGroup = {
      ...(stored ?? { id: crypto.randomUUID(), sortOrder: groups.length }),
      name: draft.name,
      entryIds: draft.memberIds,
    };
    const targets = stored
      ? agents.filter((a) => activeIds(mcpAgentId(a.id)).includes(stored.id))
      : [];
    const outcome: { saved: boolean; error?: string } = { saved: false };
    await run(
      async () => {
        await mcpRequest("saveGroup", {
          group,
          // 选中的原有配置只记下引用，不托管、不改动
          imports: catalog
            .filter((r) => !r.managed && group.entryIds.includes(r.entry.id))
            .map((r) => ({
              id: r.entry.id,
              definition: r.entry.definition,
              sourceIds: r.sources.map((source) => source.id),
            })),
          expected:
            stored && opened
              ? { name: opened.name, entryIds: opened.memberIds }
              : null,
        });
        outcome.saved = true;
        const untouched: McpUntouched[] = [];
        for (const agent of targets) {
          try {
            untouched.push(...(await reapplyAgent(agent.id)));
          } catch (e) {
            throw new Error(
              `分组已保存，写入 ${agent.displayName} 时失败：${e instanceof Error ? e.message : String(e)}`,
            );
          }
        }
        return untouched;
      },
      {
        success: targets.length
          ? `已保存并写入 ${targets.map((a) => a.displayName).join("、")}，${RELOAD}`
          : "分组已保存，可在卡片上为需要的 Agent 启用",
        notice: untouchedMcpNote,
        onError: (message) => {
          outcome.error = message;
        },
      },
    );
    if (outcome.saved && !stored) setDetail((d) => d && { ...d, id: group.id });
    return {
      saved: outcome.saved,
      error:
        outcome.error && !outcome.saved
          ? `保存失败，修改已保留：${outcome.error}`
          : outcome.error,
    };
  }

  const tools = (
    <PageTools
      query={query}
      onQueryChange={(v) =>
        requestNavigation(() => {
          setQuery(v);
          setDetail(null);
        })
      }
      placeholder="搜索分组…"
      createLabel="新建分组"
      onCreate={() => open(null)}
    />
  );
  const deleteDialog = (
    <ConfirmDialog
      open={!!deleting}
      onOpenChange={(open) => {
        if (!open) setDeleting(null);
      }}
      title={`删除分组「${deleting?.name ?? ""}」`}
      description="只删除分组记录，不删除 MCP Hub 中的服务。项目里绑定的这个分组会一并移出；已有 Agent 启用时需先在项目里停用。"
      confirmText="删除"
      pending={busy}
      onConfirm={() =>
        deleting &&
        void run(() => mcpRequest("removeGroup", { id: deleting.id }), {
          success: "分组已删除",
          onDone: () => setDeleting(null),
        })
      }
    />
  );

  const editing = detail?.id
    ? groups.find((g) => g.id === detail.id)
    : undefined;
  if (detail && (detail.id === null || editing)) {
    // 新建的分组还没有启用记录：标题栏的 Agent 图标全部未启用、先保存才能用
    const current = editing ?? {
      id: "",
      name: "",
      entryIds: [],
      sortOrder: groups.length,
    };
    return (
      <>
        {tools}
        <DetailTransition key={detail.key} appear={appear}>
          <GroupDetail
            group={
              editing
                ? { name: editing.name, memberIds: editing.entryIds }
                : null
            }
            noun="MCP"
            hub="MCP Hub"
            searchLabel="搜索 Hub MCP"
            options={options}
            emptyHub="MCP Hub 暂无服务，请先添加 MCP。"
            note="选中的原有配置只记下引用，不会托管或修改。"
            describe={(ids) => `${ids.length} 个 MCP`}
            state={cardState(current)}
            writesTo={agents
              .filter(
                (a) =>
                  editing && activeIds(mcpAgentId(a.id)).includes(editing.id),
              )
              .map((a) => a.displayName)}
            busy={busy}
            loading={status.isPending || !!status.error}
            onToggle={(agentId, on) => toggle(current, agentId, on)}
            onReapply={reapply}
            onBack={() => requestNavigation(() => setDetail(null))}
            onSave={(draft, opened) => save(detail.id, draft, opened)}
          />
        </DetailTransition>
        {deleteDialog}
      </>
    );
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {tools}
      <DetailTransition appear={appear}>
        <GroupBoard
          showTools={false}
          query={query}
          onQueryChange={setQuery}
          onCreate={() => open(null)}
          loading={status.isPending}
          error={status.error}
          ids={groups.map((g) => g.id)}
          visible={visible.length}
          reorderDisabled={busy || reorder.isPending}
          onReorder={(ids) => reorder.mutate(ids)}
          noun="MCP"
          hub="MCP Hub"
          summary={
            <GroupAgentFilter
              total={groups.length}
              agents={agents}
              current={(agentId) => activeIds(mcpAgentId(agentId)).length}
              selected={viewAgent?.id ?? null}
              onSelect={setViewAgentId}
            >
              {viewAgent && viewMcpId ? (
                <>
                  <SummaryStat
                    title={`已启用：${viewAgent.displayName} 用户全局配置里已启用的 MCP，共 ${enabledMcp} 个`}
                  >
                    已启用 {enabledMcp}
                  </SummaryStat>
                  <McpAgentNote agent={viewMcpId} compact />
                </>
              ) : (
                // 总数已在「全部」上；这里给出在用的分组数，和卡片上的「使用中」同一口径
                <SummaryStat
                  title={`已启用：至少有一个 Agent 正在使用的分组，共 ${usedGroups} 个`}
                >
                  已启用 {usedGroups}
                </SummaryStat>
              )}
            </GroupAgentFilter>
          }
        >
          {visible.map((g) => {
            return (
              <GroupCard
                key={g.id}
                id={g.id}
                name={g.name}
                noun="MCP"
                memberCount={g.entryIds.length}
                members={g.entryIds.map(
                  (id) => entryName(id) ?? `已缺失：${id}`,
                )}
                state={cardState(g)}
                viewAgent={
                  viewAgent && {
                    id: viewAgent.id,
                    label: viewAgent.displayName,
                  }
                }
                dragDisabled={busy || reorder.isPending || !!viewAgent}
                dragHint={viewAgent ? "在「全部」视角下拖动排序" : undefined}
                busy={busy}
                deleting={busy && deleting?.id === g.id}
                onOpen={() => open(g.id)}
                onDelete={() => setDeleting(g)}
                onToggle={(agentId, on) => toggle(g, agentId, on)}
                onReapply={reapply}
              />
            );
          })}
        </GroupBoard>
      </DetailTransition>
      {deleteDialog}
    </div>
  );
}
