import { SettingCard, SettingsSection } from "@/components/common/SettingCard";
import {
  ClipboardPaste,
  Compass,
  Route,
  Search,
  Settings2,
  Tag,
  WandSparkles,
} from "lucide-react";
import { McpChoiceCards } from "./McpChoiceCards";
import { McpDefinitionFields, readDefinition } from "./McpDefinitionFields";
import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useUnsavedProject } from "@/components/common/NavigationGuard";
import { mcpRequest } from "@/lib/api/mcp";
import {
  managementApi,
  type ManagedMcp,
  type ParsedMcp,
  type McpInstallResult,
} from "@/lib/api/mcpManagement";

function emptyEntry(): ManagedMcp {
  return {
    id: crypto.randomUUID(),
    name: "",
    mode: "direct",
    definition: { type: "http", url: "" },
    oauth: false,
    clientId: null,
    scopes: [],
    bindings: [],
  };
}
export function McpQuickInstall({
  onClose,
  onSaved,
}: {
  onClose: () => void;
  onSaved: (entry: ManagedMcp, result?: McpInstallResult) => void;
}) {
  const [text, setText] = useState("");
  const [source, setSource] = useState<"manual" | "registry">("manual");
  const [registryQuery, setRegistryQuery] = useState("");
  const [registryItems, setRegistryItems] = useState<
    {
      name: string;
      description: string;
      version: string;
      definition: Record<string, unknown> | null;
      source: string;
    }[]
  >([]);
  const [registrySearched, setRegistrySearched] = useState(false);
  const [registryCursor, setRegistryCursor] = useState<string | null>(null);
  const [registryBusy, setRegistryBusy] = useState(false);
  const [registryError, setRegistryError] = useState<string | null>(null);
  const [items, setItems] = useState<ParsedMcp[]>([]);
  const [entry, setEntry] = useState<ManagedMcp | null>(() => emptyEntry());
  const [raw, setRaw] = useState('{"type":"http","url":""}');
  const { definition, error: configError } = readDefinition(raw);
  const [parsing, setParsing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [phase, setPhase] = useState("");
  // Parsing resets generated IDs; only editable fields determine whether a
  // draft would be lost when navigating through the persistent sidebar.
  const [original] = useState(() =>
    JSON.stringify({ entry: { ...entry, id: "" }, raw }),
  );
  useUnsavedProject(
    !!text.trim() ||
      (entry !== null &&
        JSON.stringify({ entry: { ...entry, id: "" }, raw }) !== original),
    busy,
  );
  function choose(item: ParsedMcp) {
    const definition = item.definition;
    setRaw(JSON.stringify(definition, null, 2));
    setEntry({
      id: crypto.randomUUID(),
      name: item.name,
      mode: "direct",
      definition,
      oauth: false,
      clientId: null,
      scopes: [],
      bindings: [],
    });
  }
  async function searchRegistry(cursor?: string) {
    if (registryQuery.trim().length < 2 || registryBusy) return;
    setRegistryBusy(true);
    setRegistryError(null);
    if (!cursor) {
      setRegistrySearched(false);
      setRegistryItems([]);
      setRegistryCursor(null);
    }
    try {
      const page = await mcpRequest<{
        items: typeof registryItems;
        nextCursor: string | null;
      }>("searchRegistry", {
        query: registryQuery.trim(),
        cursor,
      });
      setRegistryItems((items) =>
        cursor ? [...items, ...page.items] : page.items,
      );
      setRegistryCursor(page.nextCursor);
      setRegistrySearched(true);
    } catch (error) {
      setRegistryError(String(error));
    } finally {
      setRegistryBusy(false);
    }
  }
  useEffect(() => {
    let cancelled = false;
    setEntry(null);
    setItems([]);
    setError(null);
    setParsing(!!text.trim());
    if (!text.trim()) {
      setEntry(emptyEntry());
      setRaw('{"type":"http","url":""}');
      return;
    }
    const timer = setTimeout(() => {
      void managementApi
        .parse(text)
        .then((results) => {
          if (cancelled) return;
          setItems(results);
          if (results.length === 1) choose(results[0]);
        })
        .catch((e) => {
          if (!cancelled) setError(String(e));
        })
        .finally(() => {
          if (!cancelled) setParsing(false);
        });
    }, 350);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [text]);
  const gateway = entry?.mode === "gateway";
  const local = definition.type === "stdio";
  const effective = entry ? { ...entry, definition, oauth: false } : null;
  const check = useQuery({
    queryKey: ["mcp-check", effective],
    queryFn: () =>
      mcpRequest<{ issue: string | null }>("gatewayCheck", {
        entry: effective,
      }),
    enabled: !!gateway && !!effective,
    retry: false,
  });
  const invalid =
    !entry ||
    !entry.name.trim() ||
    !!configError ||
    !String(
      local ? (definition.command ?? "") : (definition.url ?? ""),
    ).trim() ||
    parsing ||
    (gateway && (check.isPending || !!check.data?.issue || !!check.error));
  async function install() {
    if (!effective || invalid || busy) return;
    setBusy(true);
    setError(null);
    setPhase("正在保存配置…");
    try {
      await mcpRequest("saveEntry", {
        entry: effective,
        expectedEntry: null,
        bindingIds: [],
        sources: [],
        agents: [],
        scope: "user",
        projectId: "",
      });
      onSaved(effective, { installed: false });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <form
      className="flex min-h-0 flex-1 flex-col"
      onSubmit={(e) => {
        e.preventDefault();
        void install();
      }}
    >
      <div className="-mx-1 min-h-0 flex-1 overflow-y-auto px-1">
        <fieldset disabled={busy} className="min-w-0 space-y-3 py-3">
          <div className="flex gap-2" role="group" aria-label="MCP 来源">
            <Button
              type="button"
              size="sm"
              variant={source === "manual" ? "default" : "outline"}
              onClick={() => setSource("manual")}
            >
              粘贴配置
            </Button>
            <Button
              type="button"
              size="sm"
              variant={source === "registry" ? "default" : "outline"}
              onClick={() => setSource("registry")}
            >
              官方目录
            </Button>
          </div>
          {source === "manual" ? (
            <SettingsSection title="快速填写" icon={<WandSparkles />}>
              <SettingCard
                compact
                icon={<ClipboardPaste />}
                title={
                  <Label htmlFor="mcp-install-input">
                    粘贴安装命令、网址或配置
                  </Label>
                }
                description="支持 Codex／Claude 安装命令、HTTP 网址、JSON 和 TOML，粘贴后自动填入下方。"
                details={
                  <div className="space-y-3">
                    <textarea
                      id="mcp-install-input"
                      className="min-h-20 w-full rounded-lg border border-border-default bg-background p-3 font-mono text-sm outline-none focus:border-border-active focus:ring-2 focus:ring-blue-500/20"
                      autoCorrect="off"
                      autoCapitalize="off"
                      spellCheck={false}
                      value={text}
                      onChange={(e) => setText(e.target.value)}
                      placeholder={
                        'codex mcp add hf-mcp-server --url "https://huggingface.co/mcp?login"'
                      }
                    />
                    {parsing && (
                      <p
                        role="status"
                        className="text-sm text-muted-foreground"
                      >
                        正在识别…
                      </p>
                    )}
                    {items.length > 1 && (
                      <div
                        className="flex flex-wrap gap-2"
                        role="group"
                        aria-label="选择要安装的 MCP"
                      >
                        {items.map((item, i) => (
                          <Button
                            type="button"
                            variant="outline"
                            key={i}
                            onClick={() => choose(item)}
                          >
                            {item.name || `服务 ${i + 1}`}
                          </Button>
                        ))}
                      </div>
                    )}
                  </div>
                }
              />
            </SettingsSection>
          ) : (
            <SettingsSection title="查找 MCP" icon={<Compass />}>
              <SettingCard
                compact
                icon={<Search />}
                title="官方 MCP 目录"
                description="按服务名称搜索；选中后仅填入配置，请核对服务地址、权限及登录要求。"
              >
                <div className="space-y-3">
                  <div className="flex gap-2">
                    <Input
                      aria-label="搜索官方 MCP 目录"
                      value={registryQuery}
                      disabled={registryBusy}
                      onChange={(event) => {
                        setRegistryQuery(event.target.value);
                        setRegistryItems([]);
                        setRegistrySearched(false);
                        setRegistryCursor(null);
                      }}
                      placeholder="例如 github"
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          event.preventDefault();
                          void searchRegistry();
                        }
                      }}
                    />
                    <Button
                      type="button"
                      variant="outline"
                      disabled={registryBusy || registryQuery.trim().length < 2}
                      onClick={() => void searchRegistry()}
                    >
                      {registryBusy ? "搜索中…" : "搜索"}
                    </Button>
                  </div>
                  {registryError && (
                    <p role="alert" className="text-sm text-destructive">
                      {registryError}
                    </p>
                  )}
                  {registrySearched && !registryItems.length && (
                    <p className="text-sm text-muted-foreground">
                      没有匹配的服务，请尝试服务名称中的英文关键词。
                    </p>
                  )}
                  {registryItems.map((item, index) => (
                    <div
                      key={`${item.name}-${index}`}
                      className="flex items-start justify-between gap-3 rounded-lg border border-border-default p-3"
                    >
                      <div className="min-w-0 space-y-1">
                        <p className="text-sm font-medium">
                          {item.name}{" "}
                          <span className="text-xs font-normal text-muted-foreground">
                            {item.version} · {item.source}
                          </span>
                        </p>
                        <p className="line-clamp-2 text-xs text-muted-foreground">
                          {item.description}
                        </p>
                      </div>
                      <Button
                        type="button"
                        size="sm"
                        variant="outline"
                        disabled={!item.definition}
                        title={
                          !item.definition
                            ? "此服务需要额外参数或凭据，请参考发布者说明手动配置"
                            : undefined
                        }
                        onClick={() => {
                          if (item.definition)
                            choose({
                              name: item.name,
                              definition: item.definition,
                            });
                        }}
                      >
                        {item.definition ? "填入" : "需手动配置"}
                      </Button>
                    </div>
                  ))}
                  {registryCursor && (
                    <Button
                      type="button"
                      variant="outline"
                      disabled={registryBusy}
                      onClick={() => void searchRegistry(registryCursor)}
                    >
                      {registryBusy ? "加载中…" : "加载更多"}
                    </Button>
                  )}
                </div>
              </SettingCard>
            </SettingsSection>
          )}
          {entry && (
            <>
              <SettingsSection title="服务配置" icon={<Settings2 />}>
                <SettingCard
                  compact
                  icon={<Tag />}
                  title={<Label htmlFor="mcp-install-name">名称</Label>}
                  description="用于在 Hub 和 Agent 中识别此 MCP。"
                >
                  <Input
                    className="w-52 sm:w-72"
                    id="mcp-install-name"
                    required
                    value={entry.name}
                    onChange={(e) =>
                      setEntry({ ...entry, name: e.target.value })
                    }
                  />
                </SettingCard>
              </SettingsSection>
              <McpDefinitionFields raw={raw} setRaw={setRaw} />
              <div className="space-y-3">
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
                      description:
                        "由 Studio 连接服务，可让多个 Agent 共用登录。",
                    },
                  ]}
                />
                {gateway && !local && (
                  <p className="text-xs text-muted-foreground">
                    连接时自动检查授权需求，需要时会提示登录。
                  </p>
                )}
                {gateway && !local && (
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
                {gateway && (check.data?.issue || check.error) && (
                  <p role="alert" className="text-sm text-amber-600">
                    {check.data?.issue || String(check.error)}。可改用 Agent
                    直连或修改上方配置。
                  </p>
                )}
              </div>
              <p className="text-xs text-muted-foreground">
                添加到 MCP Hub 后，在 Agent 页面全局启用，或在项目页面配置项目
                MCP。
              </p>
            </>
          )}
          {error && (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          )}
        </fieldset>
      </div>
      <footer className="-mx-6 flex shrink-0 items-center justify-end gap-2 border-t border-border-default bg-background px-6 py-4">
        <Button
          type="button"
          variant="outline"
          disabled={busy}
          onClick={onClose}
        >
          取消
        </Button>
        <Button type="submit" disabled={busy || !!invalid}>
          {busy ? phase : "添加到 Hub"}
        </Button>
      </footer>
    </form>
  );
}
