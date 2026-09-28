import { SettingCard, SettingsSection } from "@/components/common/SettingCard";
import { Route, Settings2, Tag } from "lucide-react";
import { McpChoiceCards } from "./McpChoiceCards";
import { McpDefinitionFields, readDefinition } from "./McpDefinitionFields";
import { McpQuickInstall } from "./McpQuickInstall";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useUnsavedProject } from "@/components/common/NavigationGuard";
import { mcpRequest } from "@/lib/api/mcp";
import {
  type ManagedMcp,
  type McpRow,
  type McpInstallResult,
} from "@/lib/api/mcpManagement";

type EditorProps = {
  row: McpRow | null;
  onClose: () => void;
  onSaved: (entry: ManagedMcp, result?: McpInstallResult) => void;
};
export function McpEditor(props: EditorProps) {
  return props.row ? (
    <McpExistingEditor {...props} />
  ) : (
    <McpQuickInstall onClose={props.onClose} onSaved={props.onSaved} />
  );
}
function McpExistingEditor({ row, onClose, onSaved }: EditorProps) {
  const existing = row?.managed;
  const [entry, setEntry] = useState<ManagedMcp>(() =>
    row
      ? { ...row.entry, id: existing ? row.entry.id : crypto.randomUUID() }
      : {
          id: crypto.randomUUID(),
          name: "",
          mode: "direct",
          definition: { type: "http", url: "" },
          oauth: false,
          clientId: null,
          scopes: [],
          bindings: [],
        },
  );
  const [raw, setRaw] = useState(() =>
    JSON.stringify(entry.definition, null, 2),
  );
  const [busy, setBusy] = useState(false);
  const [original] = useState(() => JSON.stringify({ entry, raw }));
  useUnsavedProject(JSON.stringify({ entry, raw }) !== original, busy);
  const { definition, error } = readDefinition(raw);
  const effective = {
    ...entry,
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
    enabled: entry.mode === "gateway" && !error,
    retry: false,
  });
  const local = definition.type === "stdio";
  return (
    <form
      className="flex min-h-0 flex-1 flex-col"
      onSubmit={(e) => {
        e.preventDefault();
        if (busy || error) return;
        setBusy(true);
        void mcpRequest("saveEntry", {
          entry: effective,
          expectedEntry: existing ? row?.entry : null,
          bindingIds: entry.bindings.map((b) => b.id),
          sources: [],
          agents: [],
          projectId: "",
          scope: "user",
        })
          .then(() =>
            onSaved(effective, { installed: entry.bindings.length > 0 }),
          )
          .catch((e) => toast.error(String(e)))
          .finally(() => setBusy(false));
      }}
    >
      <div className="-mx-1 min-h-0 flex-1 overflow-y-auto px-1">
        <fieldset disabled={busy} className="min-w-0 space-y-3 py-3">
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
            {entry.bindings.length
              ? "保存会同步已有接入；分组中的配置需在 Agent 页面应用修改。接入位置请在 Agent 或项目页面管理。"
              : "保存到 MCP Hub 后，在 Agent 或项目页面选择使用。原有配置保持不变。"}
          </p>
        </fieldset>
      </div>
      <footer className="-mx-6 flex shrink-0 justify-end gap-2 border-t border-border-default bg-background px-6 py-4">
        <Button
          type="button"
          variant="outline"
          disabled={busy}
          onClick={onClose}
        >
          取消
        </Button>

        <Button
          type="submit"
          disabled={
            busy ||
            !!error ||
            (entry.mode === "gateway" &&
              (gatewayCheck.isPending ||
                !!gatewayCheck.data?.issue ||
                !!gatewayCheck.error))
          }
        >
          {busy ? "正在保存…" : "保存到 Hub"}
        </Button>
      </footer>
    </form>
  );
}
