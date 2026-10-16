import { MCP_AGENTS, mcpAgentName, inProject } from "@/lib/mcpAgents";
import { McpAgentNote } from "./McpAgentNote";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { useUnsavedProject } from "@/components/common/NavigationGuard";
import { useTarget } from "@/components/targets/TargetProvider";
import {
  managementApi,
  MCP_STATUS_STALE_TIME,
  MCP_STATUS_POLL_INTERVAL,
  rows,
  type ManagedBinding,
  type McpRow,
} from "@/lib/api/mcpManagement";
import { mcpRequest, type McpSource } from "@/lib/api/mcp";

type Project = { id: string; name: string; root: string };
type Props = { project: Project };

/**
 * 项目里的 MCP 接入。用户全局的接入在 MCP Hub 卡片的「部署到」图标上管理，
 * 分组部署的在分组页管理。
 */
export function McpAssignments(props: Props) {
  const target = useTarget();
  return target.id === "local" ? (
    <LocalAssignments {...props} />
  ) : (
    <p className="text-sm text-muted-foreground">MCP 管理当前仅支持本机。</p>
  );
}
function LocalAssignments({ project }: Props) {
  const status = useQuery({
    queryKey: ["mcp", "local"],
    queryFn: managementApi.list,
    // Keep polling in the background without fetching again on every Agent switch.
    staleTime: MCP_STATUS_STALE_TIME,
    refetchInterval: MCP_STATUS_POLL_INTERVAL,
  });
  const [open, setOpen] = useState(false);
  const [selected, setSelected] = useState("");
  const [selectedAgent, setSelectedAgent] = useState("claude");
  const [scope, setScope] = useState<"project" | "local">("project");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<{
    row: McpRow;
    binding: ManagedBinding;
  } | null>(null);
  useUnsavedProject(false, busy);
  const catalog = rows(status.data);
  const visible = catalog
    .map((row) => ({
      row,
      bindings: row.entry.bindings.filter((b) => inProject(b, project)),
      sources: row.sources.filter((s) =>
        inProject({ ...s, project: s.project ?? null }, project),
      ),
    }))
    .filter((v) => v.bindings.length || v.sources.length);
  const available = catalog.filter((r) => r.managed);
  const chosen = available.find((r) => r.entry.id === selected);
  const alreadyConfigured =
    !!chosen &&
    visible.some(
      (v) =>
        v.row.entry.id === chosen.entry.id &&
        (v.bindings.some(
          (b) =>
            b.agent === selectedAgent &&
            (scope === "local" ? !!b.project : !b.project),
        ) ||
          v.sources.some(
            (s) =>
              s.agent === selectedAgent &&
              (scope === "local" ? !!s.project : !s.project),
          )),
    );
  const groupSource = (source: McpSource) =>
    Object.values(status.data?.activeGroups ?? {}).some((groups) =>
      groups.some((g) =>
        g.bindings.some(
          (b) =>
            b.agent === source.agent &&
            b.path === source.path &&
            b.key === source.key &&
            b.project === (source.project ?? null),
        ),
      ),
    );
  async function save(row: McpRow, removeId?: string, source?: McpSource) {
    setBusy(true);
    setError(null);
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
        agents: removeId || source ? [] : [selectedAgent],
        scope: removeId || source ? "user" : scope,
        projectId: removeId || source ? "" : project.id,
      });
      await status.refetch();
      setOpen(false);
      setRemoving(null);
      toast.success(
        removeId
          ? "已移除此处接入，Hub 中的 MCP 已保留"
          : "已添加，请在 Agent 中重新加载 MCP",
      );
    } catch (e) {
      setError(String(e));
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="space-y-3">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold">项目 MCP</h3>
          <p className="mt-1 text-xs text-muted-foreground">
            从 MCP Hub 选择服务，配置仅在此项目生效。MCP 单独管理，不随 Skill
            的写入或项目开关变化。
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={busy || !status.data || !!status.error}
          onClick={() => {
            setSelected("");
            setSelectedAgent("claude");
            setScope("project");
            setError(null);
            setOpen(true);
          }}
        >
          <Plus className="h-4 w-4" />
          添加 MCP
        </Button>
      </div>
      {status.isPending && (
        <p className="text-sm text-muted-foreground">正在读取 MCP…</p>
      )}
      {(status.error || error) && (
        <p role="alert" className="text-sm text-destructive">
          {String(status.error ?? error)}
        </p>
      )}
      {!status.isPending && !status.error && !visible.length && (
        <p className="rounded-xl border border-dashed p-6 text-center text-sm text-muted-foreground">
          此项目还没有 MCP。从 Hub 添加后即可使用。
        </p>
      )}
      {visible.map(({ row, bindings, sources }) => (
        <div
          key={row.entry.id}
          className="space-y-2 rounded-xl border border-border-default bg-card p-4"
        >
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium">{row.entry.name}</span>
            <Badge variant="outline">
              {row.entry.mode === "gateway" ? "Studio 网关" : "Agent 直连"}
            </Badge>
          </div>
          {row.entry.mode === "gateway" && (
            <p className="text-xs text-muted-foreground">
              使用前请在 MCP Hub 开启网关；需要登录时，在连接详情中授权。
            </p>
          )}
          {bindings.map((binding) => (
            <div
              key={binding.id}
              className="flex items-center justify-between gap-3"
            >
              <div className="min-w-0">
                <p className="text-xs">
                  {mcpAgentName(binding.agent)} ·{" "}
                  {binding.project ? "项目本地" : "项目配置"}
                </p>
                <p className="break-all text-xs text-muted-foreground">
                  {binding.path}
                </p>
              </div>
              <Button
                variant="ghost"
                size="sm"
                disabled={busy}
                aria-label={`移除 ${row.entry.name} 的 ${binding.agent} 接入`}
                onClick={() => setRemoving({ row, binding })}
              >
                <Trash2 className="h-4 w-4" />
                移除
              </Button>
            </div>
          ))}
          {sources
            .filter(
              (s) =>
                !bindings.some(
                  (b) =>
                    b.path === s.path &&
                    b.agent === s.agent &&
                    b.key === s.key &&
                    b.project === (s.project ?? null),
                ),
            )
            .map((source) => (
              <div key={source.id} className="text-xs text-muted-foreground">
                <p>
                  {mcpAgentName(source.agent)} ·{" "}
                  {source.enabled ? "已配置" : "已停用"} ·{" "}
                  {groupSource(source) ? "由分组管理" : "原有配置"}
                </p>
                <p className="break-all">{source.path}</p>
                {!source.gateway && !groupSource(source) && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="mt-2"
                    disabled={busy}
                    onClick={() => void save(row, undefined, source)}
                  >
                    纳入管理
                  </Button>
                )}
              </div>
            ))}
        </div>
      ))}
      <Dialog open={open} onOpenChange={(next) => !busy && setOpen(next)}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>{`添加 MCP 到 ${project.name}`}</DialogTitle>
            <DialogDescription>
              从 MCP Hub 选择已保存的服务。暂无服务时，请先在 MCP Hub 添加。
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 px-6 pb-4">
            <McpAgentNote agent={selectedAgent} />
            <label className="block space-y-2 text-sm">
              <span>Agent</span>
              <select
                aria-label="MCP Agent"
                className="w-full rounded-md border bg-background p-2"
                disabled={busy}
                value={selectedAgent}
                onChange={(e) => {
                  setSelectedAgent(e.target.value);
                  setScope("project");
                }}
              >
                {MCP_AGENTS.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="block space-y-2 text-sm">
              <span>保存位置</span>
              <select
                aria-label="MCP 保存位置"
                className="w-full rounded-md border bg-background p-2"
                disabled={busy}
                value={scope}
                onChange={(e) => setScope(e.target.value as typeof scope)}
              >
                <option value="project">项目配置 · 可随项目分享</option>
                {selectedAgent === "claude" && (
                  <option value="local">项目本地 · 仅自己使用</option>
                )}
              </select>
            </label>
            <label className="block space-y-2 text-sm">
              <span>MCP Hub 中的服务</span>
              <select
                aria-label="选择 Hub MCP"
                className="w-full rounded-md border bg-background p-2"
                disabled={busy}
                value={selected}
                onChange={(e) => setSelected(e.target.value)}
              >
                <option value="">请选择 MCP</option>
                {available.map((r) => (
                  <option key={r.entry.id} value={r.entry.id}>
                    {r.entry.name}
                  </option>
                ))}
              </select>
            </label>
            {alreadyConfigured && (
              <p className="text-sm text-muted-foreground">
                此位置已有该 MCP，无需重复添加。
              </p>
            )}
            {error && (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            )}
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => setOpen(false)}
            >
              取消
            </Button>
            <Button
              disabled={busy || !chosen || alreadyConfigured || !!status.error}
              onClick={() => chosen && void save(chosen)}
            >
              {busy ? "正在添加…" : "添加并启用"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        open={!!removing}
        onOpenChange={(next) => !next && !busy && setRemoving(null)}
        title={`移除「${removing?.row.entry.name ?? ""}」的接入`}
        description="只移除此位置的 MCP 配置，Hub 中的服务和其他位置的接入会保留。"
        confirmText="移除"
        pending={busy}
        onConfirm={() =>
          removing && void save(removing.row, removing.binding.id)
        }
      />
    </section>
  );
}
