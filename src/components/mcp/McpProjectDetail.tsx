import { useState } from "react";
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
import { GroupAgentToggles } from "@/components/common/GroupAgentToggles";
import {
  ListContainer,
  ListItemRow,
  RowActions,
} from "@/components/common/ListItemRow";
import { useUnsavedProject } from "@/components/common/NavigationGuard";
import { SourceMark } from "@/components/common/SourceMark";
import type { GroupCardState } from "@/components/groups/groupCardState";
import { untouchedMcpNote } from "@/components/groups/untouched";
import { MemberChecklist } from "@/components/projects/MemberChecklist";
import { ProjectDetailHeader } from "@/components/projects/ProjectDetailHeader";
import { mcpRequest, type McpSource } from "@/lib/api/mcp";
import {
  ownedBindings,
  rows,
  type ManagedBinding,
  type ManagementStatus,
  type McpRemoval,
  type McpRow,
  type McpUntouched,
} from "@/lib/api/mcpManagement";
import {
  foreignNote,
  inProject,
  mcpAgentName,
  mcpAppId,
} from "@/lib/mcpAgents";
import type { ProjectBinding } from "@/types";

/** 启停后的提示：MCP 要在 Agent 里重新加载才生效 */
const RELOAD = "请在 Agent 中重新加载 MCP";
const same = (a: ManagedBinding | McpSource, b: ManagedBinding | McpSource) =>
  a.agent === b.agent &&
  a.path === b.path &&
  a.key === b.key &&
  (a.project ?? null) === (b.project ?? null);

/**
 * MCP 项目详情，与 Skill 项目详情同一个结构：标题栏（各 Agent 启停、保存即写入已启用的
 * Agent）→ 项目里已有的 MCP（可纳入管理）→ 写入方式（Claude 的保存位置）→ 成员（绑定分组 +
 * 单独选择 MCP）。
 */
export function McpProjectDetail({
  project,
  status,
  refetch,
  state,
  busy,
  onToggle,
  onReapply,
  onBack,
}: {
  project: ProjectBinding;
  status: ManagementStatus;
  refetch: () => Promise<unknown>;
  state: GroupCardState;
  busy: boolean;
  onToggle: (agentId: string, on: boolean) => void;
  onReapply: () => void;
  onBack: () => void;
}) {
  const record = status.projects?.[project.id];
  const catalog = rows(status);
  const remote = {
    entryIds: record?.entryIds ?? [],
    groupIds: record?.groupIds ?? [],
    claudeLocal: record?.claudeLocal ?? false,
  };
  const [value, setValue] = useState(remote);
  const [saved, setSaved] = useState(remote);
  const [pinned] = useState(remote);
  const [writing, setWriting] = useState(false);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<{
    row: McpRow;
    binding: ManagedBinding;
  } | null>(null);
  const [removingAll, setRemovingAll] = useState(false);
  const fingerprint = (v: typeof value) =>
    JSON.stringify({
      ...v,
      entryIds: [...v.entryIds].sort(),
      groupIds: [...v.groupIds].sort(),
    });
  const dirty = fingerprint(value) !== fingerprint(saved);
  const locked = writing || busy || !!pending;
  useUnsavedProject(dirty, locked);
  const activeAgents = Object.keys(record?.active ?? {});
  // 这个项目自己写进去的接入，不算「项目里已有的」
  const deployed = Object.values(record?.active ?? {}).flatMap(
    (d) => d.bindings,
  );
  const owned = ownedBindings(status);
  const existing = catalog.flatMap((row) => [
    ...row.entry.bindings
      .filter((b) => inProject(b, project))
      .map((binding) => ({ row, source: binding, hub: binding })),
    ...row.sources
      .filter(
        (s) =>
          inProject({ ...s, project: s.project ?? null }, project) &&
          !row.entry.bindings.some((b) => same(b, s)) &&
          !deployed.some((b) => same(b, s)) &&
          !owned.some((b) => same(b, s)),
      )
      .map((source) => ({ row, source, hub: null })),
  ]);

  async function save() {
    setWriting(true);
    setError(null);
    try {
      const untouched = await mcpRequest<McpUntouched[] | null>("saveProject", {
        projectId: project.id,
        ...value,
        // 选中的原有配置只记下引用，不托管、不改动
        imports: catalog
          .filter((r) => !r.managed && value.entryIds.includes(r.entry.id))
          .map((r) => ({
            id: r.entry.id,
            definition: r.entry.definition,
            sourceIds: r.sources.map((source) => source.id),
          })),
      });
      setSaved(value);
      await refetch();
      const done = activeAgents.length
        ? `已保存并写入 ${activeAgents.map(mcpAgentName).join("、")}，${RELOAD}`
        : "已保存，可在卡片上为需要的 Agent 启用";
      const note = untouchedMcpNote(untouched);
      if (note) toast.warning(`${done}。${note}。`, { duration: 8000 });
      else toast.success(done);
    } catch (e) {
      setError(String(e));
    } finally {
      setWriting(false);
    }
  }
  /** 项目里已有的 MCP：纳入 Hub 管理（保留原位置），或移除 Hub 写在这里的接入 */
  async function adopt(row: McpRow, source?: McpSource, removeId?: string) {
    setPending(source?.id ?? removeId ?? null);
    try {
      await mcpRequest("saveEntry", {
        entry: row.managed
          ? row.entry
          : { ...row.entry, id: crypto.randomUUID() },
        expectedEntry: row.managed ? row.entry : null,
        bindingIds: row.entry.bindings
          .filter((b) => b.id !== removeId)
          .map((b) => b.id),
        sources: source
          ? [{ id: source.id, definition: source.definition }]
          : [],
        agents: [],
        scope: "user",
        projectId: "",
      });
      await refetch();
      setRemoving(null);
      toast.success(
        removeId
          ? "已移除此处接入，Hub 中的 MCP 已保留"
          : "已纳入管理，原配置位置不变",
      );
    } catch (e) {
      toast.error(String(e));
    } finally {
      setPending(null);
    }
  }

  /** 移除 Hub 写在这个项目里的全部接入：被其他程序改写的原样保留，并说明是哪几处 */
  async function removeHub() {
    setPending("all");
    try {
      const result = await mcpRequest<McpRemoval>("removeProjectBindings", {
        projectId: project.id,
      });
      await refetch();
      setRemovingAll(false);
      const done = result.removed
        ? `已移除 ${result.removed} 处 Hub 接入，${RELOAD}。`
        : "";
      if (result.foreign.length)
        toast.warning(`${done}${foreignNote(result.foreign)}。`, {
          duration: 8000,
        });
      else toast.success(done || "已移除托管记录");
    } catch (e) {
      toast.error(String(e));
    } finally {
      setPending(null);
    }
  }
  const hubCount = existing.filter((e) => e.hub).length;

  const toggle = (key: "entryIds" | "groupIds", id: string) =>
    setValue((prev) => ({
      ...prev,
      [key]: prev[key].includes(id)
        ? prev[key].filter((x) => x !== id)
        : [...prev[key], id],
    }));
  const groups = [...(status.groups ?? [])].sort(
    (a, b) => a.sortOrder - b.sortOrder,
  );
  const entryName = (id: string) =>
    catalog.find((r) => r.entry.id === id)?.entry.name;

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
        save={{
          onClick: () => void save(),
          pending: writing,
          disabled: locked || !dirty,
          title: activeAgents.length
            ? `保存并写入 ${activeAgents.map(mcpAgentName).join("、")}`
            : "保存成员；还没有 Agent 启用，不写入项目",
        }}
      />
      {error && (
        <p role="alert" className="text-sm text-destructive">
          写入失败，修改已保留：{error}
        </p>
      )}
      <div className="-mx-1 min-h-0 flex-1 space-y-6 overflow-y-auto px-1 pb-6 pt-1">
        <section className="space-y-2">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-semibold">项目已有 MCP</h3>
            {hubCount > 0 && (
              <Button
                variant="outline"
                size="sm"
                disabled={locked}
                onClick={() => setRemovingAll(true)}
              >
                移除 Hub 接入 ({hubCount})
              </Button>
            )}
          </div>
          <p className="text-xs text-muted-foreground">
            项目配置里已有的 MCP 可纳入 Hub 管理（保留原位置）；卡片上的 Agent
            启停只作用于下面选择的成员。
          </p>
          {!existing.length ? (
            <p className="text-sm text-muted-foreground">项目配置中暂无 MCP</p>
          ) : (
            <ListContainer>
              {existing.map(({ row, source, hub }, i) => (
                <ListItemRow
                  key={`${source.agent}:${source.path}:${source.key}:${source.project ?? ""}`}
                  isLast={i === existing.length - 1}
                >
                  <AgentIcon agentId={mcpAppId(source.agent)} size="sm" />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <p className="truncate text-sm font-medium">
                        {source.key}
                      </p>
                      <Badge variant="outline">
                        {hub ? "Hub 托管" : "原有配置"}
                      </Badge>
                      {source.project && (
                        <Badge variant="outline">项目本地</Badge>
                      )}
                    </div>
                    <p className="break-all font-mono text-[11px] text-muted-foreground">
                      {source.path}
                    </p>
                  </div>
                  <RowActions busy={pending !== null}>
                    {hub ? (
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={locked}
                        aria-label={`移除 ${row.entry.name} 的 ${mcpAgentName(source.agent)} 接入`}
                        onClick={() => setRemoving({ row, binding: hub })}
                      >
                        移除
                      </Button>
                    ) : (
                      !("gateway" in source && source.gateway) && (
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={locked}
                          onClick={() => void adopt(row, source as McpSource)}
                        >
                          纳入管理
                        </Button>
                      )
                    )}
                  </RowActions>
                </ListItemRow>
              ))}
            </ListContainer>
          )}
        </section>

        <section className="space-y-2">
          <h3 className="text-sm font-semibold">Claude Code 保存位置</h3>
          <div className="flex items-center gap-3">
            <Select
              value={value.claudeLocal ? "local" : "project"}
              disabled={locked}
              onValueChange={(v) =>
                setValue((prev) => ({ ...prev, claudeLocal: v === "local" }))
              }
            >
              <SelectTrigger className="w-48" aria-label="Claude Code 保存位置">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="project">项目配置（推荐）</SelectItem>
                <SelectItem value="local">项目本地</SelectItem>
              </SelectContent>
            </Select>
            <p className="text-xs leading-relaxed text-muted-foreground">
              项目配置写进项目里的 .mcp.json，可随项目分享；项目本地写进
              ~/.claude.json 里这个项目的配置，只有自己用。其他 Agent
              都写进各自的项目配置文件。
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
                g.entryIds
                  .map((id) => entryName(id) ?? `已缺失：${id}`)
                  .join(" · ") || "尚未选择 MCP",
              badge: `${g.entryIds.length} 个`,
            }))}
            selected={value.groupIds}
            pinned={pinned.groupIds}
            onToggle={(id) => toggle("groupIds", id)}
            disabled={locked}
            emptyText="还没有 MCP 分组"
          />
        )}

        <MemberChecklist
          title="单独选择 MCP"
          items={[
            ...catalog.map((row) => {
              const target = String(
                row.entry.definition.command ?? row.entry.definition.url ?? "",
              );
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
                marks: [...new Set(row.sources.map((s) => s.agent))].map(
                  (agent) => (
                    <SourceMark
                      key={agent}
                      id={mcpAppId(agent)}
                      label={mcpAgentName(agent)}
                    />
                  ),
                ),
                disabledReason:
                  !row.managed && row.sources.some((s) => s.gateway)
                    ? "指向 Studio 网关的配置不能作为成员，请先在 MCP Hub 添加独立配置"
                    : undefined,
              };
            }),
            ...value.entryIds
              .filter((id) => !entryName(id))
              .map((id) => ({ id, name: `已缺失：${id}` })),
          ]}
          selected={value.entryIds}
          pinned={pinned.entryIds}
          onToggle={(id) => toggle("entryIds", id)}
          disabled={locked}
          searchPlaceholder="搜索 MCP…"
          emptyText="MCP Hub 暂无服务，请先添加 MCP。"
        />
      </div>
      <ConfirmDialog
        open={removingAll}
        onOpenChange={(next) => !next && !pending && setRemovingAll(false)}
        title={`移除 ${project.name} 的 Hub 接入？`}
        description="还原此项目里由 MCP Hub 直接写入的接入，保留 Hub 服务、原有手动配置和这个项目自己的成员部署。已被其他程序或手动改过的接入原样保留，不会覆盖修改。"
        confirmText="移除 Hub 接入"
        pending={pending === "all"}
        onConfirm={() => void removeHub()}
      />
      <ConfirmDialog
        open={!!removing}
        onOpenChange={(next) => !next && !pending && setRemoving(null)}
        title={`移除「${removing?.row.entry.name ?? ""}」的接入`}
        description="只移除此位置的 MCP 配置，Hub 中的服务和其他位置的接入会保留。"
        confirmText="移除"
        pending={!!pending}
        onConfirm={() =>
          removing && void adopt(removing.row, undefined, removing.binding.id)
        }
      />
    </div>
  );
}
