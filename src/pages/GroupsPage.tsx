import { useMemo, useRef, useState } from "react";
import { usePageReset } from "@/components/common/PageReset";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { queryKeys, NAVIGATION_STALE_TIME } from "@/lib/queryKeys";
import { groupsApi, settingsApi } from "@/lib/api";
import {
  useAgents,
  useGroups,
  useProjects,
  useSettings,
  useSkills,
} from "@/hooks/useData";
import { Badge } from "@/components/ui/badge";
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
import { HelpPopover } from "@/components/common/HelpPopover";
import {
  NavigationGuard,
  useNavigationGuard,
} from "@/components/common/NavigationGuard";
import { PageTools } from "@/components/common/PageTools";
import { SOURCE_LABELS, SourceMark } from "@/components/common/SourceMark";
import { SummaryStat } from "@/components/common/SummaryBar";
import { GroupBoard } from "@/components/groups/GroupBoard";
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
import type { ChecklistItem } from "@/components/projects/MemberChecklist";
import {
  sortedBy,
  useGroupReorder,
  useGroupRunner,
} from "@/components/groups/useGroupActions";
import { agentDetection } from "@/lib/agents";
import { hubSourceIds } from "@/lib/hubCatalog";
import { STATUS_LABEL } from "@/lib/linkReport";
import { isCopyDrifted, isRegistered, needsManualFix } from "@/lib/linkStatus";
import { formatTokens, sumTokens, tokenIndex, tokenTitle } from "@/lib/tokens";
import { sameMembers } from "@/lib/utils";
import type { Group, UntouchedCopy } from "@/types";
import { untouchedSkillNote } from "@/components/groups/untouched";

/** 启停后的提示：skill 要在 Agent 里重新加载或新开会话才生效 */
const RELOAD = "请在 Agent 中重新加载 skill 或新开会话";

/**
 * 分组不归属某个 Agent：名称和成员只保存一次，是否对某个 Agent 生效完全
 * 由卡片上的 Agent 图标独立决定，同一分组可以同时是多个 Agent 的当前组合。
 * 部署到 Agent 只通过分组；顶部胶囊可切到单个 Agent 的视角。
 * 页面骨架、卡片和整页编辑与 MCP 分组共用（见 `components/groups`）：点卡片或「新建分组」
 * 在页内展开，保存即写入正在使用这个分组的 Agent。
 */
export function GroupsPage() {
  return (
    <NavigationGuard>
      <GroupsContent />
    </NavigationGuard>
  );
}
function GroupsContent() {
  const requestNavigation = useNavigationGuard();
  const { data: allAgents = [], isDetecting, detectionFailed } = useAgents();
  const { data: liveSettings } = useSettings();
  const groupsQuery = useGroups();
  const { data: liveSkills = [] } = useSkills();
  const { data: projects = [] } = useProjects();
  const configQuery = useQuery({
    queryKey: queryKeys.config,
    queryFn: settingsApi.getConfig,
    staleTime: NAVIGATION_STALE_TIME,
  });
  const liveGroups = groupsQuery.data;
  const liveConfig = configQuery.data;
  // 启停期间配置和文件扫描各自刷新：保持操作开始时的一致画面，两边都刷新完再换
  const [operationView, setOperationView] = useState<{
    settings: typeof liveSettings;
    groups: typeof liveGroups;
    skills: typeof liveSkills;
    config: typeof liveConfig;
  } | null>(null);
  const {
    settings,
    groups: rawGroups = [],
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
  /** 展开的整页编辑：id 为 null 是新建；key 区分每次打开，新建保存后换成真实 id 也不重新挂载 */
  const [detail, setDetail] = useState<{
    key: number;
    id: string | null;
  } | null>(null);
  const sessions = useRef(0);
  usePageReset(() => setDetail(null));
  const appear = useDetailAppear();
  const [deleting, setDeleting] = useState<Group | null>(null);
  const [query, setQuery] = useState("");
  const managedIds = new Set(agents.map((a) => a.id));
  const activeGroups = config?.activeGroups ?? {};
  const [viewAgentId, setViewAgentId] = useGroupAgentView(
    "skills",
    agents.map((a) => a.id),
  );
  const viewAgent = agents.find((a) => a.id === viewAgentId);
  const activeIds = (agentId: string) =>
    (activeGroups[agentId] ?? []).map((g) => g.groupId);
  const inUse = groupsInUse(activeGroups);
  const usedGroups = groups.filter((g) => inUse.has(g.id)).length;
  // 索引跟着 skills 走：与 ProjectsPage 同一口径，别在每次重渲染时重建一遍
  const tokens = useMemo(() => tokenIndex(skills), [skills]);
  const skillName = (id: string) => skills.find((s) => s.id === id)?.name;
  const agentName = (id: string) =>
    allAgents.find((a) => a.id === id)?.displayName ?? id;

  const { busy, run } = useGroupRunner(() => client.invalidateQueries());
  const reorder = useGroupReorder<Group[]>({
    queryKey: queryKeys.groups,
    reorder: sortedBy,
    save: groupsApi.reorder,
  });
  /** 启停与重新应用：操作期间保持开始时的画面；提示里带上原样留下的副本 */
  const switching = (action: () => Promise<UntouchedCopy[]>, success: string) =>
    run(action, {
      success,
      notice: untouchedSkillNote,
      onStart: () =>
        setOperationView({ settings, groups: rawGroups, skills, config }),
      onSettled: () => setOperationView(null),
    });

  /** 卡片和整页编辑的标题栏共用：各 Agent 的状态，已关闭管理但仍在用的保留一个只能停用的图标 */
  function cardState(g: Group) {
    const statuses: AgentGroupStatus[] = allAgents
      .filter(
        (a) =>
          managedIds.has(a.id) ||
          (activeGroups[a.id] ?? []).some((v) => v.groupId === g.id),
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
          ...status(g, a.id),
        };
      });
    return groupCardState({
      statuses,
      used: inUse.has(g.id),
      memberCount: g.skillIds.length,
      missingMembers: g.skillIds.some((id) => !skillName(id)),
      viewAgentId: viewAgent?.id,
    });
  }
  // 只传这一个分组和 Agent：后端按最新的启用记录计算，不会撤掉别处（另一个窗口、
  // 命令行）刚启用、这里还没刷新出来的分组
  const toggle = (g: Group, agentId: string, on: boolean) =>
    void switching(
      () => groupsApi.enable(agentId, g.id, on),
      `${on ? "分组已启用" : "分组已停用"}，${RELOAD}`,
    );
  const reapply = (agentIds: string[]) =>
    void switching(async () => {
      // 依次写：遇到第一个失败就停下报错，已经成功的保持生效
      const untouched: UntouchedCopy[] = [];
      for (const agentId of agentIds)
        untouched.push(...((await groupsApi.reapply(agentId)) ?? []));
      return untouched;
    }, `修改已应用，${RELOAD}`);
  /**
   * 保存名称和成员，再依次重新写入正在使用这个分组的 Agent（与项目详情一致）。
   * 分组已经存下、写入某个 Agent 失败时，修改不丢：卡片和标题栏会显示「有待应用修改」。
   */
  async function save(
    id: string | null,
    draft: GroupDraft,
    opened: GroupDraft | null,
  ): Promise<GroupSaveResult> {
    const targets = id
      ? agents.filter((a) => activeIds(a.id).includes(id))
      : [];
    const outcome: { id: string | null; error?: string } = { id: null };
    await run(
      async () => {
        const group = await groupsApi.save(
          id,
          draft.name,
          draft.memberIds,
          opened && { name: opened.name, skillIds: opened.memberIds },
        );
        outcome.id = group.id;
        const untouched: UntouchedCopy[] = [];
        for (const agent of targets) {
          try {
            untouched.push(...((await groupsApi.reapply(agent.id)) ?? []));
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
        notice: untouchedSkillNote,
        // 写入 Agent 期间与启停一样，保持开始时的一致画面
        onStart: () => {
          if (targets.length)
            setOperationView({ settings, groups: rawGroups, skills, config });
        },
        onSettled: () => setOperationView(null),
        onError: (message) => {
          outcome.error = message;
        },
      },
    );
    const saved = outcome.id;
    if (saved && saved !== id) setDetail((d) => d && { ...d, id: saved });
    return {
      saved: !!saved,
      error:
        outcome.error && !saved
          ? `保存失败，修改已保留：${outcome.error}`
          : outcome.error,
    };
  }

  /**
   * 一个分组对某个 Agent 当前是什么状态：
   * - changed：分组定义已变化（成员、保留策略）或某个成员副本已过期，与「有待应用修改」同义
   * - issue：启用记录里的某个成员在这个 Agent 上缺失、被停用或需要人工处理，与「需要检查」同义
   */
  function status(g: Group, agentId: string) {
    const current = (activeGroups[agentId] ?? []).find(
      (record) => record.groupId === g.id,
    );
    if (!current) return { active: false, changed: false };
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
    const problems = current.skillIds.flatMap((id) => {
      const name = skillName(id);
      const state = skills.find((s) => s.id === id)?.agents[agentId];
      if (!name) return [`${id} 已缺失`];
      if (!state) return [`${name} 在这个 Agent 上不可用`];
      if (state.disabled) return [`${name} 已被手动停用`];
      // 本地改过的副本 Studio 不会再覆盖或删除，需要用户自己处理
      if (state.status === "copyModified") return [`${name} 的副本已被修改`];
      if (needsManualFix(state.status))
        return [`${name}：${STATUS_LABEL[state.status]}`];
      return [];
    });
    return {
      active: true,
      changed,
      issue: problems.length ? problems.join("；") : undefined,
    };
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

  const keyword = query.trim().toLowerCase();
  const visible = groups.filter((g) =>
    `${g.name} ${g.skillIds.map((id) => skillName(id) ?? "").join(" ")}`
      .toLowerCase()
      .includes(keyword),
  );
  const open = (id: string | null) =>
    requestNavigation(() => setDetail({ key: ++sessions.current, id }));
  const options: ChecklistItem[] = skills.map((s) => ({
    id: s.id,
    name: s.name,
    detail: s.description ?? undefined,
    title: s.sourcePath,
    marks: hubSourceIds({
      key: s.id,
      skill: s,
      sources: [s],
      unavailable: false,
    }).map((id) => (
      <SourceMark key={id} id={id} label={SOURCE_LABELS[id] ?? agentName(id)} />
    )),
  }));
  const boundProjects = deleting
    ? projects.filter((p) => p.groupIds.includes(deleting.id))
    : [];

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
      description={`只删除分组记录，不删除 Skill Hub 中的 skill。${
        boundProjects.length
          ? `项目${boundProjects.map((p) => `「${p.name}」`).join("")}绑定了这个分组，删除后会从这些项目中移除。`
          : ""
      }`}
      confirmText="删除"
      pending={busy}
      onConfirm={() =>
        deleting &&
        void run(() => groupsApi.remove(deleting.id), {
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
      sortOrder: groups.length,
      skillIds: [],
    };
    return (
      <>
        {tools}
        <DetailTransition key={detail.key} appear={appear}>
          <GroupDetail
            group={
              editing
                ? { name: editing.name, memberIds: editing.skillIds }
                : null
            }
            noun="skill"
            hub="Skill Hub"
            searchLabel="搜索 Hub skill"
            options={options}
            emptyHub="Skill Hub 暂无 skill，请先安装或导入。"
            describe={(ids) => {
              const total = sumTokens(tokens, ids).total;
              return `${ids.length} 个 skill${total > 0 ? ` · ≈ ${formatTokens(total)} tokens` : ""}`;
            }}
            state={cardState(current)}
            writesTo={agents
              .filter((a) => editing && activeIds(a.id).includes(editing.id))
              .map((a) => a.displayName)}
            busy={busy}
            loading={groupsQuery.isPending || configQuery.isPending}
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
          loading={groupsQuery.isPending || configQuery.isPending}
          error={groupsQuery.error ?? configQuery.error}
          ids={groups.map((g) => g.id)}
          visible={visible.length}
          reorderDisabled={busy || reorder.isPending}
          onReorder={(ids) => reorder.mutate(ids)}
          noun="skill"
          hub="Skill Hub"
          summary={
            <GroupAgentFilter
              total={groups.length}
              agents={agents}
              current={(agentId) => activeIds(agentId).length}
              selected={viewAgent?.id ?? null}
              onSelect={setViewAgentId}
            >
              {viewAgent ? (
                <>
                  <SummaryStat
                    title={
                      viewAgent.supportsNativeToggle
                        ? `已启用：文件在位、且没被 agent 的原生开关停用的 skill，共 ${enabledSkills.length} 个。目录里有痕迹但用不上的（外来占位、悬空链接）不计。`
                        : `已安装：按标准目录中的 Skill 文件统计，共 ${enabledSkills.length} 个；实际加载状态由 Agent 自身配置决定。`
                    }
                  >
                    {viewAgent.supportsNativeToggle ? "已启用" : "已安装"}{" "}
                    {enabledSkills.length}
                  </SummaryStat>
                  <SummaryStat
                    title={tokenTitle(
                      enabledTokens,
                      viewAgent.supportsNativeToggle
                        ? "当前已启用的 skill"
                        : "当前已安装的 skill",
                      driftedCount,
                    )}
                  >
                    合计 ≈ {formatTokens(enabledTokens.total)} tokens
                  </SummaryStat>
                  {!viewAgent.supportsNativeToggle && (
                    <HelpPopover
                      label={`${viewAgent.displayName} Skill 使用说明`}
                    >
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
            const rowTokens = sumTokens(tokens, g.skillIds);
            return (
              <GroupCard
                key={g.id}
                id={g.id}
                name={g.name}
                noun="skill"
                memberCount={g.skillIds.length}
                members={g.skillIds.map(
                  (id) => skillName(id) ?? `已缺失：${id}`,
                )}
                extraBadges={
                  rowTokens.total > 0 && (
                    <Badge
                      variant="outline"
                      title={tokenTitle(rowTokens, "组内 skill")}
                    >
                      ≈ {formatTokens(rowTokens.total)} tokens
                    </Badge>
                  )
                }
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
