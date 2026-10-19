import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderOpen, PackagePlus, Play, Square, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { AgentIcon } from "@/components/common/AgentIcon";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import {
  DetailTransition,
  useDetailAppear,
} from "@/components/common/DetailTransition";
import { GroupAgentToggles } from "@/components/common/GroupAgentToggles";
import {
  GroupAgentFilter,
  useGroupAgentView,
} from "@/components/common/GroupAgentView";
import { ListItemRow, RowActions } from "@/components/common/ListItemRow";
import {
  NavigationGuard,
  useNavigationGuard,
  useUnsavedProject,
} from "@/components/common/NavigationGuard";
import { PageTools } from "@/components/common/PageTools";
import { SkillBackups } from "@/components/common/SkillBackups";
import { useSkillPreview } from "@/components/common/SkillPreview";
import { SOURCE_LABELS, SourceMark } from "@/components/common/SourceMark";
import { SummaryStat } from "@/components/common/SummaryBar";
import { GroupBoard } from "@/components/groups/GroupBoard";
import { GroupCard } from "@/components/groups/GroupCard";
import {
  groupCardState,
  type AgentGroupStatus,
  type GroupCardState,
} from "@/components/groups/groupCardState";
import { untouchedSkillNote } from "@/components/groups/untouched";
import {
  useGroupReorder,
  useGroupRunner,
} from "@/components/groups/useGroupActions";
import { MemberChecklist } from "@/components/projects/MemberChecklist";
import {
  PROJECT_BOARD_LABELS,
  PROJECT_CARD_LABELS,
  PROJECT_REMOVAL,
} from "@/components/projects/projectLabels";
import { ProjectCreateDialog } from "@/components/projects/ProjectCreateDialog";
import { ProjectDetailHeader } from "@/components/projects/ProjectDetailHeader";
import {
  useAgents,
  useGroups,
  useProjects,
  useSettings,
  useSkills,
  useWriteProjectGitignore,
} from "@/hooks/useData";
import { agentDetection, projectSkillDirectory } from "@/lib/agents";
import { projectsApi, systemApi } from "@/lib/api";
import { mcpRequest } from "@/lib/api/mcp";
import {
  managementApi,
  MCP_STATUS_POLL_INTERVAL,
  MCP_STATUS_STALE_TIME,
} from "@/lib/api/mcpManagement";
import type { ProjectLocalSkill } from "@/lib/api/projects";
import { hubSourceIds } from "@/lib/hubCatalog";
import { queryKeys } from "@/lib/queryKeys";
import {
  formatTokens,
  projectSkillIds,
  sumTokens,
  tokenIndex,
  tokenTitle,
} from "@/lib/tokens";
import { sameMembers } from "@/lib/utils";
import type { LinkMode, LinkReport, ProjectBinding } from "@/types";

/** 启停后的提示：skill 要在 Agent 里重新加载或新开会话才生效 */
const RELOAD = "请在 Agent 中重新加载 skill 或新开会话";
/** 项目里这些目录不止一个 Agent 读：卡片提示里说明，按 Agent 启用做不到完全隔离 */
const SHARED_DIRS: Record<string, string> = {
  ".agents/skills": "共享目录，Pi、OpenCode、Grok Build 也会读取",
  ".claude/skills": "OpenCode、Grok Build 也会读取（Claude 兼容）",
};
const LINK_MODES: Record<LinkMode, string> = {
  copy: "复制",
  symlink: "软链",
  auto: "自动",
};
export function ProjectsPage() {
  return (
    <NavigationGuard>
      <ProjectsContent />
    </NavigationGuard>
  );
}

/**
 * 项目与分组同构：成员（单独绑定的 skill + 绑定的分组）只保存一次，每个 Agent 独立决定
 * 是否写进这个项目的目录。卡片、Agent 图标、需要检查 / 有待应用修改都和分组共用
 * （见 `components/groups`），只是写到项目目录而不是 Agent 的全局目录。
 */
function ProjectsContent() {
  const requestNavigation = useNavigationGuard();
  const projectsQuery = useProjects();
  const projects = useMemo(
    () => projectsQuery.data ?? [],
    [projectsQuery.data],
  );
  const { data: skills = [] } = useSkills();
  const { data: groups = [] } = useGroups();
  const { data: allAgents = [], isDetecting, detectionFailed } = useAgents();
  const { data: settings } = useSettings();
  // 同一份项目列表也在 MCP 项目页：移除项目前两边都要停用
  const mcpStatus = useQuery({
    queryKey: ["mcp", "local"],
    queryFn: managementApi.list,
    enabled: settings?.manageMcp ?? false,
    staleTime: MCP_STATUS_STALE_TIME,
    refetchInterval: MCP_STATUS_POLL_INTERVAL,
  });
  const client = useQueryClient();
  const projectAgents = useMemo(
    () => allAgents.filter((a) => a.supportsProjectSkills),
    [allAgents],
  );
  const managedIds = new Set(
    projectAgents
      .filter((a) => !settings?.disabledAgents?.includes(a.id))
      .map((a) => a.id),
  );
  const [viewAgentId, setViewAgentId] = useGroupAgentView(
    "skills",
    [...managedIds],
    "projects",
  );
  const viewAgent = projectAgents.find((a) => a.id === viewAgentId);
  const [query, setQuery] = useState("");
  const [detail, setDetail] = useState<string | null>(null);
  const appear = useDetailAppear();
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<ProjectBinding | null>(null);
  const { busy, run } = useGroupRunner(() => client.invalidateQueries());
  const reorder = useGroupReorder<ProjectBinding[]>({
    queryKey: queryKeys.projects,
    reorder: (data, ids) =>
      ids.flatMap((id) => data.filter((p) => p.id === id)),
    save: projectsApi.reorder,
  });
  const tokens = useMemo(() => tokenIndex(skills), [skills]);
  const skillName = (id: string) => skills.find((s) => s.id === id)?.name;
  const activeIds = (p: ProjectBinding) =>
    (p.deployments ?? []).map((d) => d.agentId);
  const mcpActive = (p: ProjectBinding) =>
    Object.keys(mcpStatus.data?.projects?.[p.id]?.active ?? {}).length > 0;

  /** 各 Agent 对这个项目的状态，由部署明细算出；卡片和详情标题栏共用 */
  function cardState(p: ProjectBinding): GroupCardState {
    const members = projectSkillIds(p, groups);
    const existing = members.filter((id) => skillName(id));
    const statuses: AgentGroupStatus[] = projectAgents
      .filter(
        (a) =>
          managedIds.has(a.id) ||
          (p.deployments ?? []).some((d) => d.agentId === a.id),
      )
      .map((a) => {
        const deployed = (p.deployments ?? []).find((d) => d.agentId === a.id);
        const detection = agentDetection(a, { isDetecting, detectionFailed });
        const dir = projectSkillDirectory(a);
        return {
          where: dir
            ? `${dir}${SHARED_DIRS[dir] ? `（${SHARED_DIRS[dir]}）` : ""}`
            : undefined,
          agentId: a.id,
          label: a.displayName,
          managed: managedIds.has(a.id),
          note: detection.note,
          dimmed: detection.notInstalled,
          active: !!deployed,
          changed:
            !!deployed &&
            (deployed.stale || !sameMembers(deployed.skillIds, existing)),
          issue: deployed?.issue ?? undefined,
        };
      });
    return groupCardState({
      statuses,
      used: statuses.some((s) => s.active),
      memberCount: members.length,
      missingMembers:
        existing.length < members.length ||
        p.groupIds.some((id) => !groups.some((g) => g.id === id)),
      viewAgentId: viewAgent?.id,
      labels: PROJECT_CARD_LABELS("skill"),
    });
  }
  /** 只改这个项目写进哪些 Agent：成员用已保存的 */
  const write = (p: ProjectBinding, agentIds: string[]) =>
    projectsApi
      .apply(p.id, {
        agentIds,
        skillIds: p.skillIds,
        groupIds: p.groupIds,
        linkMode: p.linkMode,
      })
      .then(checked);
  const toggle = (p: ProjectBinding, agentId: string, on: boolean) =>
    void run(
      () =>
        write(
          p,
          on
            ? [...activeIds(p), agentId]
            : activeIds(p).filter((id) => id !== agentId),
        ),
      {
        success: `${on ? "项目已启用" : "项目已停用"}，${RELOAD}`,
        notice: (report) => untouchedSkillNote(report.untouched),
      },
    );
  const reapply = (p: ProjectBinding) =>
    void run(() => write(p, activeIds(p)), {
      success: `修改已写入，${RELOAD}`,
      notice: (report) => untouchedSkillNote(report.untouched),
    });

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
        description="选一个项目根目录，之后选择只在这个项目里用的 skill，再在卡片上为需要的 Agent 启用。"
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
            <span className="font-medium">
              已写进项目目录的 skill 文件不会被删除
            </span>
            —— 它们可能已经提交进 git 了。
          </>
        }
        onConfirm={() =>
          deleting &&
          void run(
            async () => {
              await projectsApi.remove(deleting.id);
              // 同一个项目在 MCP 项目页的记录（成员）一并删掉
              if (mcpStatus.data?.projects?.[deleting.id])
                await mcpRequest("removeProject", { projectId: deleting.id });
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

  const active = projects.find((p) => p.id === detail) ?? null;
  if (active)
    return (
      <>
        {tools}
        <DetailTransition key={active.id} appear={appear}>
          <ProjectDetail
            project={active}
            state={cardState(active)}
            busy={busy}
            onToggle={(agentId, on) => toggle(active, agentId, on)}
            onReapply={() => reapply(active)}
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
  const uncollected = projects.reduce(
    (sum, p) => sum + (p.uncollectedSkillCount ?? 0),
    0,
  );
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
          loading={projectsQuery.isPending}
          error={projectsQuery.error}
          ids={projects.map((p) => p.id)}
          visible={visible.length}
          reorderDisabled={busy || reorder.isPending || !!keyword}
          onReorder={(ids) => reorder.mutate(ids)}
          noun="skill"
          hub="Skill Hub"
          labels={{
            ...PROJECT_BOARD_LABELS("skill"),
            emptyDescription:
              "绑定一个项目目录，就能让某些 skill 只对这个项目生效。项目级默认用文件复制而不是软链——软链进 git 是一个指向本机绝对路径的死链。",
          }}
          summary={
            <GroupAgentFilter
              noun="项目"
              total={projects.length}
              agents={projectAgents.filter((a) => managedIds.has(a.id))}
              current={(agentId) =>
                projects.filter((p) => activeIds(p).includes(agentId)).length
              }
              selected={viewAgent?.id ?? null}
              onSelect={setViewAgentId}
            >
              <SkillBackups scope="projects" />
              <SummaryStat
                title={`未托管：各项目目录中未托管、未部署的 skill，共 ${uncollected} 个`}
              >
                未托管 {uncollected}
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
            const members = projectSkillIds(p, groups);
            const rowTokens = sumTokens(tokens, members);
            const state = cardState(p);
            const locked = state.used || mcpActive(p);
            return (
              <GroupCard
                key={p.id}
                id={p.id}
                name={p.name}
                noun="skill"
                memberCount={members.length}
                subtitle={
                  <p className="mt-1 truncate font-mono text-[11px] text-muted-foreground">
                    {p.root}
                  </p>
                }
                extraBadges={
                  <>
                    {rowTokens.total > 0 && (
                      <Badge
                        variant="outline"
                        title={tokenTitle(
                          rowTokens,
                          "项目成员（直接绑定 + 绑定分组，去重后）",
                        )}
                      >
                        ≈ {formatTokens(rowTokens.total)} tokens
                      </Badge>
                    )}
                    <Badge variant="outline" title="链接方式">
                      {LINK_MODES[p.linkMode]}
                    </Badge>
                    {!!p.uncollectedSkillCount && (
                      <Badge
                        variant="outline"
                        title={`未托管：项目目录里未托管、未部署的 skill，共 ${p.uncollectedSkillCount} 个`}
                      >
                        未托管 {p.uncollectedSkillCount}
                      </Badge>
                    )}
                  </>
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
                        blocked: "请先在 MCP 项目里停用所有 Agent",
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
                onReapply={() => reapply(p)}
              />
            );
          })}
        </GroupBoard>
      </DetailTransition>
      {dialogs}
    </div>
  );
}

/** 写入报告里有失败的 skill 时当作失败：修改已保留，用户检查后重试 */
function checked(report: LinkReport): LinkReport {
  if (report.failed.length)
    throw new Error(
      `部分 skill 写入失败：${report.failed.map((f) => f.skillName).join("、")}`,
    );
  return report;
}

function ProjectDetail({
  project,
  state,
  busy,
  onToggle,
  onReapply,
  onBack,
}: {
  project: ProjectBinding;
  state: GroupCardState;
  busy: boolean;
  onToggle: (agentId: string, on: boolean) => void;
  onReapply: () => void;
  onBack: () => void;
}) {
  const qc = useQueryClient();
  const { previewSkill, previewDialog } = useSkillPreview();
  const [deletingLocal, setDeletingLocal] = useState<ProjectLocalSkill | null>(
    null,
  );
  const localSkills = useQuery({
    queryKey: [...queryKeys.projects, project.id, "localSkills"],
    queryFn: () => projectsApi.localSkills(project.id),
  });
  const { data: agents = [] } = useAgents();
  const { data: skills = [] } = useSkills();
  const { data: groups = [] } = useGroups();
  const gitignore = useWriteProjectGitignore();
  const [pending, setPending] = useState<string | null>(null);
  const [writing, setWriting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const remote = useMemo(
    () => ({
      skillIds: project.skillIds,
      groupIds: project.groupIds,
      linkMode: project.linkMode,
    }),
    [project.skillIds, project.groupIds, project.linkMode],
  );
  const [value, setValue] = useState(remote);
  const [saved, setSaved] = useState(remote);
  // 打开详情时已选的排在最前，勾选过程中不跳动
  const [pinned] = useState(remote);
  const fingerprint = (v: typeof value) =>
    JSON.stringify({
      ...v,
      skillIds: [...v.skillIds].sort(),
      groupIds: [...v.groupIds].sort(),
    });
  const dirty = fingerprint(value) !== fingerprint(saved);
  const locked = writing || busy || !!pending;
  useUnsavedProject(dirty, locked);
  const activeAgents = (project.deployments ?? []).map((d) => d.agentId);
  const agentName = (id: string) =>
    agents.find((a) => a.id === id)?.displayName ?? id;

  const refreshLocal = () =>
    Promise.all([
      qc.invalidateQueries({ queryKey: queryKeys.projects }),
      qc.invalidateQueries({ queryKey: queryKeys.skills }),
      qc.invalidateQueries({ queryKey: queryKeys.skillBackups }),
    ]);
  /** 项目里已有 skill 的就地操作：同一时间只做一件 */
  async function local(
    key: string,
    action: () => Promise<unknown>,
    done?: string,
  ) {
    setPending(key);
    try {
      await action();
      await refreshLocal();
      if (done) toast.success(done);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setPending(null);
    }
  }
  const toggleMember = (key: "skillIds" | "groupIds", id: string) =>
    setValue((prev) => ({
      ...prev,
      [key]: prev[key].includes(id)
        ? prev[key].filter((x) => x !== id)
        : [...prev[key], id],
    }));
  // 已启用 Agent 上没有可用版本的 skill 不能新选
  const variantIssue = (skill: (typeof skills)[number]) =>
    activeAgents
      .map((id) => skill.agents[id]?.unavailableReason)
      .filter(Boolean)
      .join("；");
  async function save() {
    setWriting(true);
    setError(null);
    try {
      const report = checked(
        await projectsApi.apply(project.id, {
          ...value,
          agentIds: activeAgents,
        }),
      );
      setSaved(value);
      await qc.invalidateQueries();
      const done = activeAgents.length
        ? `已保存并写入 ${activeAgents.map(agentName).join("、")}，${RELOAD}`
        : "已保存，可在卡片上为需要的 Agent 启用";
      const note = untouchedSkillNote(report.untouched);
      if (note) toast.warning(`${done}。${note}。`, { duration: 8000 });
      else toast.success(done);
    } catch (e) {
      setError(String(e));
    } finally {
      setWriting(false);
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ProjectDetailHeader
        name={project.name}
        root={project.root}
        onBack={onBack}
        agents={
          <div title={dirty ? "先保存修改，再为 Agent 启停" : undefined}>
            <GroupAgentToggles
              groupName={project.name}
              busy={locked || dirty}
              agents={state.toggles}
              onToggle={onToggle}
              reapply={
                state.reapply && {
                  label: state.reapply.label,
                  targets: state.reapply.targets,
                  disabled: state.reapply.disabled,
                  onClick: onReapply,
                }
              }
            />
          </div>
        }
        actions={
          <Button
            variant="outline"
            size="sm"
            onClick={() => gitignore.mutate(project.id)}
          >
            写入 .gitignore
          </Button>
        }
        save={{
          onClick: () => void save(),
          pending: writing,
          disabled: locked || !dirty,
          title: activeAgents.length
            ? `保存并写入 ${activeAgents.map(agentName).join("、")}`
            : "保存成员；还没有 Agent 启用，不写入项目",
        }}
      />
      {error && (
        <p role="alert" className="text-sm text-destructive">
          写入失败，修改已保留：{error}
        </p>
      )}
      {/* Leave room for focus rings inside the scroll viewport without shifting the fields. */}
      <div className="-mx-1 min-h-0 flex-1 space-y-6 overflow-y-auto px-1 pb-6 pt-1">
        <section className="space-y-2">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-semibold">项目已有 skill</h3>
            <SkillBackups scope={`project:${project.id}`} />
          </div>
          <p className="text-xs text-muted-foreground">
            项目目录里已有的 skill 可单独启停、托管到 Hub；卡片上的 Agent
            启停只作用于下面选择的成员。
          </p>
          {localSkills.isPending && (
            <p className="text-sm text-muted-foreground">扫描中…</p>
          )}
          {localSkills.isError && (
            <p role="alert" className="text-sm text-destructive">
              扫描失败：{String(localSkills.error)}
            </p>
          )}
          {localSkills.data?.length === 0 && (
            <p className="text-sm text-muted-foreground">
              项目目录中暂无 skill
            </p>
          )}
          {localSkills.data?.map((skill) => (
            <ListItemRow
              key={skill.path}
              onPreview={() =>
                previewSkill(skill.name, skill.storagePath ?? skill.path)
              }
              previewLabel={`预览 ${skill.name}`}
              card
              className={
                !skill.disabled &&
                skill.frontmatter &&
                !skill.frontmatter.malformed
                  ? "border-emerald-500/40 bg-card bg-gradient-to-r from-emerald-500/10 to-transparent shadow-sm shadow-emerald-500/5 hover:border-emerald-500/60 hover:bg-card dark:from-emerald-500/15"
                  : undefined
              }
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <p className="truncate text-sm font-medium">{skill.name}</p>
                  <span className="shrink-0" title={agentName(skill.agentId)}>
                    <AgentIcon agentId={skill.agentId} size="sm" />
                  </span>
                  {skill.tokens && (
                    <Badge
                      variant="outline"
                      className="shrink-0"
                      title={`${tokenTitle(
                        {
                          ...skill.tokens,
                          total: skill.tokens.skillMd + skill.tokens.extras,
                        },
                        skill.name,
                      )} 整个目录文本合计 ≈ ${formatTokens(skill.tokens.skillMd + skill.tokens.extras)} tokens。`}
                    >
                      ≈ {formatTokens(skill.tokens.skillMd)} tokens
                    </Badge>
                  )}
                  {skill.disabled && <Badge variant="outline">已停用</Badge>}
                  {skill.collected && <Badge variant="outline">已托管</Badge>}
                  {skill.managed && <Badge variant="outline">已部署</Badge>}
                </div>
                {skill.frontmatter?.description && (
                  <p className="truncate text-xs text-muted-foreground">
                    {skill.frontmatter.description}
                  </p>
                )}
                <p
                  className="truncate font-mono text-[11px] text-muted-foreground"
                  title={skill.path}
                >
                  {skill.path}
                </p>
                {!skill.frontmatter && (
                  <p className="text-xs text-destructive">
                    链接目标不可用或缺少 SKILL.md
                  </p>
                )}
                {skill.frontmatter?.malformed && (
                  <p className="text-xs text-destructive">
                    {skill.frontmatter.error ?? "SKILL.md 格式无效"}
                  </p>
                )}
              </div>
              <RowActions busy={pending === skill.path}>
                <Button
                  variant="ghost"
                  size="icon"
                  title="打开目录"
                  aria-label={`打开 ${skill.name} 目录`}
                  onClick={() =>
                    void systemApi
                      .revealPath(skill.storagePath ?? skill.path)
                      .catch((e) => toast.error(String(e)))
                  }
                >
                  <FolderOpen className="h-4 w-4" />
                </Button>
                {!skill.managed && (
                  <>
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={locked}
                      onClick={() =>
                        void local(skill.path, () =>
                          projectsApi.setLocalEnabled(
                            project.id,
                            skill.path,
                            !!skill.disabled,
                          ),
                        )
                      }
                    >
                      {skill.disabled ? (
                        <Play className="h-4 w-4" />
                      ) : (
                        <Square className="h-4 w-4" />
                      )}
                      {skill.disabled ? "启用" : "停用"}
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      title={
                        skill.collected
                          ? "已托管到 Hub"
                          : "托管到 Hub（保留项目原文件）"
                      }
                      aria-label={`托管 ${skill.name} 到 Hub`}
                      disabled={
                        skill.collected ||
                        !skill.frontmatter ||
                        skill.frontmatter.malformed ||
                        locked
                      }
                      onClick={() =>
                        void local(
                          skill.path,
                          () =>
                            projectsApi.collectLocal(project.id, skill.path),
                          "已托管到 Hub，项目原文件已保留",
                        )
                      }
                    >
                      <PackagePlus className="h-4 w-4" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      title="删除项目 skill"
                      aria-label={`删除 ${skill.name}`}
                      className="hover:text-destructive"
                      disabled={locked}
                      onClick={() => setDeletingLocal(skill)}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </>
                )}
              </RowActions>
            </ListItemRow>
          ))}
        </section>

        <section className="space-y-2">
          <h3 className="text-sm font-semibold">链接方式</h3>
          <div className="flex items-center gap-3">
            <Select
              value={value.linkMode}
              disabled={locked}
              onValueChange={(v) =>
                setValue((prev) => ({ ...prev, linkMode: v as LinkMode }))
              }
            >
              <SelectTrigger className="w-48" aria-label="链接方式">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="copy">复制（推荐）</SelectItem>
                <SelectItem value="symlink">符号链接</SelectItem>
                <SelectItem value="auto">自动</SelectItem>
              </SelectContent>
            </Select>
            <p className="text-xs leading-relaxed text-muted-foreground">
              项目 skill 通常要进 git 给团队共享。软链进 git 只是一个指向本机
              绝对路径的文本文件，别人拉下来就是断链，所以默认用复制。
            </p>
          </div>
        </section>

        {groups.length > 0 && (
          <MemberChecklist
            title="绑定分组"
            items={groups.map((g) => ({
              id: g.id,
              name: g.name,
              detail:
                g.skillIds
                  .map(
                    (id) =>
                      skills.find((s) => s.id === id)?.name ?? `已缺失：${id}`,
                  )
                  .join(" · ") || "尚未选择 skill",
              badge: `${g.skillIds.length} 个`,
            }))}
            selected={value.groupIds}
            pinned={pinned.groupIds}
            onToggle={(id) => toggleMember("groupIds", id)}
            disabled={locked}
            emptyText="还没有分组"
          />
        )}

        <MemberChecklist
          title="单独绑定 skill"
          items={[
            ...skills.map((s) => ({
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
                <SourceMark
                  key={id}
                  id={id}
                  label={SOURCE_LABELS[id] ?? agentName(id)}
                />
              )),
              disabledReason: variantIssue(s) || undefined,
            })),
            ...value.skillIds
              .filter((id) => !skills.some((s) => s.id === id))
              .map((id) => ({ id, name: `已缺失：${id}` })),
          ]}
          selected={value.skillIds}
          pinned={pinned.skillIds}
          onToggle={(id) => toggleMember("skillIds", id)}
          disabled={locked}
          searchPlaceholder="搜索 skill…"
          emptyText="Skill Hub 暂无 skill，请先安装或导入。"
        />
      </div>
      {previewDialog}
      <ConfirmDialog
        open={!!deletingLocal}
        onOpenChange={(open) => {
          if (!open && !pending) setDeletingLocal(null);
        }}
        title={`删除 ${deletingLocal?.name ?? "skill"}？`}
        description={
          <>
            从项目移除此
            skill，非软链接文件保留备份。软链接只移除链接，不保留备份；Hub
            副本不受影响。
            <span className="block break-all pt-2 font-mono text-xs">
              {deletingLocal?.path}
            </span>
          </>
        }
        confirmText="删除"
        pending={!!pending}
        onConfirm={() =>
          deletingLocal &&
          void local(
            deletingLocal.path,
            async () => {
              await projectsApi.deleteLocalSkill(
                project.id,
                deletingLocal.path,
              );
              setDeletingLocal(null);
            },
            "已删除；非软链接文件可从已备份列表恢复",
          )
        }
      />
    </div>
  );
}
