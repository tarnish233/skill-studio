import {
  SummaryBar,
  SummaryStart,
  SummaryEnd,
  summaryPillClass,
} from "@/components/common/SummaryBar";
import { agentSourceColor, projectSkillDirectory } from "@/lib/agents";
import { useSkillPreview } from "@/components/common/SkillPreview";
import { SkillBackups } from "@/components/common/SkillBackups";
import { PageTools } from "@/components/common/PageTools";
import { NameAvatar } from "@/components/common/NameAvatar";
import {
  NavigationGuard,
  useNavigationGuard,
  useUnsavedProject,
} from "@/components/common/NavigationGuard";
import { useMemo, useState, type ReactNode } from "react";
import {
  ArrowLeft,
  FolderGit2,
  FolderOpen,
  Trash2,
  Upload,
  Play,
  Square,
  GripVertical,
  PackagePlus,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { AgentIcon } from "@/components/common/AgentIcon";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { EmptyState } from "@/components/common/EmptyState";
import {
  ListContainer,
  ListItemRow,
  RowActions,
} from "@/components/common/ListItemRow";
import { projectsApi, systemApi } from "@/lib/api";
import type { ProjectLocalSkill } from "@/lib/api/projects";
import {
  useAgents,
  useApplyProject,
  useCreateProject,
  useDeleteProject,
  useGroups,
  useProjects,
  useSkills,
  useWriteProjectGitignore,
} from "@/hooks/useData";
import {
  formatTokens,
  projectSkillIds,
  sumTokens,
  tokenIndex,
  tokenTitle,
} from "@/lib/tokens";
import type { LinkMode, ProjectBinding } from "@/types";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { queryKeys } from "@/lib/queryKeys";
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

function projectAgentIds(project: ProjectBinding): string[] {
  return project.enabledAgentIds ?? [];
}

export function ProjectsPage() {
  return (
    <NavigationGuard>
      <ProjectsContent />
    </NavigationGuard>
  );
}
function ProjectsContent() {
  const requestNavigation = useNavigationGuard();
  const { data: projects = [] } = useProjects();
  const { data: skills = [] } = useSkills();
  const { data: groups = [] } = useGroups();
  const { data: agents = [] } = useAgents();
  const client = useQueryClient();
  const [sourceFilter, setSourceFilter] = useState<string | null>(null);
  const toggleProject = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      projectsApi.setEnabled(id, enabled),
    onSuccess: (_, { enabled }) =>
      toast.success(enabled ? "项目已启用" : "项目已停用"),
    onError: (e) => toast.error(String(e)),
    onSettled: () => client.invalidateQueries({ queryKey: queryKeys.projects }),
  });
  const reorder = useMutation({
    mutationFn: (next: ProjectBinding[]) =>
      projectsApi.reorder(next.map((p) => p.id)),
    onMutate: async (next) => {
      await client.cancelQueries({ queryKey: queryKeys.projects });
      const previous = client.getQueryData<ProjectBinding[]>(
        queryKeys.projects,
      );
      client.setQueryData(queryKeys.projects, next);
      return { previous };
    },
    // The command returns stored bindings; keep computed deployment status until refetch.

    onError: (e, _, context) => {
      if (context?.previous)
        client.setQueryData(queryKeys.projects, context.previous);
      toast.error(String(e));
    },
    onSettled: () => client.invalidateQueries({ queryKey: queryKeys.projects }),
  });
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );
  const onDragEnd = ({ active, over }: DragEndEvent) => {
    if (!over || active.id === over.id || reorder.isPending) return;
    const from = projects.findIndex((p) => p.id === active.id);
    const to = projects.findIndex((p) => p.id === over.id);
    if (from >= 0 && to >= 0) reorder.mutate(arrayMove(projects, from, to));
  };
  const createProject = useCreateProject();
  const deleteProject = useDeleteProject();

  // token 口径与后端写入口径必须是同一个，见 projectSkillIds：
  // 勾了哪些 agent 决定了哪些绑定分组会被展开，没勾 agent 就一个文件都不写
  const tokens = useMemo(() => tokenIndex(skills), [skills]);
  const projectTokens = (p: ProjectBinding) =>
    sumTokens(tokens, projectSkillIds(p, groups));

  const [query, setQuery] = useState("");
  const filtered = projects.filter(
    (p) =>
      `${p.name} ${p.root}`
        .toLowerCase()
        .includes(query.trim().toLowerCase()) &&
      (!sourceFilter || projectAgentIds(p).includes(sourceFilter)),
  );
  const [detail, setDetail] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<ProjectBinding | null>(null);
  const [form, setForm] = useState({ name: "", root: "" });

  const tools = (
    <PageTools
      query={query}
      onQueryChange={(v) => {
        requestNavigation(() => {
          setQuery(v);
          setDetail(null);
        });
      }}
      placeholder="搜索项目…"
      createLabel="添加项目"
      onCreate={() => {
        requestNavigation(() => {
          setDetail(null);
          setForm({ name: "", root: "" });
          setCreating(true);
        });
      }}
    />
  );
  const active = projects.find((p) => p.id === detail) ?? null;
  if (active) {
    return (
      <>
        {tools}
        <ProjectDetail
          key={active.id}
          project={active}
          onBack={() => requestNavigation(() => setDetail(null))}
        />
      </>
    );
  }

  const pick = async () => {
    const picked = await projectsApi.pickDirectory();
    if (!picked) return;
    const base = picked.split(/[/\\]/).filter(Boolean).pop() ?? picked;
    setForm({ root: picked, name: form.name || base });
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {tools}
      <SummaryBar>
        <SummaryStart role="group" aria-label="按项目 Agent 筛选">
          {agents
            .filter((a) => a.supportsProjectSkills)
            .map((a) => (
              <button
                key={a.id}
                type="button"
                title={`筛选 ${a.displayName} 已启用的项目`}
                className={`${summaryPillClass} border-transparent transition-colors hover:brightness-95 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${agentSourceColor(a.id)} ${sourceFilter === a.id ? "ring-2 ring-current" : ""}`}
                aria-pressed={sourceFilter === a.id}
                onClick={() =>
                  setSourceFilter(sourceFilter === a.id ? null : a.id)
                }
              >
                <AgentIcon agentId={a.id} size="sm" />
                {`${a.displayName}:`}{" "}
                {
                  projects.filter((p) => projectAgentIds(p).includes(a.id))
                    .length
                }
              </button>
            ))}
        </SummaryStart>
        <SummaryEnd title="按 Skill Studio 已写入的项目部署统计，同一项目只计一次">
          <Badge variant="outline" className={summaryPillClass}>
            项目 {projects.length} 个
          </Badge>
          <SkillBackups scope="projects" className={summaryPillClass} />
          <Badge
            variant="outline"
            className={summaryPillClass}
            title="各项目目录中未托管、未部署的 skill 数量"
          >
            未托管 skill{" "}
            {projects.reduce(
              (sum, p) => sum + (p.uncollectedSkillCount ?? 0),
              0,
            )}{" "}
            个
          </Badge>
          <Badge variant="outline" className={summaryPillClass}>
            已启用{" "}
            {projects.filter((p) => projectAgentIds(p).length > 0).length} 个
          </Badge>
        </SummaryEnd>
      </SummaryBar>

      <div className="min-h-0 flex-1 overflow-y-auto pb-6">
        {projects.length === 0 ? (
          <EmptyState
            icon={FolderGit2}
            title="还没有项目"
            description="绑定一个项目目录，就能让某些 skill 只对这个项目生效。项目级默认用文件复制而不是软链——软链进 git 是一个指向本机绝对路径的死链。"
          />
        ) : (
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={onDragEnd}
          >
            <SortableContext
              items={filtered.map((p) => p.id)}
              strategy={verticalListSortingStrategy}
            >
              <ListContainer cards>
                {filtered.map((p) => {
                  // 一行只遍历一次 id 集合：下面的判断、数字、tooltip 共用
                  const rowTokens = projectTokens(p);
                  return (
                    <SortableProjectCard
                      key={p.id}
                      project={p}
                      disabled={reorder.isPending || toggleProject.isPending}
                      onOpen={() => setDetail(p.id)}
                    >
                      <NameAvatar name={p.name} />
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <span className="truncate text-sm font-medium">
                            {p.name}
                          </span>
                          {projectAgentIds(p).length > 0 && (
                            <Badge variant="success">使用中</Badge>
                          )}
                          <Badge
                            variant="outline"
                            className="h-4 px-1.5 text-[10px]"
                          >
                            {p.enabledGroupIds?.length ?? 0} 分组启动
                          </Badge>
                          <Badge
                            variant="outline"
                            className="h-4 px-1.5 text-[10px]"
                          >
                            {p.enabledSkillIds?.length ?? 0} skill启用
                          </Badge>
                          <Badge
                            variant="outline"
                            className="h-4 px-1.5 text-[10px]"
                          >
                            {p.uncollectedSkillCount ?? 0} skill未托管
                          </Badge>
                          {rowTokens.total > 0 && (
                            <Badge
                              variant="outline"
                              className="h-4 px-1.5 text-[10px]"
                              title={tokenTitle(
                                rowTokens,
                                "这个项目会用到的 skill（勾选的 agent × 直接绑定 + 归属匹配的分组，去重后）",
                              )}
                            >
                              ≈ {formatTokens(rowTokens.total)} tokens
                            </Badge>
                          )}
                          <Badge
                            variant="outline"
                            className="h-4 px-1.5 text-[10px]"
                          >
                            {p.linkMode === "copy"
                              ? "复制"
                              : p.linkMode === "symlink"
                                ? "软链"
                                : "自动"}
                          </Badge>
                        </div>
                        <p className="truncate pt-0.5 font-mono text-[11px] text-muted-foreground">
                          {p.root}
                        </p>
                      </div>
                      <div onClick={(e) => e.stopPropagation()}>
                        <RowActions
                          busy={
                            toggleProject.isPending &&
                            toggleProject.variables?.id === p.id
                          }
                        >
                          <Button
                            size="sm"
                            variant={
                              p.managedEntries?.length ? "outline" : "default"
                            }
                            disabled={
                              toggleProject.isPending ||
                              (!p.managedEntries?.length &&
                                projectSkillIds(p, groups).length === 0)
                            }
                            onClick={() =>
                              toggleProject.mutate({
                                id: p.id,
                                enabled: !p.managedEntries?.length,
                              })
                            }
                          >
                            {p.managedEntries?.length ? (
                              <Square className="h-4 w-4" />
                            ) : (
                              <Play className="h-4 w-4" />
                            )}
                            {p.managedEntries?.length ? "停用" : "启用"}
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8"
                            title="打开项目目录"
                            onClick={() => void systemApi.revealPath(p.root)}
                          >
                            <FolderOpen className="h-4 w-4" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 hover:text-red-500"
                            title={
                              p.managedEntries?.length
                                ? "请先停用项目"
                                : "移除项目"
                            }
                            disabled={
                              !!p.managedEntries?.length ||
                              toggleProject.isPending
                            }
                            onClick={() => setDeleting(p)}
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </RowActions>
                      </div>
                    </SortableProjectCard>
                  );
                })}
                {filtered.length === 0 && (
                  <p className="py-8 text-center text-sm text-muted-foreground">
                    没有匹配的项目
                  </p>
                )}
              </ListContainer>
            </SortableContext>
          </DndContext>
        )}
      </div>

      <Dialog open={creating} onOpenChange={setCreating}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>添加项目</DialogTitle>
            <DialogDescription>
              选一个项目根目录，之后绑定的 skill 只会写进这个项目里。
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 px-6 py-5">
            <div className="space-y-1.5">
              <Label>项目目录</Label>
              <div className="flex gap-2">
                <Input
                  value={form.root}
                  placeholder="点右侧按钮选择"
                  onChange={(e) => setForm({ ...form, root: e.target.value })}
                />
                <Button
                  variant="outline"
                  size="icon"
                  onClick={() => void pick()}
                >
                  <FolderOpen className="h-4 w-4" />
                </Button>
              </div>
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="proj-name">项目名</Label>
              <Input
                id="proj-name"
                value={form.name}
                placeholder="用于在列表里识别"
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </div>
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setCreating(false)}
            >
              取消
            </Button>
            <Button
              size="sm"
              disabled={!form.name.trim() || !form.root.trim()}
              onClick={() => {
                createProject.mutate({ name: form.name, root: form.root });
                setCreating(false);
              }}
            >
              添加
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => !o && setDeleting(null)}
        title={`移除项目「${deleting?.name}」`}
        confirmText="移除"
        pending={deleteProject.isPending}
        description={
          <>
            只解除绑定，
            <span className="font-medium">
              已写进项目目录的 skill 文件不会被删除
            </span>
            —— 它们可能已经提交进 git 了。
          </>
        }
        onConfirm={() => {
          if (deleting) deleteProject.mutate(deleting.id);
          setDeleting(null);
        }}
      />
    </div>
  );
}

function ProjectDetail({
  project,
  onBack,
}: {
  project: ProjectBinding;
  onBack: () => void;
}) {
  const qc = useQueryClient();
  const { previewSkill, previewDialog } = useSkillPreview();
  const [deletingLocal, setDeletingLocal] = useState<ProjectLocalSkill | null>(
    null,
  );
  const refreshLocal = async () => {
    await Promise.all([
      qc.invalidateQueries({ queryKey: queryKeys.projects }),
      qc.invalidateQueries({ queryKey: queryKeys.skills }),
      qc.invalidateQueries({ queryKey: queryKeys.skillBackups }),
    ]);
  };
  const collectLocal = useMutation({
    mutationFn: (path: string) => projectsApi.collectLocal(project.id, path),
    onSuccess: async () => {
      await refreshLocal();
      toast.success("已托管到 Hub，项目原文件已保留");
    },
    onError: (e) => toast.error(String(e)),
  });
  const deleteLocal = useMutation({
    mutationFn: (path: string) =>
      projectsApi.deleteLocalSkill(project.id, path),
    onSuccess: async () => {
      setDeletingLocal(null);
      await refreshLocal();
      toast.success("已删除；非软链接文件可从已备份列表恢复");
    },
    onError: (e) => toast.error(String(e)),
  });
  const toggleLocal = useMutation({
    mutationFn: ({ path, enabled }: { path: string; enabled: boolean }) =>
      projectsApi.setLocalEnabled(project.id, path, enabled),
    onSuccess: refreshLocal,
    onError: (e) => toast.error(String(e)),
  });
  const localSkills = useQuery({
    queryKey: [...queryKeys.projects, project.id, "localSkills"],
    queryFn: () => projectsApi.localSkills(project.id),
  });
  const { data: agents = [] } = useAgents();
  const { data: skills = [] } = useSkills();
  const { data: groups = [] } = useGroups();
  const apply = useApplyProject();
  const gitignore = useWriteProjectGitignore();
  const [query, setQuery] = useState("");
  const [writing, setWriting] = useState(false);

  const remote = useMemo(
    () => ({
      agentIds: project.agentIds,
      skillIds: project.skillIds,
      groupIds: project.groupIds,
      linkMode: project.linkMode,
    }),
    [project.agentIds, project.skillIds, project.groupIds, project.linkMode],
  );
  const [value, setValue] = useState(remote);
  const [saved, setSaved] = useState(remote);
  const [error, setError] = useState<string | null>(null);
  const fingerprint = (v: typeof value) =>
    JSON.stringify({
      ...v,
      agentIds: [...v.agentIds].sort(),
      skillIds: [...v.skillIds].sort(),
      groupIds: [...v.groupIds].sort(),
    });
  const dirty = fingerprint(value) !== fingerprint(saved);
  useUnsavedProject(dirty, writing);
  const draft = { value, edit: setValue };

  const projectAgents = useMemo(
    () => agents.filter((a) => a.supportsProjectSkills),
    [agents],
  );

  const filteredSkills = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return skills;
    return skills.filter((s) => s.name.toLowerCase().includes(q));
  }, [skills, query]);

  const toggle = (key: "agentIds" | "skillIds" | "groupIds", id: string) =>
    !writing &&
    draft.edit((previous) => ({
      ...previous,
      [key]: previous[key].includes(id)
        ? previous[key].filter((x) => x !== id)
        : [...previous[key], id],
    }));
  const toggleAgent = (id: string) => toggle("agentIds", id);
  const variantIssue = (skill: (typeof skills)[number]) =>
    draft.value.agentIds
      .map((id) => skill.agents[id]?.unavailableReason)
      .filter(Boolean)
      .join("；");
  const toggleSkill = (id: string) => {
    const skill = skills.find((s) => s.id === id);
    if (draft.value.skillIds.includes(id) || !skill || !variantIssue(skill))
      toggle("skillIds", id);
  };
  const toggleGroup = (id: string) => toggle("groupIds", id);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-3 py-4">
        <Button variant="outline" size="icon" onClick={onBack} title="返回">
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div className="min-w-0 flex-1">
          <p className="truncate text-base font-semibold">{project.name}</p>
          <p className="truncate font-mono text-[11px] text-muted-foreground">
            {project.root}
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => gitignore.mutate(project.id)}
        >
          写入 .gitignore
        </Button>
        <Button
          size="sm"
          disabled={
            writing ||
            apply.isPending ||
            collectLocal.isPending ||
            toggleLocal.isPending ||
            deleteLocal.isPending
          }
          onClick={() => {
            setWriting(true);
            setError(null);
            void apply
              .mutateAsync({ projectId: project.id, selection: value })
              .then((report) => {
                if (report.failed.length)
                  throw new Error("部分 skill 写入失败，请检查后重试");
                setSaved(value);
              })
              .catch((e) => setError(String(e)))
              .finally(() => setWriting(false));
          }}
        >
          <Upload className="h-4 w-4" />
          写入项目
        </Button>
      </div>

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
            项目已有 skill 可单独启停；项目卡片的启停仍只作用于绑定项。
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
                  <span
                    className="shrink-0"
                    title={
                      agents.find((a) => a.id === skill.agentId)?.displayName ??
                      skill.agentId
                    }
                  >
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
              <RowActions>
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
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={
                      writing ||
                      toggleLocal.isPending ||
                      collectLocal.isPending ||
                      deleteLocal.isPending
                    }
                    onClick={() =>
                      toggleLocal.mutate({
                        path: skill.path,
                        enabled: !!skill.disabled,
                      })
                    }
                  >
                    {skill.disabled ? (
                      <Play className="h-4 w-4" />
                    ) : (
                      <Square className="h-4 w-4" />
                    )}
                    {skill.disabled ? "启用" : "停用"}
                  </Button>
                )}
                {!skill.managed && (
                  <>
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
                        writing ||
                        collectLocal.isPending ||
                        toggleLocal.isPending ||
                        deleteLocal.isPending
                      }
                      onClick={() => collectLocal.mutate(skill.path)}
                    >
                      <PackagePlus className="h-4 w-4" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      title="删除项目 skill"
                      aria-label={`删除 ${skill.name}`}
                      className="hover:text-destructive"
                      disabled={
                        writing ||
                        collectLocal.isPending ||
                        toggleLocal.isPending ||
                        deleteLocal.isPending
                      }
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
          <h3 className="text-sm font-semibold">agent</h3>
          <ListContainer>
            {projectAgents.map((a, i) => (
              <ListItemRow
                key={a.id}
                isLast={i === projectAgents.length - 1}
                onClick={() => toggleAgent(a.id)}
              >
                <Checkbox
                  checked={draft.value.agentIds.includes(a.id)}
                  aria-label={a.displayName}
                />
                <AgentIcon agentId={a.id} size="sm" />
                <span className="flex-1 text-sm font-medium">
                  {a.displayName}
                </span>
                <span className="font-mono text-[11px] text-muted-foreground">
                  {projectSkillDirectory(a) ?? "由 Agent 提供"}
                </span>
              </ListItemRow>
            ))}
          </ListContainer>
        </section>

        <section className="space-y-2">
          <h3 className="text-sm font-semibold">链接方式</h3>
          <div className="flex items-center gap-3">
            <Select
              value={draft.value.linkMode}
              disabled={writing}
              onValueChange={(v) =>
                draft.edit((previous) => ({
                  ...previous,
                  linkMode: v as LinkMode,
                }))
              }
            >
              <SelectTrigger className="w-48">
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
          <section className="space-y-2">
            <h3 className="text-sm font-semibold">绑定分组</h3>
            <ListContainer>
              {groups.map((g, i) => (
                <ListItemRow
                  key={g.id}
                  isLast={i === groups.length - 1}
                  onClick={() => toggleGroup(g.id)}
                >
                  <Checkbox
                    checked={draft.value.groupIds.includes(g.id)}
                    aria-label={g.name}
                  />
                  <span className="flex-1 text-sm font-medium">{g.name}</span>
                  <Badge variant="outline" className="h-4 px-1.5 text-[10px]">
                    {g.skillIds.length} 个
                  </Badge>
                </ListItemRow>
              ))}
            </ListContainer>
          </section>
        )}

        <section className="space-y-2">
          <h3 className="text-sm font-semibold">单独绑定 skill</h3>
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索 skill…"
          />
          {filteredSkills.length === 0 ? (
            <p className="py-4 text-sm text-muted-foreground">
              没有匹配的 skill
            </p>
          ) : (
            <ListContainer>
              {filteredSkills.map((s, i) => (
                <ListItemRow
                  key={s.id}
                  isLast={i === filteredSkills.length - 1}
                  onClick={() => toggleSkill(s.id)}
                >
                  <Checkbox
                    checked={draft.value.skillIds.includes(s.id)}
                    disabled={
                      !draft.value.skillIds.includes(s.id) && !!variantIssue(s)
                    }
                    aria-label={s.name}
                  />
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium">{s.name}</p>
                    {variantIssue(s) && (
                      <p className="text-xs text-amber-600">
                        {variantIssue(s)}
                      </p>
                    )}
                    {s.description && (
                      <p className="truncate text-xs text-muted-foreground">
                        {s.description}
                      </p>
                    )}
                  </div>
                </ListItemRow>
              ))}
            </ListContainer>
          )}
        </section>
      </div>
      {previewDialog}
      <ConfirmDialog
        open={!!deletingLocal}
        onOpenChange={(open) => {
          if (!open && !deleteLocal.isPending) setDeletingLocal(null);
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
        pending={deleteLocal.isPending}
        onConfirm={() => {
          if (deletingLocal) deleteLocal.mutate(deletingLocal.path);
        }}
      />
    </div>
  );
}

function SortableProjectCard({
  project,
  disabled,
  onOpen,
  children,
}: {
  project: ProjectBinding;
  disabled: boolean;
  /** 点整张卡片时打开的管理界面（项目详情） */
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
  } = useSortable({ id: project.id, disabled });
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
        className={
          projectAgentIds(project).length
            ? "border-emerald-500/40 bg-card bg-gradient-to-r from-emerald-500/10 to-transparent shadow-sm shadow-emerald-500/5 hover:border-emerald-500/60 hover:bg-card dark:from-emerald-500/15"
            : undefined
        }
        onPreview={onOpen}
        previewLabel={onOpen ? `编辑 ${project.name}` : undefined}
      >
        <button
          ref={setActivatorNodeRef}
          type="button"
          {...attributes}
          {...listeners}
          aria-label={`拖拽排序 ${project.name}`}
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
