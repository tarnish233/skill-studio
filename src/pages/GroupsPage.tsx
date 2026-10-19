import { useMemo, useState } from "react";
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
  GroupAgentFilter,
  groupsInUse,
  useGroupAgentView,
} from "@/components/common/GroupAgentView";
import { HelpPopover } from "@/components/common/HelpPopover";
import { SOURCE_LABELS, SourceMark } from "@/components/common/SourceMark";
import { SummaryStat } from "@/components/common/SummaryBar";
import { GroupBoard } from "@/components/groups/GroupBoard";
import { GroupCard } from "@/components/groups/GroupCard";
import {
  GroupEditor,
  type GroupDraft,
  type MemberOption,
} from "@/components/groups/GroupEditor";
import {
  groupCardState,
  type AgentGroupStatus,
} from "@/components/groups/groupCardState";
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
 * 页面骨架、卡片和编辑弹窗与 MCP 分组共用（见 `components/groups`）。
 */
export function GroupsPage() {
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
  const [draft, setDraft] = useState<GroupDraft | null>(null);
  const [error, setError] = useState<string | null>(null);
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
  const edit = (g?: Group) => {
    setError(null);
    setDraft({
      id: g?.id ?? null,
      name: g?.name ?? "",
      memberIds: [...(g?.skillIds ?? [])],
      pinned: [...(g?.skillIds ?? [])],
    });
  };
  const options: MemberOption[] = skills.map((s) => ({
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

  return (
    <>
      <GroupBoard
        query={query}
        onQueryChange={setQuery}
        onCreate={() => edit()}
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
          // 已关闭管理的 Agent 不显示图标，但若它仍在用这个分组，保留一个只能停用的图标
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
          const rowTokens = sumTokens(tokens, g.skillIds);
          return (
            <GroupCard
              key={g.id}
              id={g.id}
              name={g.name}
              noun="skill"
              memberCount={g.skillIds.length}
              members={g.skillIds.map((id) => skillName(id) ?? `已缺失：${id}`)}
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
              state={groupCardState({
                statuses,
                used: inUse.has(g.id),
                memberCount: g.skillIds.length,
                missingMembers: g.skillIds.some((id) => !skillName(id)),
                viewAgentId: viewAgent?.id,
              })}
              viewAgent={
                viewAgent && { id: viewAgent.id, label: viewAgent.displayName }
              }
              dragDisabled={busy || reorder.isPending || !!viewAgent}
              dragHint={viewAgent ? "在「全部」视角下拖动排序" : undefined}
              busy={busy}
              deleting={busy && deleting?.id === g.id}
              onOpen={() => edit(g)}
              onDelete={() => setDeleting(g)}
              onToggle={(agentId, on) =>
                void switching(
                  () =>
                    groupsApi.activate(
                      agentId,
                      on
                        ? [...activeIds(agentId), g.id]
                        : activeIds(agentId).filter((id) => id !== g.id),
                    ),
                  `${on ? "分组已启用" : "分组已停用"}，${RELOAD}`,
                )
              }
              onReapply={(agentIds) =>
                void switching(async () => {
                  // 依次写：遇到第一个失败就停下报错，已经成功的保持生效
                  const untouched: UntouchedCopy[] = [];
                  for (const agentId of agentIds)
                    untouched.push(
                      ...((await groupsApi.activate(
                        agentId,
                        activeIds(agentId),
                      )) ?? []),
                    );
                  return untouched;
                }, `修改已应用，${RELOAD}`)
              }
            />
          );
        })}
      </GroupBoard>
      <GroupEditor
        draft={draft}
        onChange={setDraft}
        onCancel={() => setDraft(null)}
        onSave={() =>
          draft &&
          void run(
            () => groupsApi.save(draft.id, draft.name, draft.memberIds),
            {
              success: "分组已保存，可在卡片上为需要的 Agent 启用",
              onDone: () => setDraft(null),
              onError: setError,
            },
          )
        }
        pending={busy}
        error={error}
        noun="skill"
        hub="Skill Hub"
        searchLabel="搜索 Hub skill"
        options={options}
        emptyHub="Skill Hub 暂无 skill，请先安装或导入。"
        note="保存只记录成员，不会改动各 Agent 的 skill 目录。"
      />
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
    </>
  );
}
