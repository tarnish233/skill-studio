import { useEffect, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  FolderOpen,
  Link2,
  Package,
  Power,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { AgentIcon } from "@/components/common/AgentIcon";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { DetailHeader } from "@/components/common/DetailHeader";
import { EmptyState } from "@/components/common/EmptyState";
import {
  ListContainer,
  ListItemRow,
  RowActions,
} from "@/components/common/ListItemRow";
import { PageTools } from "@/components/common/PageTools";
import { usePageReset } from "@/components/common/PageReset";
import { SOURCE_LABELS } from "@/components/common/SourceMark";
import {
  SummaryBar,
  SummaryEnd,
  SummaryFilter,
  SummaryStat,
  agentOption,
} from "@/components/common/SummaryBar";
import {
  PluginPackageDetails,
  pluginFormats,
} from "@/components/plugins/PluginDetails";
import { useTarget } from "@/components/targets/TargetProvider";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useAgents, useSettings, useUpdateSettings } from "@/hooks/useData";
import { usePluginCli, usePluginInventory } from "@/hooks/usePlugins";
import { projectsApi } from "@/lib/api";
import {
  pluginsApi,
  type PluginAction,
  type PluginOperation,
  type PluginOperationResult,
  type PluginPackage,
  type PluginView,
} from "@/lib/api/plugins";
import { getTarget } from "@/lib/api/transport";
import { queryKeys } from "@/lib/queryKeys";

const actionLabels: Record<PluginAction, string> = {
  install: "安装",
  enable: "启用",
  disable: "停用",
  update: "更新",
  uninstall: "移除",
  forget: "移除停用记录",
};
const writeAgents = ["claude-code", "opencode", "pi"];
const scopeLabels: Record<string, string> = {
  user: "个人配置",
  project: "项目",
  local: "项目本地",
  profile: "Profile",
  unknown: "作用域未确认",
};
export function PluginsPage({
  requestedPluginId,
  onRequestConsumed,
}: { requestedPluginId?: string | null; onRequestConsumed?: () => void } = {}) {
  const local = useTarget().id === "local";
  const { data: settings } = useSettings();
  const { data: agents = [] } = useAgents();
  const inventory = usePluginInventory();
  const managed = local && settings?.managePlugins === true;
  const capabilities = usePluginCli(managed);
  const updateSettings = useUpdateSettings();
  const qc = useQueryClient();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<string | null>(null);
  const [detail, setDetail] = useState<PluginView | null>(null);
  const [adding, setAdding] = useState(false);
  const [mode, setMode] = useState("install");
  const [agentId, setAgentId] = useState("claude-code");
  const [source, setSource] = useState("");
  const [marketplace, setMarketplace] = useState("");
  const [path, setPath] = useState("");
  const [preview, setPreview] = useState<PluginPackage | null>(null);
  const [lastResult, setLastResult] = useState<PluginOperationResult | null>(
    null,
  );
  const [lastError, setLastError] = useState<string | null>(null);
  const previewRequest = useRef(0);
  const [confirmation, setConfirmation] = useState<{
    input: PluginOperation;
    name: string;
  } | null>(null);
  usePageReset(() => setDetail(null));
  useEffect(() => {
    if (!local) {
      setAdding(false);
      setConfirmation(null);
      setDetail(null);
      previewRequest.current++;
    }
  }, [local]);
  useEffect(() => {
    if (!requestedPluginId || !inventory.data) return;
    const plugin = inventory.data.plugins.find(
      (plugin) => plugin.id === requestedPluginId,
    );
    if (plugin) setDetail(plugin);
    else toast.warning("所属插件已不在本机清单中，请重新扫描");
    onRequestConsumed?.();
  }, [requestedPluginId, inventory.data, onRequestConsumed]);
  const operation = useMutation({
    mutationFn: pluginsApi.operate,
    onSuccess: (result, input) => {
      setLastResult(result);
      setLastError(null);
      setConfirmation(null);
      if (!result.pending) {
        setAdding(false);
        if (input.action === "uninstall" || input.action === "forget")
          setDetail(null);
      }
      if (result.pending) toast.warning(`本机：${result.message}`);
      else toast.success(`本机：${result.message}`);
      for (const warning of result.warnings) toast.warning(warning);
    },
    onError: (error) => {
      setLastResult(null);
      setLastError(String(error));
      setConfirmation(null);
      toast.error(`本机插件操作失败：${String(error)}`);
    },
    onSettled: () =>
      Promise.all(
        [
          queryKeys.plugins,
          queryKeys.skills,
          queryKeys.mcpStatus,
          queryKeys.config,
          queryKeys.backups,
        ].map((queryKey) => qc.invalidateQueries({ queryKey })),
      ),
  });
  const recovery = useMutation({
    mutationFn: () => pluginsApi.recover(true),
    onSuccess: (result) => toast.success(result.message),
    onError: (error) => toast.error(String(error)),
    onSettled: () => qc.invalidateQueries({ queryKey: queryKeys.plugins }),
  });
  const inspection = useMutation({
    mutationFn: ({ path, agentId }: { path: string; agentId: string }) =>
      pluginsApi.inspect(path, agentId || null),
  });
  if (!local)
    return (
      <EmptyState
        icon={Package}
        title="插件首版仅支持本机"
        description="不会读取本机插件冒充服务器状态，也不提供远程插件写入。"
      />
    );
  const plugins = inventory.data?.plugins ?? [];
  const agentName = (id: string) =>
    agents.find((agent) => agent.id === id)?.displayName ??
    SOURCE_LABELS[id] ??
    id;
  const visibleAgents = agents.filter(
    (agent) => !settings?.disabledAgents?.includes(agent.id),
  );
  const activeFilter =
    filter && !settings?.disabledAgents?.includes(filter) ? filter : null;
  const filtered = plugins.filter(
    (plugin) =>
      (!activeFilter || plugin.agentId === activeFilter) &&
      `${plugin.package.name} ${plugin.source} ${plugin.package.description ?? ""} ${plugin.package.components.map((component) => component.name).join(" ")}`
        .toLowerCase()
        .includes(query.trim().toLowerCase()),
  );
  const currentDetail = detail
    ? plugins.find((plugin) => plugin.id === detail.id)
    : undefined;
  const liveDetail = currentDetail ?? detail;
  const canInstall =
    managed &&
    capabilities.data?.find((capability) => capability.agentId === agentId)
      ?.install === true &&
    !operation.isPending;
  const actions = (plugin: PluginView, compact: boolean) => (
    <div className="flex flex-wrap items-center gap-1">
      {plugin.actions
        .filter(
          (action) =>
            action !== "install" &&
            !(action === "enable" && plugin.enabled !== false) &&
            !(action === "disable" && plugin.enabled === false),
        )
        .map((action) => {
          const allowed =
            managed &&
            !operation.isPending &&
            (action === "forget" ||
              capabilities.data?.find(
                (capability) => capability.agentId === plugin.agentId,
              )?.install === true);
          const Icon =
            action === "enable" || action === "disable"
              ? Power
              : action === "update"
                ? RefreshCw
                : Trash2;
          const reason = !managed
            ? "请先开启插件管理"
            : (capabilities.data?.find(
                (capability) => capability.agentId === plugin.agentId,
              )?.reason ?? "原生 CLI 检测中");
          return (
            <Button
              key={action}
              size={compact ? "icon" : "sm"}
              variant="ghost"
              className={compact ? "h-8 w-8" : ""}
              disabled={!allowed}
              title={allowed ? actionLabels[action] : reason}
              aria-label={`${actionLabels[action]} ${plugin.package.name}`}
              onClick={() =>
                setConfirmation({
                  name: plugin.package.name,
                  input: {
                    action,
                    agentId: plugin.agentId,
                    pluginId: plugin.id,
                    expectedRevision: plugin.revision,
                    confirmed: false,
                  },
                })
              }
            >
              <Icon className="h-4 w-4" />
              {!compact && actionLabels[action]}
            </Button>
          );
        })}
    </div>
  );
  const inspectPath = () => {
    const request = ++previewRequest.current;
    inspection.mutate(
      { path: path.trim(), agentId },
      {
        onSuccess: (value) => {
          if (request === previewRequest.current) setPreview(value);
        },
        onError: (error) => {
          if (request === previewRequest.current) toast.error(String(error));
        },
      },
    );
  };
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <PageTools
        query={query}
        onQueryChange={setQuery}
        placeholder="搜索插件名称、来源或组件"
        createLabel="添加插件"
        onCreate={() => {
          setPreview(null);
          setAdding(true);
        }}
      />
      {!managed && (
        <div className="my-3 flex items-center justify-between gap-3 rounded-xl border border-border-default p-4">
          <p className="text-sm text-muted-foreground">
            当前只读扫描。开启管理不会自动启用、迁移或卸载已有插件。
          </p>
          <Button
            size="sm"
            disabled={updateSettings.isPending}
            onClick={() => updateSettings.mutate({ managePlugins: true })}
          >
            开启插件管理
          </Button>
        </div>
      )}
      {lastResult && (
        <div
          role={lastResult.pending ? "alert" : "status"}
          className="mb-3 rounded-lg border border-border-default p-3 text-xs"
        >
          <p>{lastResult.message}</p>
          {lastResult.backupPath && (
            <p className="mt-1 break-all font-mono text-muted-foreground">
              原生配置快照：{lastResult.backupPath}
            </p>
          )}
        </div>
      )}
      {lastError && (
        <p role="alert" className="mb-3 text-sm text-destructive">
          {lastError}
        </p>
      )}
      {inventory.data?.pending && (
        <p
          role="alert"
          className="my-2 text-sm text-amber-700 dark:text-amber-400"
        >
          上次插件配置写入有待处理记录。下一次明确的写操作会核对配置后完成记录；外部修改不会被覆盖。
        </p>
      )}
      {inventory.isError && (
        <p role="alert" className="my-3 text-sm text-destructive">
          插件清单读取失败：{String(inventory.error)}
        </p>
      )}
      {managed && (inventory.data?.pending || inventory.isError) && (
        <Button
          className="mb-3 self-start"
          size="sm"
          variant="outline"
          disabled={recovery.isPending || operation.isPending}
          onClick={() => recovery.mutate()}
        >
          核对待处理记录（不改 Agent）
        </Button>
      )}
      {!!inventory.data?.warnings.length && (
        <details className="my-2 text-xs text-amber-700 dark:text-amber-400">
          <summary>扫描提示 · {inventory.data.warnings.length}</summary>
          <ul className="mt-2 space-y-1">
            {inventory.data.warnings.map((warning, index) => (
              <li key={index}>{warning}</li>
            ))}
          </ul>
        </details>
      )}
      {liveDetail ? (
        <div className="min-h-0 flex-1 overflow-y-auto pb-6">
          <DetailHeader
            title={liveDetail.package.name}
            subtitle={`${agentName(liveDetail.agentId)} · ${scopeLabels[liveDetail.scope] ?? liveDetail.scope}${liveDetail.profile ? " · " + liveDetail.profile : ""}`}
            onBack={() => setDetail(null)}
            actions={currentDetail ? actions(currentDetail, false) : undefined}
          />
          {!currentDetail && inventory.isSuccess && (
            <p role="alert" className="mb-4 text-sm text-destructive">
              原生插件记录已被移除或来源变更，不能继续操作旧记录。
            </p>
          )}
          <div className="mb-4 space-y-1 text-xs text-muted-foreground">
            <p className="break-all">原生标识：{liveDetail.nativeId}</p>
            <p className="break-all">配置：{liveDetail.configPath}</p>
            {liveDetail.readOnlyReason && <p>{liveDetail.readOnlyReason}</p>}
          </div>
          <PluginPackageDetails value={liveDetail.package} />
        </div>
      ) : (
        <>
          <SummaryBar>
            <SummaryFilter
              label="按 Agent 查看插件"
              total={plugins.length}
              options={visibleAgents.map((agent) =>
                agentOption({
                  id: agent.id,
                  label: agent.displayName,
                  count: plugins.filter((plugin) => plugin.agentId === agent.id)
                    .length,
                }),
              )}
              selected={activeFilter}
              onSelect={setFilter}
            />
            <SummaryEnd>
              <SummaryStat title="可读取包文件，不代表会话已加载">
                文件可读 {plugins.filter((plugin) => plugin.available).length}
              </SummaryStat>
            </SummaryEnd>
          </SummaryBar>
          <div className="min-h-0 flex-1 overflow-y-auto pb-6">
            {inventory.isLoading ? (
              <p className="py-10 text-center text-sm text-muted-foreground">
                读取插件清单…
              </p>
            ) : filtered.length ? (
              <ListContainer cards>
                {filtered.map((plugin) => (
                  <ListItemRow
                    key={plugin.id}
                    card
                    onPreview={() => setDetail(plugin)}
                    previewLabel={`查看插件 ${plugin.package.name} · ${agentName(plugin.agentId)} · ${scopeLabels[plugin.scope] ?? plugin.scope}${plugin.profile ? " · " + plugin.profile : ""}`}
                  >
                    <AgentIcon agentId={plugin.agentId} />
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-medium">
                          {plugin.package.name}
                        </span>
                        <Badge variant="outline">
                          {pluginFormats[plugin.package.format] ??
                            plugin.package.format}
                        </Badge>
                        <Badge variant="secondary">
                          {plugin.enabled === true
                            ? "配置已启用"
                            : plugin.enabled === false
                              ? "配置已停用"
                              : "启用状态未确认"}
                        </Badge>
                        {!plugin.actions.length && (
                          <Badge variant="outline">只读</Badge>
                        )}
                      </div>
                      <p className="mt-1 truncate text-xs text-muted-foreground">
                        {scopeLabels[plugin.scope] ?? plugin.scope}
                        {plugin.profile ? " · " + plugin.profile : ""} ·{" "}
                        {plugin.package.version
                          ? "v" + plugin.package.version
                          : "版本未确认"}{" "}
                        · {plugin.package.components.length} 项声明组件
                      </p>
                      <p className="mt-1 truncate text-xs text-muted-foreground">
                        {plugin.source}
                      </p>
                    </div>
                    <RowActions busy={operation.isPending}>
                      {actions(plugin, true)}
                    </RowActions>
                  </ListItemRow>
                ))}
              </ListContainer>
            ) : (
              <EmptyState
                icon={Package}
                title={
                  query || activeFilter
                    ? "没有匹配的插件"
                    : "未发现可识别的插件"
                }
                description="扫描现有 Agent 的原生声明和可定位包目录；也可以检查本地目录，不会执行插件代码。"
              />
            )}
          </div>
        </>
      )}
      <Dialog
        open={adding}
        onOpenChange={(open) => {
          if (!operation.isPending) {
            setAdding(open);
            if (!open) {
              previewRequest.current++;
              setPreview(null);
            }
          }
        }}
      >
        <DialogContent className="max-w-xl">
          <DialogHeader>
            <DialogTitle>添加插件</DialogTitle>
            <DialogDescription>
              只操作本机个人配置。保持原生包，不生成市场、转换格式或拆包部署。
            </DialogDescription>
          </DialogHeader>
          <Tabs
            value={mode}
            onValueChange={setMode}
            className="min-h-0 flex-1 overflow-y-auto px-6 pb-4"
          >
            <TabsList className="grid w-full grid-cols-2">
              <TabsTrigger value="install">原生安装</TabsTrigger>
              <TabsTrigger value="inspect">检查本地目录</TabsTrigger>
            </TabsList>
            <div className="space-y-3 pt-4">
              <Label htmlFor="plugin-agent">Agent</Label>
              <Select
                value={agentId}
                onValueChange={(value) => {
                  setAgentId(value);
                  setPreview(null);
                  previewRequest.current++;
                }}
              >
                <SelectTrigger id="plugin-agent">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {(mode === "install"
                    ? writeAgents
                    : agents.map((agent) => agent.id)
                  ).map((id) => (
                    <SelectItem key={id} value={id}>
                      {agentName(id)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <TabsContent value="install" className="space-y-4 pt-2">
              <div className="space-y-2">
                <Label htmlFor="plugin-source">原生插件来源</Label>
                <Input
                  id="plugin-source"
                  value={source}
                  onChange={(event) => setSource(event.target.value)}
                  placeholder={
                    agentId === "claude-code"
                      ? "name@marketplace"
                      : agentId === "pi"
                        ? "npm:@example/pi-tools@1.0.0 或本地路径"
                        : "npm 包名、Git 包来源或本地目录"
                  }
                />
              </div>
              {agentId === "claude-code" && (
                <div className="space-y-2">
                  <Label htmlFor="plugin-marketplace">
                    市场来源（尚未添加时填写）
                  </Label>
                  <Input
                    id="plugin-marketplace"
                    value={marketplace}
                    onChange={(event) => setMarketplace(event.target.value)}
                    placeholder="owner/repo 或原生支持的市场路径"
                  />
                </div>
              )}
              <p className="text-xs text-muted-foreground">
                {!managed
                  ? "请先开启插件管理。"
                  : capabilities.isPending
                    ? "正在核对原生 CLI 能力…"
                    : (capabilities.data?.find(
                        (capability) => capability.agentId === agentId,
                      )?.reason ??
                      "使用已验证版本的原生 CLI；需要交互授权时会停止，不会自动跳过。")}
              </p>
              <DialogFooter>
                <Button
                  disabled={!canInstall || !source.trim()}
                  onClick={() =>
                    setConfirmation({
                      name: source.trim(),
                      input: {
                        action: "install",
                        agentId,
                        source: source.trim(),
                        marketplace:
                          agentId === "claude-code" && marketplace.trim()
                            ? marketplace.trim()
                            : undefined,
                        confirmed: false,
                      },
                    })
                  }
                >
                  检查影响并安装
                </Button>
              </DialogFooter>
            </TabsContent>
            <TabsContent value="inspect" className="space-y-4 pt-2">
              <div className="space-y-2">
                <Label htmlFor="plugin-path">插件目录或 JS/TS 文件路径</Label>
                <div className="flex gap-2">
                  <Input
                    id="plugin-path"
                    value={path}
                    onChange={(event) => {
                      setPath(event.target.value);
                      setPreview(null);
                      previewRequest.current++;
                    }}
                  />
                  <Button
                    size="icon"
                    variant="outline"
                    aria-label="选择插件目录"
                    onClick={async () => {
                      const picked = await projectsApi.pickDirectory();
                      if (picked && getTarget().id === "local") {
                        setPath(picked);
                        setPreview(null);
                        previewRequest.current++;
                      }
                    }}
                  >
                    <FolderOpen className="h-4 w-4" />
                  </Button>
                </div>
              </div>
              <Button
                variant="outline"
                disabled={!path.trim() || inspection.isPending}
                onClick={inspectPath}
              >
                <Link2 className="h-4 w-4" />
                {inspection.isPending ? "检查中…" : "只读检查"}
              </Button>
              {preview && <PluginPackageDetails value={preview} />}
            </TabsContent>
          </Tabs>
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        open={!!confirmation}
        onOpenChange={(open) => {
          if (!open && !operation.isPending) setConfirmation(null);
        }}
        title={
          confirmation?.input.action === "forget"
            ? "清理插件停用记录"
            : confirmation?.input.action === "uninstall"
              ? "移除插件"
              : `${confirmation ? actionLabels[confirmation.input.action] : "操作"}插件`
        }
        variant={
          confirmation?.input.action === "uninstall" ||
          confirmation?.input.action === "forget"
            ? "destructive"
            : "info"
        }
        pending={operation.isPending}
        confirmDisabled={!managed}
        confirmText="确认执行"
        description={
          <>
            <span className="break-all">
              {confirmation?.name} ·{" "}
              {agentName(confirmation?.input.agentId ?? "")} · 本机个人配置。
            </span>
            <br />
            {confirmation?.input.action === "forget"
              ? "仅移除 Studio 停用记录，不修改原生配置和插件文件。"
              : confirmation?.input.action === "uninstall"
                ? "调用原生移除命令。尽量保留可保留的数据，但原生工具仍可能删除选项或密钥；配置快照不能恢复插件代码、认证或运行数据。"
                : "安装、启用或更新可能执行本机代码；更新可能替换插件文件。Studio 不授予额外信任，不转换格式，也不保证现有会话已重载。"}
          </>
        }
        onConfirm={() => {
          if (confirmation)
            operation.mutate({ ...confirmation.input, confirmed: true });
        }}
      />
    </div>
  );
}
