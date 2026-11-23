import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { SOURCE_LABELS } from "@/components/common/SourceMark";
import { useTarget } from "@/components/targets/TargetProvider";
import { usePluginInventory } from "@/hooks/usePlugins";
import type { PluginPackage } from "@/lib/api/plugins";

export const pluginFormats: Record<string, string> = {
  "agent-plugins-v1": "Agent Plugins v1",
  claude: "Claude 原生包",
  codex: "Codex 原生包",
  "pi-package": "Pi Package",
  opencode: "OpenCode 扩展",
  "dsh-bundle": "DSH Bundle",
  extension: "JS/TS 扩展",
  unknown: "格式未确认",
};
const componentNames: Record<string, string> = {
  skill: "Skill",
  mcp: "MCP",
  extension: "扩展",
  hook: "Hook",
  agent: "Agent",
  command: "命令",
  lsp: "LSP",
  prompt: "提示词",
  theme: "主题",
};

export function PluginPackageDetails({ value }: { value: PluginPackage }) {
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="secondary">
          {pluginFormats[value.format] ?? value.format}
        </Badge>
        {value.version && <Badge variant="outline">v{value.version}</Badge>}
      </div>
      {value.description && (
        <p className="text-sm text-muted-foreground">{value.description}</p>
      )}
      {value.root && (
        <p className="break-all font-mono text-xs text-muted-foreground">
          {value.root}
        </p>
      )}
      {value.error && (
        <p role="alert" className="text-sm text-destructive">
          {value.error}
        </p>
      )}
      {!!value.warnings.length && (
        <ul className="space-y-1 text-xs text-amber-700 dark:text-amber-400">
          {value.warnings.map((warning, index) => (
            <li key={index}>{warning}</li>
          ))}
        </ul>
      )}
      {!!value.extensions.length && (
        <p className="text-xs text-muted-foreground">
          客户端扩展：{value.extensions.join("、")}（不推断跨 Agent 可用性）
        </p>
      )}
      <div className="space-y-2">
        <h3 className="text-sm font-medium">
          声明的组件 · {value.components.length}
        </h3>
        {value.components.length ? (
          <ul className="divide-y rounded-lg border border-border-default">
            {value.components.map((component, index) => (
              <li key={index} className="space-y-1 px-3 py-2">
                <div className="flex items-center gap-2">
                  <Badge variant="outline">
                    {componentNames[component.kind] ?? component.kind}
                  </Badge>
                  <span className="text-sm">{component.name}</span>
                  <span className="ml-auto text-xs text-muted-foreground">
                    只读
                  </span>
                </div>
                {component.path && (
                  <p className="break-all font-mono text-[11px] text-muted-foreground">
                    {component.path}
                  </p>
                )}
                {component.issue && (
                  <p className="text-xs text-destructive">{component.issue}</p>
                )}
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-xs text-muted-foreground">
            未发现可静态枚举的组件；不会执行扩展代码探测能力。
          </p>
        )}
      </div>
      <p className="text-xs text-muted-foreground">
        组件仅关联展示，不拆包托管或重复部署。清单不是当前 Agent
        会话已加载的证明。
      </p>
    </div>
  );
}

export function PluginComponents({
  kind,
  onOpenPlugin,
}: {
  kind: "skill" | "mcp";
  onOpenPlugin?: (id: string) => void;
}) {
  const local = useTarget().id === "local";
  const inventory = usePluginInventory();
  const rows = (inventory.data?.plugins ?? [])
    .filter((plugin) => plugin.enabled !== false)
    .flatMap((plugin) =>
      plugin.package.components
        .filter((component) => component.kind === kind)
        .map((component, index) => ({ plugin, component, index })),
    );
  if (!local || !rows.length) return null;
  return (
    <details className="mt-5 rounded-xl border border-border-default px-4 py-3">
      <summary className="cursor-pointer text-sm font-medium">
        插件提供的 {kind === "skill" ? "Skills" : "MCP"} · {rows.length}
      </summary>
      <p className="mt-3 text-xs text-muted-foreground">
        由配置未停用的完整插件提供，只读关联；不计入独立 Hub
        部署，不提供单项托管或删除。
      </p>
      <ul className="mt-2 divide-y">
        {rows.map(({ plugin, component, index }) => (
          <li key={plugin.id + index} className="flex items-center gap-3 py-2">
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm">{component.name}</p>
              <p className="truncate text-xs text-muted-foreground">
                由 {plugin.package.name} 提供 ·{" "}
                {SOURCE_LABELS[plugin.agentId] ?? plugin.agentId}
              </p>
              {component.issue && (
                <p className="text-xs text-destructive">{component.issue}</p>
              )}
            </div>
            <Badge variant="outline">插件组件 · 只读</Badge>
            {onOpenPlugin && (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => onOpenPlugin(plugin.id)}
                aria-label={"查看所属插件 " + plugin.package.name}
              >
                查看插件
              </Button>
            )}
          </li>
        ))}
      </ul>
    </details>
  );
}
