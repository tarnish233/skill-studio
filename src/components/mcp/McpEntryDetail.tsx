import { useId, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Activity,
  LogIn,
  LogOut,
  MapPin,
  PackagePlus,
  Play,
  Route,
  Settings2,
  Tag,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { AgentIcon } from "@/components/common/AgentIcon";
import { DetailHeader } from "@/components/common/DetailHeader";
import { ListContainer, ListItemRow } from "@/components/common/ListItemRow";
import { useUnsavedProject } from "@/components/common/NavigationGuard";
import { SettingCard, SettingsSection } from "@/components/common/SettingCard";
import { McpChoiceCards } from "./McpChoiceCards";
import { McpDefinitionFields, readDefinition } from "./McpDefinitionFields";
import { mcpRequest } from "@/lib/api/mcp";
import type {
  ManagedMcp,
  McpInstallResult,
  McpRow,
} from "@/lib/api/mcpManagement";
import { mcpAgentName, mcpAppId } from "@/lib/mcpAgents";

/** 本次启动后对当前配置的测试结果 */
export interface McpTestState {
  status: "ok" | "auth" | "error";
  at: number;
}

/** 卡片和详情标题栏共用的状态徽标：连接方式、托管、最近测试、授权 */
export function McpStatusBadges({
  row,
  test,
  authPending,
  running,
}: {
  row: McpRow;
  test?: McpTestState;
  /** 本次测试发现需要登录 */
  authPending: boolean;
  running: boolean;
}) {
  return (
    <>
      <Badge variant="outline">
        {row.managed
          ? row.entry.mode === "direct"
            ? "Agent 直连"
            : "Studio 网关"
          : "原有配置"}
      </Badge>
      {row.managed && (
        <Badge
          variant="outline"
          className="h-4 border-blue-200 bg-blue-50 px-1.5 text-[10px] text-blue-600 dark:border-blue-800 dark:bg-blue-950/50 dark:text-blue-400"
        >
          托管中
        </Badge>
      )}
      {test && (
        <Badge
          title={`本次启动后的测试：${new Date(test.at).toLocaleString()}`}
          variant={test.status === "ok" ? "success" : "warning"}
        >
          {test.status === "ok"
            ? "本机连接已验证"
            : test.status === "auth"
              ? "需要登录"
              : "最近测试失败"}
        </Badge>
      )}
      {row.entry.mode === "gateway" &&
        (row.authorized || row.authRequired || authPending) && (
          <Badge variant="outline">
            {row.authRequired || authPending
              ? "待授权"
              : row.authorized
                ? "已授权"
                : running
                  ? "待授权"
                  : "授权待检查"}
          </Badge>
        )}
    </>
  );
}

/**
 * MCP Hub 里点开一张卡片的整页：与项目、分组详情同一套标题栏。上面是测试、登录和保存，
 * 下面依次是连接说明、接入位置（Studio 管理的接入、扫描到的配置来源）和可直接编辑的服务配置。
 * 原有配置可以托管到 Hub（接管原有接入），也可以另存为 Hub 里的新服务（原有配置不动）。
 */
export function McpEntryDetail({
  row,
  running,
  busy,
  authPending,
  test,
  onBack,
  onTest,
  onLogin,
  onLogout,
  onAdopt,
  onSaved,
}: {
  row: McpRow;
  /** 网关是否在运行 */
  running: boolean;
  /** 页面上的测试、登录、托管等操作正在进行 */
  busy: boolean;
  authPending: boolean;
  test?: McpTestState;
  onBack: () => void;
  onTest: () => void;
  onLogin: () => void;
  onLogout: () => void;
  /** 原有配置：托管到 Hub，接管它的原有接入 */
  onAdopt: () => void;
  onSaved: (entry: ManagedMcp, result?: McpInstallResult) => void;
}) {
  const managed = row.managed;
  // 指向 Studio 网关的原有配置不是独立的服务，不能另存或托管
  const viaGateway = !managed && row.sources.some((s) => s.gateway);
  const [entry, setEntry] = useState<ManagedMcp>(() => ({
    ...row.entry,
    id: managed ? row.entry.id : crypto.randomUUID(),
  }));
  const [raw, setRaw] = useState(() =>
    JSON.stringify(row.entry.definition, null, 2),
  );
  const [original, setOriginal] = useState(() =>
    JSON.stringify({ entry, raw }),
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const formId = useId();
  const dirty = JSON.stringify({ entry, raw }) !== original;
  const locked = saving || busy;
  useUnsavedProject(dirty, locked);
  const { definition, error: definitionError } = readDefinition(raw);
  const effective: ManagedMcp = {
    ...entry,
    // 接入以 Hub 里的最新记录为准：页面开着期间可能托管了新的来源
    bindings: managed ? row.entry.bindings : entry.bindings,
    definition,
    oauth:
      entry.mode === "gateway" && definition.type !== "stdio" && entry.oauth,
  };
  const gatewayCheck = useQuery({
    queryKey: ["mcp-check", effective],
    queryFn: () =>
      mcpRequest<{ issue: string | null }>("gatewayCheck", {
        entry: effective,
      }),
    enabled: entry.mode === "gateway" && !definitionError,
    retry: false,
  });
  const local = definition.type === "stdio";
  const sse =
    row.entry.mode === "direct" && row.entry.definition.type === "sse";
  const gatewayBlocked =
    entry.mode === "gateway" &&
    (gatewayCheck.isPending ||
      !!gatewayCheck.data?.issue ||
      !!gatewayCheck.error);
  const canSave =
    !locked &&
    !viaGateway &&
    !definitionError &&
    !!entry.name.trim() &&
    !gatewayBlocked &&
    (dirty || !managed);

  async function save() {
    if (!canSave) return;
    setSaving(true);
    setError(null);
    const snapshot = JSON.stringify({ entry, raw });
    try {
      await mcpRequest("saveEntry", {
        entry: effective,
        expectedEntry: managed ? row.entry : null,
        bindingIds: effective.bindings.map((b) => b.id),
        sources: [],
        agents: [],
        projectId: "",
        scope: "user",
      });
      setOriginal(snapshot);
      onSaved(effective, { installed: effective.bindings.length > 0 });
    } catch (e) {
      setError(
        `保存失败，修改已保留：${e instanceof Error ? e.message : String(e)}`,
      );
    } finally {
      setSaving(false);
    }
  }

  const testLabel =
    row.entry.mode === "direct" || running ? "测试连接" : "启动并测试";
  const showLogin =
    row.entry.mode === "gateway" &&
    row.entry.definition.type !== "stdio" &&
    (row.authorized || row.authRequired || authPending);
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <DetailHeader
        title={entry.name.trim() || row.entry.name}
        badges={
          <McpStatusBadges
            row={row}
            test={test}
            authPending={authPending}
            running={running}
          />
        }
        subtitle={String(
          row.entry.definition.url ?? row.entry.definition.command ?? "",
        )}
        mono
        onBack={onBack}
        actions={
          managed ? (
            <div
              className="flex items-center gap-2"
              title={dirty ? "先保存修改，再测试连接" : undefined}
            >
              <Button
                variant="outline"
                size="sm"
                disabled={locked || dirty || sse}
                onClick={onTest}
              >
                <Play className="h-4 w-4" />
                {testLabel}
              </Button>
              {showLogin && (
                <Button
                  variant="outline"
                  size="sm"
                  disabled={locked || dirty}
                  onClick={onLogin}
                >
                  <LogIn className="h-4 w-4" />
                  {running ? "登录授权" : "启动并授权"}
                </Button>
              )}
              {row.entry.mode === "gateway" && row.authorized && (
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={locked || !running}
                  onClick={onLogout}
                >
                  <LogOut className="h-4 w-4" />
                  清除授权
                </Button>
              )}
            </div>
          ) : (
            <Button
              variant="outline"
              size="sm"
              title={`保存到 MCP Hub，并管理它的 ${row.sources.length} 处原有接入。配置文件会先备份，之后可移出 Hub 并还原。`}
              disabled={locked || viaGateway}
              onClick={onAdopt}
            >
              <PackagePlus className="h-4 w-4" />
              托管到 Hub
            </Button>
          )
        }
        save={{
          form: formId,
          pending: saving,
          disabled: !canSave,
          title: managed
            ? row.entry.bindings.length
              ? "保存并同步 Studio 管理的接入"
              : "保存到 MCP Hub"
            : "另存为 Hub 里的新服务，原有配置保持不变",
          label: managed ? "保存" : "保存到 Hub",
          pendingLabel: "正在保存…",
        }}
      />
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      {/* Leave room for focus rings inside the scroll viewport without shifting the fields. */}
      <div className="-mx-1 min-h-0 flex-1 space-y-6 overflow-y-auto px-1 pb-6 pt-1">
        <SettingsSection title="连接" icon={<Activity />}>
          <p className="text-xs text-muted-foreground">
            {!managed
              ? viaGateway
                ? "这处配置指向 Studio 网关，不是独立的服务：请在 MCP Hub 添加独立配置。"
                : "这是 Agent 配置里已有的 MCP，还没交给 Studio 管理。托管到 Hub 会接管下面的原有接入；保存到 Hub 只另存一份，原有配置保持不变。"
              : row.entry.mode === "gateway"
                ? "由 Studio 网关连接服务，授权由 Studio 管理。"
                : "由 Agent 直接连接服务，登录和运行状态由各 Agent 管理。"}
          </p>
          {managed && row.entry.mode === "direct" && (
            <p className="text-xs text-muted-foreground">
              {sse
                ? "SSE 服务请在 Agent 中测试连接，Studio 暂不支持此类型的本机测试。"
                : "本机测试只验证服务握手和工具列表；Agent 是否已重新加载及其登录状态仍需在 Agent 中确认。"}
            </p>
          )}
        </SettingsSection>
        <SettingsSection title="接入位置" icon={<MapPin />}>
          {!row.entry.bindings.length && !row.sources.length ? (
            <p className="text-sm text-muted-foreground">
              还没有接入：在分组或项目里选择它，再为需要的 Agent 启用。
            </p>
          ) : (
            <ListContainer>
              {row.entry.bindings.map((b, i) => (
                <ListItemRow
                  key={`binding:${b.id}`}
                  isLast={
                    !row.sources.length && i === row.entry.bindings.length - 1
                  }
                >
                  <AgentIcon agentId={mcpAppId(b.agent)} size="sm" />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <p className="truncate text-sm font-medium">
                        {mcpAgentName(b.agent)}
                      </p>
                      <Badge variant="outline">Studio 管理</Badge>
                      <Badge variant="outline">
                        {row.entry.mode === "gateway" ? "网关" : "直连"}
                      </Badge>
                    </div>
                    <p className="break-all font-mono text-[11px] text-muted-foreground">
                      {b.path} · {b.key}
                    </p>
                  </div>
                </ListItemRow>
              ))}
              {row.sources.map((s, i) => (
                <ListItemRow
                  key={`source:${s.id}`}
                  isLast={i === row.sources.length - 1}
                >
                  <AgentIcon agentId={mcpAppId(s.agent)} size="sm" />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <p className="truncate text-sm font-medium">
                        {mcpAgentName(s.agent)}
                      </p>
                      <Badge variant="outline">扫描到的配置</Badge>
                      <Badge variant="outline">{s.scope}</Badge>
                      {!s.enabled && <Badge variant="outline">已停用</Badge>}
                    </div>
                    <p className="break-all font-mono text-[11px] text-muted-foreground">
                      {s.path} · {s.key}
                    </p>
                    {s.projectOverrides?.map((override) => (
                      <p
                        key={`${override.path}:${override.key}`}
                        className="mt-1 break-all text-xs text-muted-foreground"
                      >
                        {override.scope}
                        {override.enabled === false
                          ? " · 已停用"
                          : override.enabled
                            ? " · 已启用"
                            : ""}
                        {" · 项目级覆盖"}
                        <span className="block font-mono text-[11px]">
                          {override.path} · {override.key}
                        </span>
                      </p>
                    ))}
                  </div>
                </ListItemRow>
              ))}
            </ListContainer>
          )}
        </SettingsSection>
        <form
          id={formId}
          className="min-w-0"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <fieldset
            disabled={locked || viaGateway}
            className="min-w-0 space-y-3"
          >
            <SettingsSection title="服务配置" icon={<Settings2 />}>
              <SettingCard
                compact
                icon={<Tag />}
                title={<Label htmlFor="mcp-name">名称</Label>}
                description="用于在 Hub 和 Agent 中识别此 MCP。"
              >
                <Input
                  className="w-52 sm:w-72"
                  id="mcp-name"
                  required
                  value={entry.name}
                  onChange={(e) => setEntry({ ...entry, name: e.target.value })}
                />
              </SettingCard>
            </SettingsSection>
            <McpDefinitionFields raw={raw} setRaw={setRaw} />
            <McpChoiceCards
              label="连接方式"
              icon={<Route />}
              value={entry.mode}
              onChange={(mode) => setEntry({ ...entry, mode })}
              options={[
                {
                  value: "direct",
                  title: "Agent 直连",
                  description: "由各 Agent 连接和登录，适合大多数情况。",
                },
                {
                  value: "gateway",
                  title: "Studio 代理",
                  description: "由 Studio 连接服务，可让多个 Agent 共用登录。",
                },
              ]}
            />
            {entry.mode === "gateway" && (
              <>
                {!local && (
                  <p className="text-xs text-muted-foreground">
                    连接时自动检查授权需求，需要时会提示登录。
                  </p>
                )}
                {!local && (
                  <details className="space-y-3">
                    <summary className="cursor-pointer text-sm text-muted-foreground">
                      授权设置（可选）
                    </summary>
                    <Input
                      aria-label="OAuth Client ID"
                      placeholder="Client ID（可选）"
                      value={entry.clientId ?? ""}
                      onChange={(e) =>
                        setEntry({
                          ...entry,
                          clientId: e.target.value || null,
                        })
                      }
                    />
                    <Input
                      aria-label="OAuth scopes"
                      placeholder="Scopes（空格分隔）"
                      value={entry.scopes.join(" ")}
                      onChange={(e) =>
                        setEntry({
                          ...entry,
                          scopes: e.target.value.split(/\s+/).filter(Boolean),
                        })
                      }
                    />
                  </details>
                )}
                {(gatewayCheck.data?.issue || gatewayCheck.error) && (
                  <p
                    role="alert"
                    className="text-sm text-amber-600 dark:text-amber-400"
                  >
                    {gatewayCheck.data?.issue || String(gatewayCheck.error)}
                    。仍可选择 Agent 直连。
                  </p>
                )}
              </>
            )}
            <p className="text-xs text-muted-foreground">
              {!managed
                ? "保存到 MCP Hub 后，在分组或项目里选择使用。原有配置保持不变。"
                : row.entry.bindings.length
                  ? "保存会同步 Studio 管理的接入；分组和项目里用到它的，需在对应卡片上「应用修改」。"
                  : "保存后，分组和项目里用到它的，需在对应卡片上「应用修改」。"}
            </p>
          </fieldset>
        </form>
      </div>
    </div>
  );
}
