import { useMemo, useState } from "react";
import { usePageReset } from "@/components/common/PageReset";
import { useQuery } from "@tanstack/react-query";
import { FolderOpen } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import {
  DetailTransition,
  useDetailAppear,
} from "@/components/common/DetailTransition";
import {
  GroupAgentFilter,
  useGroupAgentView,
} from "@/components/common/GroupAgentView";
import {
  NavigationGuard,
  useNavigationGuard,
} from "@/components/common/NavigationGuard";
import { PageTools } from "@/components/common/PageTools";
import { SummaryStat } from "@/components/common/SummaryBar";
import { GroupBoard } from "@/components/groups/GroupBoard";
import { GroupCard } from "@/components/groups/GroupCard";
import {
  groupCardState,
  type AgentGroupStatus,
  type GroupCardState,
} from "@/components/groups/groupCardState";
import { untouchedMcpNote } from "@/components/groups/untouched";
import {
  useGroupReorder,
  useGroupRunner,
} from "@/components/groups/useGroupActions";
import { McpProjectDetail } from "@/components/mcp/McpProjectDetail";
import { ProjectCreateDialog } from "@/components/projects/ProjectCreateDialog";
import { useAgents, useProjects, useSettings } from "@/hooks/useData";
import { agentDetection } from "@/lib/agents";
import { projectsApi, systemApi } from "@/lib/api";
import { mcpRequest } from "@/lib/api/mcp";
import {
  entrySnapshot,
  managementApi,
  MCP_STATUS_POLL_INTERVAL,
  MCP_STATUS_STALE_TIME,
  ownedBindings,
  rows,
  type McpProject,
  type McpUntouched,
} from "@/lib/api/mcpManagement";
import { inProject, mcpAgentId, supportsMcp } from "@/lib/mcpAgents";
import { queryKeys } from "@/lib/queryKeys";
import { sameMembers } from "@/lib/utils";
import {
  PROJECT_BOARD_LABELS,
  PROJECT_CARD_LABELS,
  PROJECT_REMOVAL,
} from "@/components/projects/projectLabels";
import type { ProjectBinding } from "@/types";

/** 启停后的提示：MCP 要在 Agent 里重新加载才生效 */
const RELOAD = "请在 Agent 中重新加载 MCP";
/** 各 Agent 的项目 MCP 配置（与后端 agents::project_files 一致） */
const PROJECT_FILES: Record<string, string> = {
  claude: ".mcp.json",
  codex: ".codex/config.toml",
  opencode: "opencode.json",
  pi: ".pi/mcp.json",
  grok: ".grok/config.toml",
};

export function McpProjectsPage() {
  return (
    <NavigationGuard>
      <McpProjectsContent />
    </NavigationGuard>
  );
}

/**
 * MCP 项目与 Skill 项目、分组同构：成员（绑定的 MCP 分组 + 单独选择的 MCP）只保存一次，
 * 每个 Agent 独立决定是否写进这个项目的配置。卡片、Agent 图标、需要检查 / 有待应用修改
 * 与分组共用（见 `components/groups`）。
 */
function McpProjectsContent() {
  const requestNavigation = useNavigationGuard();
  const projectsQuery = useProjects();
  const projects = useMemo(
    () => projectsQuery.data ?? [],
    [projectsQuery.data],
  );
  const status = useQuery({
    queryKey: queryKeys.mcpStatus,
    queryFn: managementApi.list,
    // Keep polling in the background without fetching again on every Agent switch.
    staleTime: MCP_STATUS_STALE_TIME,
    refetchInterval: MCP_STATUS_POLL_INTERVAL,
  });
  const { data: allAgents = [], isDetecting, detectionFailed } = useAgents();
  const { data: settings } = useSettings();
  // Skill 侧的 Agent 列表（含品牌图标），过滤到真正支持 MCP 的
  const mcpAgents = useMemo(
    () => allAgents.filter((a) => supportsMcp(a.id)),
    [allAgents],
  );
  const managedIds = new Set(
    mcpAgents
      .filter((a) => !settings?.disabledAgents?.includes(a.id))
      .map((a) => a.id),
  );
  const [viewAgentId, setViewAgentId] = useGroupAgentView(
    "mcp",
    [...managedIds],
    "projects",
  );
  const viewAgent = mcpAgents.find((a) => a.id === viewAgentId);
  const [query, setQuery] = useState("");
  const [detail, setDetail] = useState<string | null>(null);
  usePageReset(() => setDetail(null));
  const appear = useDetailAppear();
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<ProjectBinding | null>(null);
  const { busy, run } = useGroupRunner(() =>
    Promise.all([status.refetch(), projectsQuery.refetch()]),
  );
  const reorder = useGroupReorder<ProjectBinding[]>({
    queryKey: queryKeys.projects,
    reorder: (data, ids) =>
      ids.flatMap((id) => data.filter((p) => p.id === id)),
    save: projectsApi.reorder,
  });
  const catalog = rows(status.data);
  const groups = status.data?.groups ?? [];
  const record = (p: ProjectBinding): McpProject | undefined =>
    status.data?.projects?.[p.id];
  const entryName = (id: string) =>
    catalog.find((r) => r.entry.id === id)?.entry.name;
  /** 项目成员：单独选择的在前，再展开绑定的分组，去重 */
  const members = (p: ProjectBinding) => {
    const rec = record(p);
    return [
      ...new Set([
        ...(rec?.entryIds ?? []),
        ...(rec?.groupIds ?? []).flatMap(
          (id) => groups.find((g) => g.id === id)?.entryIds ?? [],
        ),
      ]),
    ];
  };
  const activeIds = (p: ProjectBinding) => Object.keys(record(p)?.active ?? {});

  /** 各 Agent 对这个项目的状态：卡片和详情标题栏共用 */
  function cardState(p: ProjectBinding): GroupCardState {
    const rec = record(p);
    const ids = members(p);
    const existing = ids.filter((id) => entryName(id));
    const statuses: AgentGroupStatus[] = mcpAgents
      .filter((a) => managedIds.has(a.id) || !!rec?.active?.[mcpAgentId(a.id)])
      .map((a) => {
        const mcpId = mcpAgentId(a.id);
        const deployed = rec?.active?.[mcpId];
        const detection = agentDetection(a, { isDetecting, detectionFailed });
        return {
          where:
            mcpId === "claude" && rec?.claudeLocal
              ? "~/.claude.json 里这个项目的配置（项目本地）"
              : PROJECT_FILES[mcpId],
          agentId: a.id,
          label: a.displayName,
          managed: managedIds.has(a.id),
          note: detection.note,
          dimmed: detection.notInstalled,
          active: !!deployed,
          changed:
            !!deployed &&
            (!sameMembers(
              deployed.entries.map((e) => e.id),
              existing,
            ) ||
              deployed.entries.some((e) => {
                const found = catalog.find((r) => r.entry.id === e.id)?.entry;
                return !found || entrySnapshot(found) !== entrySnapshot(e);
              })),
          issue: status.data?.projectIssues?.[p.id]?.[mcpId],
        };
      });
    return groupCardState({
      statuses,
      used: statuses.some((s) => s.active),
      memberCount: ids.length,
      missingMembers:
        existing.length < ids.length ||
        (rec?.groupIds ?? []).some((id) => !groups.some((g) => g.id === id)),
      viewAgentId: viewAgent?.id,
      labels: PROJECT_CARD_LABELS("MCP"),
    });
  }
  const activate = (p: ProjectBinding, agentId: string, enabled: boolean) =>
    mcpRequest<McpUntouched[] | null>("activateProject", {
      projectId: p.id,
      agent: mcpAgentId(agentId),
      enabled,
    });
  const toggle = (p: ProjectBinding, agentId: string, on: boolean) =>
    void run(() => activate(p, agentId, on), {
      success: `${on ? "项目已启用" : "项目已停用"}，${RELOAD}`,
      notice: untouchedMcpNote,
    });
  const reapply = (p: ProjectBinding, agentIds: string[]) =>
    void run(
      async () => {
        // 依次重新应用：遇到第一个失败就停下报错
        const untouched: McpUntouched[] = [];
        for (const agentId of agentIds)
          untouched.push(...((await activate(p, agentId, true)) ?? []));
        return untouched;
      },
      { success: `修改已应用，${RELOAD}`, notice: untouchedMcpNote },
    );

  const tools = (
    <PageTools
      query={query}
      onQueryChange={(v) =>
        requestNavigation(() => {
          setQuery(v);
          setDetail(null);
        })
      }
      placeholder="搜索项目…"
      createLabel="添加项目"
      onCreate={() =>
        requestNavigation(() => {
          setDetail(null);
          setCreating(true);
        })
      }
    />
  );
  const dialogs = (
    <>
      <ProjectCreateDialog
        open={creating}
        onOpenChange={setCreating}
        description="选一个项目根目录，之后选择只在这个项目里用的 MCP，再在卡片上为需要的 Agent 启用。"
      />
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(open) => !open && !busy && setDeleting(null)}
        title={`移除项目「${deleting?.name ?? ""}」`}
        confirmText="移除"
        pending={busy}
        description={
          <>
            只解除绑定，
            <span className="font-medium">项目里已有的配置文件不会被删除</span>
            —— 它们可能已经提交进 git 了。
          </>
        }
        onConfirm={() =>
          deleting &&
          void run(
            async () => {
              // 项目的 MCP 记录由后端一并清掉（service::projects::delete_project）
              await projectsApi.remove(deleting.id);
            },
            {
              success: "项目已移除（项目目录里的文件未变动）",
              onDone: () => setDeleting(null),
            },
          )
        }
      />
    </>
  );

  const active = projects.find((p) => p.id === detail);
  if (active && status.data)
    return (
      <>
        {tools}
        {/* 标题栏和内容一起过渡，与 Skill 项目详情一致 */}
        <DetailTransition key={active.id} appear={appear}>
          <McpProjectDetail
            project={active}
            status={status.data}
            refetch={() => status.refetch()}
            state={cardState(active)}
            busy={busy}
            onToggle={(agentId, on) => toggle(active, agentId, on)}
            onReapply={() =>
              reapply(active, cardState(active).reapply?.agentIds ?? [])
            }
            onBack={() => requestNavigation(() => setDetail(null))}
          />
        </DetailTransition>
        {dialogs}
      </>
    );

  const keyword = query.trim().toLowerCase();
  const visible = projects.filter((p) =>
    `${p.name} ${p.root}`.toLowerCase().includes(keyword),
  );
  // Studio 写进项目的接入（项目自己的部署 + Hub 写在项目里的），和项目里已有但没纳入管理的配置
  const owned = ownedBindings(status.data);
  const managedHere = [
    ...owned,
    ...catalog.flatMap((row) => row.entry.bindings),
  ].filter((b) => projects.some((p) => inProject(b, p)));
  const unmanaged = (p: ProjectBinding) =>
    catalog.flatMap((row) =>
      row.sources.filter(
        (source) =>
          inProject({ ...source, project: source.project ?? null }, p) &&
          !managedHere.some(
            (b) =>
              b.agent === source.agent &&
              b.path === source.path &&
              b.key === source.key &&
              (b.project ?? null) === (source.project ?? null),
          ),
      ),
    );
  const unmanagedCount = projects.reduce((n, p) => n + unmanaged(p).length, 0);
  const enabledProjects = projects.filter((p) => activeIds(p).length).length;
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {tools}
      <DetailTransition appear={appear}>
        <GroupBoard
          showTools={false}
          query={query}
          onQueryChange={setQuery}
          onCreate={() => setCreating(true)}
          loading={projectsQuery.isPending || status.isPending}
          error={projectsQuery.error ?? status.error}
          ids={projects.map((p) => p.id)}
          visible={visible.length}
          reorderDisabled={busy || reorder.isPending || !!keyword}
          onReorder={(ids) => reorder.mutate(ids)}
          noun="MCP"
          hub="MCP Hub"
          labels={PROJECT_BOARD_LABELS("MCP")}
          summary={
            <GroupAgentFilter
              noun="项目"
              total={projects.length}
              agents={mcpAgents.filter((a) => managedIds.has(a.id))}
              current={(agentId) =>
                projects.filter((p) =>
                  activeIds(p).includes(mcpAgentId(agentId)),
                ).length
              }
              selected={viewAgent?.id ?? null}
              onSelect={setViewAgentId}
            >
              <SummaryStat
                title={`托管接入：Studio 写进这些项目的接入，共 ${managedHere.length} 处，同一项目可以有多处`}
              >
                托管接入 {managedHere.length}
              </SummaryStat>
              <SummaryStat
                title={`未托管：项目里已有、但没纳入 Studio 管理的 MCP 配置，共 ${unmanagedCount} 处，可在项目详情里「纳入管理」`}
              >
                未托管 {unmanagedCount}
              </SummaryStat>
              <SummaryStat
                title={`已启用：至少有一个 Agent 已启用的项目，共 ${enabledProjects} 个`}
              >
                已启用 {enabledProjects}
              </SummaryStat>
            </GroupAgentFilter>
          }
        >
          {visible.map((p) => {
            const ids = members(p);
            const state = cardState(p);
            const skillsActive = !!p.deployments?.length;
            const locked = state.used || skillsActive;
            const others = unmanaged(p).length;
            return (
              <GroupCard
                key={p.id}
                id={p.id}
                name={p.name}
                noun="MCP"
                memberCount={ids.length}
                subtitle={
                  <p className="mt-1 truncate font-mono text-[11px] text-muted-foreground">
                    {p.root}
                  </p>
                }
                extraBadges={
                  !!others && (
                    <Badge
                      variant="outline"
                      title={`未托管：项目配置里已有、但没纳入 Studio 管理的 MCP，共 ${others} 处`}
                    >
                      未托管 {others}
                    </Badge>
                  )
                }
                actions={
                  <Button
                    variant="ghost"
                    size="icon"
                    title="打开项目目录"
                    aria-label={`打开 ${p.name} 目录`}
                    onClick={() => void systemApi.revealPath(p.root)}
                  >
                    <FolderOpen className="h-4 w-4" />
                  </Button>
                }
                removal={
                  locked && !state.used
                    ? {
                        ...PROJECT_REMOVAL,
                        blocked: "请先在 Skill 项目里停用所有 Agent",
                      }
                    : PROJECT_REMOVAL
                }
                state={{ ...state, used: locked }}
                viewAgent={
                  viewAgent && {
                    id: viewAgent.id,
                    label: viewAgent.displayName,
                  }
                }
                dragDisabled={
                  busy || reorder.isPending || !!viewAgent || !!keyword
                }
                dragHint={
                  viewAgent
                    ? "在「全部」视角下拖动排序"
                    : keyword
                      ? "清除搜索后拖动排序"
                      : undefined
                }
                busy={busy}
                deleting={busy && deleting?.id === p.id}
                onOpen={() => setDetail(p.id)}
                onDelete={() => setDeleting(p)}
                onToggle={(agentId, on) => toggle(p, agentId, on)}
                onReapply={(agentIds) => reapply(p, agentIds)}
              />
            );
          })}
        </GroupBoard>
      </DetailTransition>
      {dialogs}
    </div>
  );
}
