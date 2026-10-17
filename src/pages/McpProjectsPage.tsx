import {
  SummaryBar,
  SummaryStart,
  SummaryEnd,
  summaryPillClass,
} from "@/components/common/SummaryBar";
import {
  MCP_PROJECT_SUFFIXES,
  inProject,
  mcpAgentId,
  supportsMcp,
} from "@/lib/mcpAgents";
import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, FolderGit2, FolderOpen, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { McpAssignments } from "@/components/mcp/McpAssignments";
import { PageTools } from "@/components/common/PageTools";
import { NameAvatar } from "@/components/common/NameAvatar";
import {
  DetailTransition,
  useDetailAppear,
} from "@/components/common/DetailTransition";
import {
  ListContainer,
  ListItemRow,
  RowActions,
} from "@/components/common/ListItemRow";
import { EmptyState } from "@/components/common/EmptyState";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import {
  NavigationGuard,
  useNavigationGuard,
} from "@/components/common/NavigationGuard";
import { useTarget } from "@/components/targets/TargetProvider";
import { AgentIcon } from "@/components/common/AgentIcon";
import { agentSourceColor } from "@/lib/agents";
import { useAgents } from "@/hooks/useData";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { useProjects, useCreateProject } from "@/hooks/useData";
import { projectsApi, systemApi } from "@/lib/api";
import {
  managementApi,
  rows,
  MCP_STATUS_STALE_TIME,
  MCP_STATUS_POLL_INTERVAL,
} from "@/lib/api/mcpManagement";
import { mcpRequest } from "@/lib/api/mcp";

export function McpProjectsPage() {
  const target = useTarget();
  if (target.id !== "local")
    return (
      <p className="p-6 text-sm text-muted-foreground">
        项目 MCP 当前仅支持本机。请切换到本机管理。
      </p>
    );
  return (
    <NavigationGuard>
      <McpProjectsContent />
    </NavigationGuard>
  );
}
function McpProjectsContent() {
  const requestNavigation = useNavigationGuard();
  const projects = useProjects();
  const status = useQuery({
    queryKey: ["mcp", "local"],
    queryFn: managementApi.list,
    // Keep polling in the background without fetching again on every Agent switch.
    staleTime: MCP_STATUS_STALE_TIME,
    refetchInterval: MCP_STATUS_POLL_INTERVAL,
  });
  const create = useCreateProject();
  const { data: allAgents = [] } = useAgents();
  // Skill 侧的 Agent 列表（含品牌图标），过滤到真正支持 MCP 的
  const mcpAgents = useMemo(
    () => allAgents.filter((a) => supportsMcp(a.id)),
    [allAgents],
  );
  const [agentFilter, setAgentFilter] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [detail, setDetail] = useState<string | null>(null);
  const appear = useDetailAppear();
  const [creating, setCreating] = useState(false);
  const [removingProject, setRemovingProject] = useState(false);
  const [removeBusy, setRemoveBusy] = useState(false);
  const [form, setForm] = useState({ name: "", root: "" });
  const active = projects.data?.find((project) => project.id === detail);
  const catalog = rows(status.data);
  const managedProjectCount = active
    ? catalog.reduce(
        (sum, row) =>
          sum +
          row.entry.bindings.filter(
            (binding) =>
              binding.project === active.root ||
              MCP_PROJECT_SUFFIXES.some(
                (suffix) =>
                  binding.path === active.root.replace(/\/+$/, "") + suffix,
              ),
          ).length,
        0,
      )
    : 0;
  async function removeAll() {
    if (!active || removeBusy) return;
    setRemoveBusy(true);
    try {
      const result = await mcpRequest<{ removed: number }>(
        "removeProjectBindings",
        { projectId: active.id },
      );
      await status.refetch();
      setRemovingProject(false);
      toast.success(
        `已移除 ${result.removed} 处托管接入，请在 Agent 中重新加载 MCP`,
      );
    } catch (error) {
      toast.error(String(error));
    } finally {
      setRemoveBusy(false);
    }
  }
  const count = (root: string) =>
    catalog.filter((row) =>
      [...row.sources, ...row.entry.bindings].some(
        (source) =>
          source.project === root ||
          MCP_PROJECT_SUFFIXES.some(
            (suffix) => source.path === root.replace(/\/+$/, "") + suffix,
          ),
      ),
    ).length;
  /** 这个项目里有没有该 MCP Agent 的接入（托管的或原有配置都算） */
  const agentInProject = (mcpId: string, project: { root: string }) =>
    catalog.some((row) =>
      [...row.entry.bindings, ...row.sources].some(
        (source) => source.agent === mcpId && inProject(source, project),
      ),
    );
  const list = projects.data ?? [];
  const agentCount = (appId: string) =>
    list.filter((project) => agentInProject(mcpAgentId(appId), project)).length;
  const selectedMcpId = agentFilter ? mcpAgentId(agentFilter) : null;
  // Studio 写进项目的托管接入，和项目里已有但没纳入管理的配置
  const projectBindings = catalog.flatMap((row) =>
    row.entry.bindings.filter((binding) =>
      list.some((project) => inProject(binding, project)),
    ),
  );
  const unmanagedSources = catalog.flatMap((row) =>
    row.sources.filter(
      (source) =>
        list.some((project) => inProject(source, project)) &&
        !row.entry.bindings.some(
          (binding) =>
            binding.agent === source.agent &&
            binding.path === source.path &&
            binding.key === source.key &&
            (binding.project ?? null) === (source.project ?? null),
        ),
    ),
  );
  const enabledProjects = list.filter((project) =>
    catalog.some((row) =>
      row.entry.bindings.some((binding) => inProject(binding, project)),
    ),
  ).length;
  const filtered = list
    .filter((project) =>
      `${project.name} ${project.root}`
        .toLowerCase()
        .includes(query.trim().toLowerCase()),
    )
    .filter(
      (project) => !selectedMcpId || agentInProject(selectedMcpId, project),
    );
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageTools
        query={query}
        onQueryChange={(value) =>
          requestNavigation(() => {
            setQuery(value);
            setDetail(null);
          })
        }
        placeholder="搜索项目…"
        createLabel="添加项目"
        onCreate={() =>
          requestNavigation(() => {
            setForm({ name: "", root: "" });
            setCreating(true);
          })
        }
      />
      {active ? (
        // 标题栏和内容一起过渡，与 Skill 项目详情一致
        <DetailTransition key={active.id} appear={appear}>
          <div className="flex items-center gap-3 py-4">
            <Button
              variant="outline"
              size="icon"
              title="返回"
              onClick={() => requestNavigation(() => setDetail(null))}
            >
              <ArrowLeft className="h-4 w-4" />
            </Button>
            <div className="min-w-0 flex-1">
              <p className="truncate text-base font-semibold">{active.name}</p>
              <p className="truncate text-xs text-muted-foreground">
                {active.root}
              </p>
            </div>
            <Button
              variant="ghost"
              size="icon"
              title="打开项目目录"
              onClick={() => void systemApi.revealPath(active.root)}
            >
              <FolderOpen className="h-4 w-4" />
            </Button>
            {managedProjectCount > 0 && (
              <Button
                variant="outline"
                size="sm"
                disabled={removeBusy}
                onClick={() => setRemovingProject(true)}
              >
                <Trash2 className="h-4 w-4" />
                移除托管接入 ({managedProjectCount})
              </Button>
            )}
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto pb-6">
            <McpAssignments project={active} />
          </div>
        </DetailTransition>
      ) : (
        <>
          <DetailTransition appear={appear}>
            <SummaryBar>
              <SummaryStart
                role="group"
                aria-label="按项目 Agent 筛选"
                title="点击 Agent 筛选：这些项目里配了该 Agent 的 MCP（含未托管的原有配置）"
              >
                {mcpAgents.map((a) => (
                  <button
                    key={a.id}
                    type="button"
                    title={`筛选：${a.displayName} 配了 MCP 的项目（含未托管的原有配置）`}
                    className={`${summaryPillClass} border-transparent transition-colors hover:brightness-95 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${agentSourceColor(a.id)} ${agentFilter === a.id ? "ring-2 ring-current" : ""}`}
                    aria-pressed={agentFilter === a.id}
                    aria-label={`${a.displayName}: ${agentCount(a.id)}`}
                    onClick={() =>
                      setAgentFilter(agentFilter === a.id ? null : a.id)
                    }
                  >
                    <AgentIcon agentId={a.id} size="sm" />
                    {agentCount(a.id)}
                  </button>
                ))}
              </SummaryStart>
              <SummaryEnd title="按 Studio 已写入的项目接入统计，同一项目只计一次">
                <Badge
                  variant="outline"
                  className={summaryPillClass}
                  title="已登记的项目目录数量"
                >
                  项目 {list.length} 个
                </Badge>
                <Badge
                  variant="outline"
                  className={summaryPillClass}
                  title="Studio 写进这些项目的接入条数，同一项目可以有多条"
                >
                  托管接入 {projectBindings.length} 处
                </Badge>
                <Badge
                  variant="outline"
                  className={summaryPillClass}
                  title="项目里已有、但没纳入 Studio 管理的 MCP 配置，可在项目详情里「纳入管理」"
                >
                  未托管 {unmanagedSources.length} 处
                </Badge>
                <Badge
                  variant="outline"
                  className={summaryPillClass}
                  title="至少有一条托管接入的项目数量"
                >
                  已启用 {enabledProjects} 个
                </Badge>
              </SummaryEnd>
            </SummaryBar>
            <div className="min-h-0 flex-1 overflow-y-auto pb-6">
              {(projects.isPending || status.isPending) && (
                <p className="py-6 text-sm text-muted-foreground">
                  正在读取项目 MCP…
                </p>
              )}
              {(projects.error || status.error) && (
                <p role="alert" className="text-sm text-destructive">
                  {String(projects.error ?? status.error)}
                </p>
              )}
              {!projects.isPending &&
                !projects.error &&
                !projects.data?.length && (
                  <EmptyState
                    icon={FolderGit2}
                    title="还没有项目"
                    description="添加项目目录后，可从 MCP Hub 选择服务，只在该项目中接入。"
                  />
                )}
              <ListContainer cards>
                {filtered.map((project) => (
                  <ListItemRow
                    key={project.id}
                    card
                    onPreview={() => setDetail(project.id)}
                    previewLabel={`查看 ${project.name} 的 MCP`}
                  >
                    <NameAvatar name={project.name} />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="truncate text-sm font-medium">
                          {project.name}
                        </span>
                        {!status.isPending && !status.error && (
                          <Badge variant="outline">
                            {count(project.root)} 个 MCP
                          </Badge>
                        )}
                      </div>
                      <p className="truncate pt-0.5 text-xs text-muted-foreground">
                        {project.root}
                      </p>
                    </div>
                    <RowActions>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8"
                        title="打开项目目录"
                        onClick={() => void systemApi.revealPath(project.root)}
                      >
                        <FolderOpen className="h-4 w-4" />
                      </Button>
                    </RowActions>
                  </ListItemRow>
                ))}
              </ListContainer>
              {!!projects.data?.length && !filtered.length && (
                <p className="py-6 text-center text-sm text-muted-foreground">
                  没有匹配的项目
                </p>
              )}
            </div>
          </DetailTransition>
        </>
      )}
      <Dialog
        open={creating}
        onOpenChange={(open) => !create.isPending && setCreating(open)}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>添加项目</DialogTitle>
            <DialogDescription>
              选择项目根目录，在此项目中配置 MCP。
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 px-6 py-5">
            <label className="block space-y-2 text-sm">
              <span>项目目录</span>
              <div className="flex gap-2">
                <Input
                  aria-label="项目目录"
                  value={form.root}
                  disabled={create.isPending}
                  onChange={(e) => setForm({ ...form, root: e.target.value })}
                />
                <Button
                  variant="outline"
                  size="icon"
                  aria-label="选择项目目录"
                  disabled={create.isPending}
                  onClick={async () => {
                    const root = await projectsApi.pickDirectory();
                    if (root)
                      setForm((prev) => ({
                        root,
                        name:
                          prev.name ||
                          root.split(/[/\\]/).filter(Boolean).pop() ||
                          root,
                      }));
                  }}
                >
                  <FolderOpen className="h-4 w-4" />
                </Button>
              </div>
            </label>
            <label className="block space-y-2 text-sm">
              <span>项目名</span>
              <Input
                aria-label="项目名"
                value={form.name}
                disabled={create.isPending}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </label>
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={create.isPending}
              onClick={() => setCreating(false)}
            >
              取消
            </Button>
            <Button
              disabled={
                create.isPending || !form.name.trim() || !form.root.trim()
              }
              onClick={() =>
                create.mutate(form, { onSuccess: () => setCreating(false) })
              }
            >
              {create.isPending ? "添加中…" : "添加"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        open={removingProject}
        onOpenChange={setRemovingProject}
        title={`移除 ${active?.name ?? "此项目"} 的托管 MCP？`}
        description="仅恢复此项目中由 Studio 写入的 MCP 接入，保留 Hub 服务、原有手动配置和项目中的 Skill。若配置已被手动修改，操作会停止，不会覆盖修改。"
        confirmText="移除托管接入"
        pending={removeBusy}
        onConfirm={() => void removeAll()}
      />
    </div>
  );
}
