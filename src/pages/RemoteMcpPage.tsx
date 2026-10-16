import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Plug } from "lucide-react";
import { AgentIcon } from "@/components/common/AgentIcon";
import { EmptyState } from "@/components/common/EmptyState";
import { ListContainer, ListItemRow } from "@/components/common/ListItemRow";
import { PageTools } from "@/components/common/PageTools";
import {
  SummaryBar,
  SummaryStart,
  SummaryEnd,
  summaryPillClass,
} from "@/components/common/SummaryBar";
import { useTarget } from "@/components/targets/TargetProvider";
import { useSettings } from "@/hooks/useData";
import { requestRemote } from "@/lib/api/transport";
import { MCP_AGENTS, mcpAppId } from "@/lib/mcpAgents";

interface RemoteMcpInventory {
  servers: { id: string; name: string; agents: string[] }[];
  warnings: string[];
}

/** Remote metadata only: never fetch the local management catalog or gateway. */
export function RemoteMcpPage() {
  const target = useTarget();
  const { data: settings } = useSettings();
  const [query, setQuery] = useState("");
  const inventory = useQuery({
    queryKey: ["mcp", "inventory", target.id],
    queryFn: () => requestRemote<RemoteMcpInventory>(target.id, "scan_mcp"),
    enabled: target.id !== "local" && target.connected,
    staleTime: 30_000,
    retry: false,
  });
  const servers = (inventory.data?.servers ?? [])
    .map((server) => ({
      ...server,
      agents: MCP_AGENTS.filter(
        (agent) =>
          server.agents.includes(agent.id) &&
          !settings?.disabledAgents?.includes(mcpAppId(agent.id)),
      ),
    }))
    .filter((server) => server.agents.length > 0);
  const filtered = servers.filter((server) =>
    `${server.name} ${server.agents.map((a) => a.name).join(" ")}`
      .toLowerCase()
      .includes(query.trim().toLowerCase()),
  );
  const error = String(inventory.error ?? "");
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageTools
        query={query}
        onQueryChange={setQuery}
        placeholder="搜索 MCP 或 Agent…"
        createLabel="添加 MCP"
        createDisabledReason="远程 MCP 仅支持只读展示，暂不支持添加。"
      />
      <SummaryBar>
        <SummaryStart>
          <Plug className="h-4 w-4 shrink-0 text-muted-foreground" />
          <span className="whitespace-nowrap text-xs text-muted-foreground">
            远程 MCP · 仅展示服务器已有配置
          </span>
        </SummaryStart>
        <SummaryEnd>
          <span className={summaryPillClass}>
            已配置 {inventory.data ? servers.length : "—"} 个 MCP
          </span>
          <span className={`${summaryPillClass} text-muted-foreground`}>
            只读
          </span>
        </SummaryEnd>
      </SummaryBar>
      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto pb-6">
        {!target.connected && (
          <p role="alert" className="text-sm text-muted-foreground">
            服务器已断开，重新连接后可刷新 MCP。
          </p>
        )}
        {inventory.isError && (
          <p role="alert" className="text-sm text-destructive">
            {/不支持的(操作|管理命令)/.test(error)
              ? "远程组件尚不支持 MCP 展示，请更新组件后重新连接。"
              : `读取远程 MCP 失败：${error}`}
          </p>
        )}
        {inventory.data?.warnings.map((warning, i) => (
          <p key={i} role="status" className="text-xs text-amber-600">
            {warning}
          </p>
        ))}
        {inventory.isLoading ? (
          <p
            role="status"
            className="py-8 text-center text-sm text-muted-foreground"
          >
            正在读取服务器 MCP…
          </p>
        ) : target.connected && !inventory.isError && filtered.length === 0 ? (
          <EmptyState
            icon={Plug}
            title={query.trim() ? "没有匹配的 MCP" : "未发现远程 MCP 配置"}
            description="扫描 Agent 的全局配置和已登记项目配置。"
          />
        ) : (
          <ListContainer cards>
            {filtered.map((server) => (
              <ListItemRow key={server.id} card className="hover:bg-card">
                <div className="flex min-w-0 flex-wrap items-center gap-2">
                  <span className="break-all text-sm font-medium">
                    {server.name}
                  </span>
                  {server.agents.map((agent) => (
                    <span
                      key={agent.id}
                      title={agent.name}
                      className="inline-flex items-center gap-1.5 rounded-full bg-muted px-2 py-1 text-xs text-muted-foreground"
                    >
                      <AgentIcon agentId={agent.appId} size="sm" />
                      <span className="sr-only">{agent.name}</span>
                    </span>
                  ))}
                </div>
              </ListItemRow>
            ))}
          </ListContainer>
        )}
      </div>
    </div>
  );
}
