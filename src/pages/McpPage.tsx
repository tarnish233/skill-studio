import { usePageReset } from "@/components/common/PageReset";
import {
  SummaryAction,
  SummaryBar,
  SummaryEnd,
  SummaryFilter,
  SummaryToggle,
  agentOption,
  summaryPillClass,
} from "@/components/common/SummaryBar";
import {
  MCP_AGENTS,
  foreignNote,
  mcpAgentName,
  mcpAppId,
} from "@/lib/mcpAgents";
import { useSettings } from "@/hooks/useData";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  ShieldCheck,
  Plug,
  Power,
  PackagePlus,
  PackageMinus,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  NavigationGuard,
  useNavigationGuard,
} from "@/components/common/NavigationGuard";
import { PageTools } from "@/components/common/PageTools";
import {
  ListContainer,
  ListItemRow,
  RowActions,
} from "@/components/common/ListItemRow";
import { EmptyState } from "@/components/common/EmptyState";
import { AgentIcon } from "@/components/common/AgentIcon";
import { useTarget } from "@/components/targets/TargetProvider";
import {
  DetailTransition,
  useDetailAppear,
} from "@/components/common/DetailTransition";
import {
  McpEntryDetail,
  McpStatusBadges,
  type McpTestState,
} from "@/components/mcp/McpEntryDetail";
import { McpQuickInstall } from "@/components/mcp/McpQuickInstall";
import { McpBackupsDialog } from "@/components/mcp/McpBackupsDialog";
import { RemoteMcpPage } from "@/pages/RemoteMcpPage";
import { mcpRequest } from "@/lib/api/mcp";
import {
  managementApi,
  MCP_STATUS_STALE_TIME,
  MCP_STATUS_POLL_INTERVAL,
  rows,
  type McpRow,
  type ManagedMcp,
  type McpInstallResult,
  type McpRemoval,
} from "@/lib/api/mcpManagement";

/**
 * MCP Hub：点卡片在页内展开这个服务的整页（连接、接入位置、可直接编辑的服务配置），
 * 「添加 MCP」同样在页内展开；标题栏与项目、分组详情同一套。
 */
export function McpPage() {
  const target = useTarget();
  if (target.id !== "local") return <RemoteMcpPage key={target.id} />;
  return (
    <NavigationGuard>
      <LocalMcpPage />
    </NavigationGuard>
  );
}
function LocalMcpPage() {
  const requestNavigation = useNavigationGuard();
  const { data: settings } = useSettings();
  const visibleMcpAgents = MCP_AGENTS.filter(
    (a) => !settings?.disabledAgents?.includes(a.appId),
  );
  const status = useQuery({
    queryKey: ["mcp", "local"],
    queryFn: managementApi.list,
    // Keep polling in the background without fetching again on every Agent switch.
    staleTime: MCP_STATUS_STALE_TIME,
    refetchInterval: MCP_STATUS_POLL_INTERVAL,
  });
  const allRows = rows(status.data);
  const catalog = allRows.filter(
    (row) =>
      row.managed ||
      !row.sources.length ||
      row.sources.some(
        (source) => !settings?.disabledAgents?.includes(mcpAppId(source.agent)),
      ),
  );
  const running = status.data?.running ?? false;
  const [query, setQuery] = useState("");
  /** 左侧筛选：某个 Agent 或「网关连接」；null 是「全部」 */
  const [filter, setFilter] = useState<string | null>(null);
  const activeFilter =
    filter && settings?.disabledAgents?.includes(mcpAppId(filter))
      ? null
      : filter;
  useEffect(() => {
    if (filter && settings?.disabledAgents?.includes(mcpAppId(filter)))
      setFilter(null);
  }, [filter, settings?.disabledAgents]);
  /** 右侧开关：只看已托管的，和左侧筛选叠加 */
  const [managedOnly, setManagedOnly] = useState(false);
  /** 展开的整页：id 为 null 是「添加 MCP」；key 区分每次打开 */
  const [detail, setDetail] = useState<{
    key: number;
    id: string | null;
  } | null>(null);
  const sessions = useRef(0);
  usePageReset(() => setDetail(null));
  const appear = useDetailAppear();
  const open = (id: string | null) =>
    requestNavigation(() => setDetail({ key: ++sessions.current, id }));
  /** 操作完成后转到某个服务的整页；已经在这一页就不动 */
  const show = (id: string) =>
    setDetail((d) => (d?.id === id ? d : { key: ++sessions.current, id }));
  const [remove, setRemove] = useState<McpRow | null>(null);
  const [adopt, setAdopt] = useState<McpRow | null>(null);
  const [restore, setRestore] = useState(true);
  const [busy, setBusy] = useState(false);
  const [pendingLogin, setPendingLogin] = useState<{
    entry: ManagedMcp;
    flowId: string;
  } | null>(null);
  const [loginUrl, setLoginUrl] = useState<string | null>(null);
  const [tools, setTools] = useState<
    { name: string; description?: string }[] | null
  >(null);
  const [authRequired, setAuthRequired] = useState<string[]>([]);
  const [lastTest, setLastTest] = useState<
    Record<
      string,
      { status: "ok" | "auth" | "error"; at: number; signature: string }
    >
  >({});
  const signature = (entry: ManagedMcp) =>
    JSON.stringify({ mode: entry.mode, definition: entry.definition });
  const [backupsOpen, setBackupsOpen] = useState(false);
  const backups = [...(status.data?.backups ?? [])].reverse();
  const detailRow = detail?.id
    ? catalog.find((r) => r.entry.id === detail.id)
    : undefined;
  /** 只认对当前配置做的测试：改过配置后旧结果不再显示 */
  const testFor = (row: McpRow): McpTestState | undefined =>
    lastTest[row.entry.id]?.signature === signature(row.entry)
      ? lastTest[row.entry.id]
      : undefined;
  const hasAgent = (row: McpRow, agent: string) =>
    row.entry.bindings.some((b) => b.agent === agent) ||
    row.sources.some((s) => s.agent === agent);
  const matches = (row: McpRow, f: string | null) =>
    !f ||
    (f === "gateway"
      ? row.managed && row.entry.mode === "gateway"
      : hasAgent(row, f));
  const managedRows = catalog.filter((row) => row.managed);
  // 左侧的数字和列表同一口径：开着「已托管」时只数托管的
  const counted = managedOnly ? managedRows : catalog;
  const count = (f: string) => counted.filter((row) => matches(row, f)).length;
  const filtered = counted.filter(
    (row) =>
      matches(row, activeFilter) &&
      `${row.entry.name} ${row.entry.definition.command ?? ""} ${row.entry.definition.url ?? ""} ${row.sources.map((s) => s.scope).join(" ")}`
        .toLowerCase()
        .includes(query.trim().toLowerCase()),
  );
  const builtinServices = (
    settings?.showCodexBuiltinMcp && !settings.disabledAgents?.includes("codex")
      ? (status.data?.builtins ?? [])
      : []
  ).filter(
    (service) =>
      // 内置服务不归 Studio 托管
      !managedOnly &&
      (!activeFilter || activeFilter === "codex") &&
      `${service.name} ${service.scope} ${service.path}`
        .toLowerCase()
        .includes(query.trim().toLowerCase()),
  );
  async function run(fn: () => Promise<void>) {
    setBusy(true);
    try {
      await fn();
      await status.refetch();
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function runtime(entry: ManagedMcp, method: string) {
    if (entry.mode === "gateway" && !running) await mcpRequest("start");
    if (method === "test") {
      setTools(null);
      let result:
        { name: string; description?: string }[] | { authRequired: true };
      try {
        result = await mcpRequest<
          { name: string; description?: string }[] | { authRequired: true }
        >("test", { id: entry.id });
      } catch (error) {
        setLastTest((tests) => ({
          ...tests,
          [entry.id]: {
            status: "error",
            at: Date.now(),
            signature: signature(entry),
          },
        }));
        throw error;
      }
      if (!Array.isArray(result)) {
        setLastTest((tests) => ({
          ...tests,
          [entry.id]: {
            status: "auth",
            at: Date.now(),
            signature: signature(entry),
          },
        }));
        setAuthRequired((ids) => [...new Set([...ids, entry.id])]);
        toast.info("此服务需要登录，请点击登录授权后重新测试连接");
      } else {
        setAuthRequired((ids) => ids.filter((id) => id !== entry.id));
        setLastTest((tests) => ({
          ...tests,
          [entry.id]: {
            status: "ok",
            at: Date.now(),
            signature: signature(entry),
          },
        }));
        setTools(result);
      }
    } else {
      const result = await mcpRequest<{ url: string; flowId: string }>(
        "login",
        {
          id: entry.id,
        },
      );
      setLoginUrl(result.url);
      if (result.flowId) {
        setPendingLogin({ entry, flowId: result.flowId });
      } else {
        toast.warning("当前网关版本不支持自动返回，请关闭再开启网关后重新授权");
      }
    }
  }
  async function testDirect(entry: ManagedMcp) {
    setTools(null);
    try {
      const result = await mcpRequest<
        { name: string; description?: string }[] | { authRequired: true }
      >("testDirect", { id: entry.id });
      if (!Array.isArray(result)) {
        setLastTest((tests) => ({
          ...tests,
          [entry.id]: {
            status: "auth",
            at: Date.now(),
            signature: signature(entry),
          },
        }));
        toast.info("服务需要认证，请在 Agent 中完成登录后验证");
        return;
      }
      setLastTest((tests) => ({
        ...tests,
        [entry.id]: {
          status: "ok",
          at: Date.now(),
          signature: signature(entry),
        },
      }));
      setTools(result);
    } catch (error) {
      setLastTest((tests) => ({
        ...tests,
        [entry.id]: {
          status: "error",
          at: Date.now(),
          signature: signature(entry),
        },
      }));
      throw error;
    }
  }
  useEffect(() => {
    if (!pendingLogin) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const check = async () => {
      try {
        const result = await mcpRequest<{
          status: "pending" | "complete" | "expired";
        }>("loginStatus", {
          id: pendingLogin.entry.id,
          flowId: pendingLogin.flowId,
        });
        if (cancelled) return;
        if (result.status === "complete") {
          setPendingLogin(null);
          setLoginUrl(null);
          setAuthRequired((ids) =>
            ids.filter((id) => id !== pendingLogin.entry.id),
          );
          show(pendingLogin.entry.id);
          toast.success("登录成功，正在测试连接");
          void run(() => runtime(pendingLogin.entry, "test"));
          return;
        }
        if (result.status === "expired") {
          setPendingLogin(null);
          setLoginUrl(null);
          toast.error("授权已过期或已取消，请重新登录");
          return;
        }
      } catch {
        // Keep waiting across a transient gateway failure; the flow expires below.
      }
      if (!cancelled) timer = setTimeout(check, 1000);
    };
    timer = setTimeout(check, 1000);
    const expiry = setTimeout(() => {
      cancelled = true;
      clearTimeout(timer);
      setPendingLogin(null);
      setLoginUrl(null);
      toast.error("等待授权超时，请重新登录");
    }, 300_000);
    return () => {
      cancelled = true;
      clearTimeout(timer);
      clearTimeout(expiry);
    };
    // The monitor belongs to this specific login attempt, not render-time callbacks.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingLogin]);
  /** 保存之后转到（或留在）这个服务的整页：新添加的可以接着测试、登录 */
  async function saved(entry: ManagedMcp, result?: McpInstallResult) {
    // 编辑的是 Hub 里已有的这一个；否则是新添加的，或从原有配置另存的
    const edited = detail?.id === entry.id && !!detailRow?.managed;
    if (!edited) {
      setQuery("");
      setFilter(null);
    }
    await status.refetch();
    show(entry.id);
    if (result?.setupError)
      toast.warning(`配置已保存，连接设置尚未完成：${result.setupError}`);
    else if (result?.loginUrl) {
      toast.success("已安装，请在浏览器完成登录");
      setLoginUrl(result.loginUrl);
    } else if (!edited) toast.success("已添加到 MCP Hub");
    else
      toast.success(
        result?.installed === false
          ? "已保存到 MCP Hub"
          : entry.mode === "gateway" && !running && !result?.gatewayStarted
            ? "已保存并同步接入，使用前请开启网关"
            : "已保存并同步接入，请在 Agent 中重新加载 MCP",
      );
  }

  const dialogs = (
    <>
      <ConfirmDialog
        open={!!adopt}
        onOpenChange={(open) => !open && !busy && setAdopt(null)}
        title="托管到 Hub"
        confirmText="托管"
        variant="info"
        pending={busy}
        description={`将「${adopt?.entry.name ?? ""}」保存到 MCP Hub，并管理它的 ${adopt?.sources.length ?? 0} 处原有接入。配置文件会先备份，之后可移出 Hub 并还原。`}
        onConfirm={() =>
          adopt &&
          void run(async () => {
            const id = crypto.randomUUID();
            await mcpRequest("saveEntry", {
              entry: { ...adopt.entry, id },
              expectedEntry: null,
              bindingIds: [],
              sources: adopt.sources.map((source) => ({
                id: source.id,
                definition: source.definition,
              })),
              agents: [],
              scope: "user",
              projectId: "",
            });
            setAdopt(null);
            setFilter(null);
            if (detail?.id === adopt.entry.id) {
              await status.refetch();
              show(id);
            }
            toast.success("已托管到 MCP Hub");
          })
        }
      />
      <ConfirmDialog
        open={!!remove}
        onOpenChange={(open) => !open && !busy && setRemove(null)}
        title={
          restore
            ? `移出 Hub 并还原 ${remove?.entry.name ?? ""}？`
            : `删除 ${remove?.entry.name ?? ""}？`
        }
        confirmText={restore ? "还原" : "删除"}
        variant={restore ? "info" : "destructive"}
        pending={busy}
        description={
          <>
            <span className="block">
              {restore
                ? "移除 Hub 托管记录，恢复托管前的原始配置；由 Studio 新建的接入会移除。已被其他程序改写的接入原样保留。"
                : remove?.managed
                  ? "删除 Hub 中的服务和全部受托管接入。扫描到但未托管的原有配置，以及已被其他程序改写的接入原样保留。"
                  : `删除扫描到的 ${remove?.sources.length ?? 0} 处原有 MCP 配置。`}
            </span>
            <span className="mt-2 block">
              {remove?.managed
                ? "配置文件会先备份。仍被启用的分组使用时，操作会停止。"
                : "配置文件会先备份。配置被外部修改或仍被启用的分组使用时，操作会停止。"}
            </span>
            <span className="mt-2 block space-y-1 break-all text-xs">
              {(remove?.managed
                ? remove.entry.bindings
                : (remove?.sources ?? [])
              ).map((source) => (
                <span className="block" key={source.id}>
                  {mcpAgentName(source.agent)} · {source.path} · {source.key}
                </span>
              ))}
            </span>
          </>
        }
        onConfirm={() =>
          remove &&
          void run(async () => {
            let removal: McpRemoval | null = null;
            if (remove.managed)
              removal = await mcpRequest<McpRemoval>("removeEntry", {
                id: remove.entry.id,
                restore,
              });
            else
              await mcpRequest("removeSources", {
                entry: remove.entry,
                sources: remove.sources.map((source) => ({
                  id: source.id,
                  definition: source.definition,
                })),
              });
            setRemove(null);
            // 接入全被其他程序改写时一个配置文件都没写，也就没有备份可说
            const wrote = !removal || removal.removed > 0;
            const done = restore
              ? wrote
                ? "已移出 Hub 并还原"
                : "已移出 Hub"
              : wrote
                ? "MCP 已删除，配置文件已备份"
                : "MCP 已从 Hub 删除";
            if (removal?.foreign.length)
              toast.warning(`${done}。${foreignNote(removal.foreign)}。`, {
                duration: 8000,
              });
            else toast.success(done);
          })
        }
      />
      <McpBackupsDialog
        open={backupsOpen}
        onOpenChange={setBackupsOpen}
        onChanged={() => status.refetch()}
      />
      <Dialog
        open={!!loginUrl}
        onOpenChange={(open) => !open && setLoginUrl(null)}
      >
        <DialogContent className="gap-3 p-6">
          <DialogHeader>
            <DialogTitle>完成浏览器授权</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-muted-foreground">
            请在浏览器中完成授权。浏览器未打开时，可复制此临时链接：
          </p>
          <Input aria-label="授权链接" readOnly value={loginUrl ?? ""} />
        </DialogContent>
      </Dialog>
      <Dialog
        open={tools !== null}
        onOpenChange={(open) => !open && setTools(null)}
      >
        <DialogContent className="gap-3 p-6">
          <DialogHeader>
            <DialogTitle>连接成功 · {tools?.length ?? 0} 个工具</DialogTitle>
          </DialogHeader>
          <div className="max-h-80 space-y-3 overflow-auto">
            {tools?.map((tool) => (
              <div key={tool.name}>
                <p className="text-sm font-medium">{tool.name}</p>
                <p className="text-xs text-muted-foreground">
                  {tool.description}
                </p>
              </div>
            ))}
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
  const pageTools = (
    <PageTools
      query={query}
      onQueryChange={(v) =>
        requestNavigation(() => {
          setQuery(v);
          setDetail(null);
        })
      }
      placeholder="按名称或地址搜索 MCP…"
      createLabel="添加 MCP"
      onCreate={() => open(null)}
    />
  );
  const back = () => requestNavigation(() => setDetail(null));
  if (detail && (detail.id === null || detailRow))
    return (
      <>
        {pageTools}
        <DetailTransition key={detail.key} appear={appear}>
          {detailRow ? (
            <McpEntryDetail
              row={detailRow}
              running={running}
              busy={busy}
              authPending={authRequired.includes(detailRow.entry.id)}
              test={testFor(detailRow)}
              onBack={back}
              onTest={() =>
                void run(() =>
                  detailRow.entry.mode === "gateway"
                    ? runtime(detailRow.entry, "test")
                    : testDirect(detailRow.entry),
                )
              }
              onLogin={() => void run(() => runtime(detailRow.entry, "login"))}
              onLogout={() =>
                void run(async () => {
                  await mcpRequest("logout", { id: detailRow.entry.id });
                  toast.success("已清除本地授权");
                })
              }
              onAdopt={() => setAdopt(detailRow)}
              onSaved={(entry, result) => void saved(entry, result)}
            />
          ) : (
            <McpQuickInstall
              onBack={back}
              onSaved={(entry, result) => void saved(entry, result)}
            />
          )}
        </DetailTransition>
        {dialogs}
      </>
    );
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {pageTools}
      <DetailTransition appear={appear}>
        <SummaryBar>
          <SummaryFilter
            label="筛选 MCP"
            total={counted.length}
            options={[
              ...visibleMcpAgents.map((a) =>
                agentOption({
                  id: a.id,
                  label: a.name,
                  iconId: a.appId,
                  count: count(a.id),
                  title: `筛选 ${a.name} 已配置的 MCP`,
                }),
              ),
              {
                id: "gateway",
                label: "网关连接",
                count: count("gateway"),
                title: "网关连接：托管后经 Studio 网关连接的 MCP",
              },
            ]}
            selected={activeFilter}
            onSelect={setFilter}
          />
          <SummaryEnd>
            <SummaryToggle
              pressed={managedOnly}
              onClick={() => setManagedOnly((v) => !v)}
              title={`已托管：已纳入 Studio 托管的 MCP，共 ${managedRows.length} 个。点击只看这些，可与左侧的筛选叠加。`}
            >
              已托管 {managedRows.length}
            </SummaryToggle>
            <SummaryAction
              onClick={() => setBackupsOpen(true)}
              title={`已备份：Studio 每次写入 MCP 配置前留下的备份，共 ${backups.length} 次。点击查看，可恢复或删除`}
            >
              已备份 {backups.length}
            </SummaryAction>
            <Button
              variant="ghost"
              size="sm"
              className={`${summaryPillClass} border-transparent`}
              aria-label="MCP 网关"
              aria-pressed={running}
              title={running ? "点击关闭网关" : "点击开启网关"}
              disabled={busy || status.isLoading || !!status.error}
              onClick={() =>
                void run(async () => {
                  await mcpRequest(running ? "stop" : "start");
                  toast.success(running ? "网关已关闭" : "网关已开启");
                })
              }
            >
              <span
                className={`h-2 w-2 rounded-full ${running ? "bg-emerald-500" : "bg-muted-foreground"}`}
              />
              {running ? "网关已开启" : "网关已关闭"}
              <Power className="h-4 w-4" />
            </Button>
          </SummaryEnd>
        </SummaryBar>
        {status.data?.gatewayOutdated && (
          <p
            role="status"
            className="mb-3 text-sm text-amber-600 dark:text-amber-400"
          >
            后台网关仍是旧版本，请关闭后重新开启，以应用新的连接配置。直连管理不受影响。
          </p>
        )}
        {status.isLoading && (
          <p className="py-6 text-sm text-muted-foreground">
            正在扫描 MCP 配置…
          </p>
        )}
        {status.error && (
          <p role="alert" className="text-destructive">
            {String(status.error)}
          </p>
        )}
        {!!status.data?.scanWarnings?.length && (
          <details className="mb-3 rounded-lg border border-amber-500/30 p-3 text-sm">
            <summary className="cursor-pointer">
              部分配置未能扫描（{status.data.scanWarnings.length}）
            </summary>
            {status.data.scanWarnings.map((warning) => (
              <p
                key={warning}
                className="mt-2 break-all text-xs text-muted-foreground"
              >
                {warning}
              </p>
            ))}
          </details>
        )}
        <div className="min-h-0 flex-1 overflow-y-auto pb-6">
          {!status.isLoading &&
            !status.error &&
            !catalog.length &&
            !builtinServices.length && (
              <EmptyState
                icon={Plug}
                title={
                  allRows.length
                    ? "当前 Agent 选择下没有可显示的 MCP"
                    : "MCP Hub 还没有发现服务"
                }
                description={
                  allRows.length
                    ? "可在设置中开启其他 Agent，或添加服务到 Hub。"
                    : "已扫描支持的 Agent 全局和已登记项目配置。点击添加 MCP，粘贴安装命令、网址或配置即可添加到 Hub，再到 Agent 或项目页面启用。"
                }
              />
            )}
          <ListContainer cards>
            {filtered.map((row) => (
              <ListItemRow
                key={row.entry.id}
                card
                onPreview={() => open(row.entry.id)}
                previewLabel={`编辑 ${row.entry.name}`}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="truncate text-sm font-medium">
                      {row.entry.name}
                    </span>
                    <McpStatusBadges
                      row={row}
                      test={testFor(row)}
                      authPending={authRequired.includes(row.entry.id)}
                      running={running}
                    />
                    {visibleMcpAgents
                      .map((a) => a.id)
                      .filter((a) => hasAgent(row, a))
                      .map((agent) => (
                        <span
                          key={agent}
                          title={`${mcpAgentName(agent)} 中已配置`}
                        >
                          <AgentIcon agentId={mcpAppId(agent)} size="sm" />
                        </span>
                      ))}
                  </div>
                  <p className="truncate pt-0.5 text-xs text-muted-foreground">
                    {String(
                      row.entry.definition.url ??
                        row.entry.definition.command ??
                        "点击查看配置来源",
                    )}
                  </p>
                </div>
                <RowActions busy={busy}>
                  {row.managed ? (
                    <>
                      {row.entry.bindings.some(
                        (binding) => binding.original !== null,
                      ) && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8"
                          disabled={busy}
                          title="移出 Hub 并还原"
                          aria-label={`还原 ${row.entry.name} 到原位置`}
                          onClick={() => {
                            setRemove(row);
                            setRestore(true);
                          }}
                        >
                          <PackageMinus className="h-4 w-4" />
                        </Button>
                      )}
                    </>
                  ) : (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8"
                      disabled={
                        busy || row.sources.some((source) => source.gateway)
                      }
                      title="托管到 Hub"
                      aria-label={`托管 ${row.entry.name} 到 Hub`}
                      onClick={() => setAdopt(row)}
                    >
                      <PackagePlus className="h-4 w-4" />
                    </Button>
                  )}
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8"
                    title="删除 MCP"
                    aria-label={`删除 ${row.entry.name}`}
                    disabled={
                      busy ||
                      (!row.managed &&
                        row.sources.some((source) => source.gateway))
                    }
                    onClick={() => {
                      setRemove(row);
                      setRestore(false);
                    }}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </RowActions>
              </ListItemRow>
            ))}
            {builtinServices.map((service) => (
              <ListItemRow
                key={`builtin:${service.path}:${service.scope}:${service.name}`}
                card
                className="cursor-default bg-muted/40 text-muted-foreground hover:bg-muted/40"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="truncate text-sm font-medium">
                      {service.name}
                    </span>
                    <Badge
                      variant="outline"
                      className="gap-1 border-border-default bg-muted text-muted-foreground"
                    >
                      <ShieldCheck className="h-4 w-4" />
                      Codex 内置
                    </Badge>
                    <Badge variant="outline" className="text-muted-foreground">
                      只读
                    </Badge>
                    <AgentIcon
                      agentId="codex"
                      size="sm"
                      className="grayscale opacity-60"
                    />
                  </div>
                  <p className="pt-0.5 text-xs">
                    由 Codex App 管理 · 不支持操作 · 不计入 MCP 数量
                  </p>
                  <p className="truncate pt-0.5 text-xs" title={service.path}>
                    {service.scope} · {service.path}
                  </p>
                </div>
              </ListItemRow>
            ))}
          </ListContainer>
          {!!catalog.length && !filtered.length && !builtinServices.length && (
            <p className="py-6 text-center text-sm text-muted-foreground">
              没有匹配的 MCP
            </p>
          )}
        </div>
      </DetailTransition>
      {dialogs}
    </div>
  );
}
