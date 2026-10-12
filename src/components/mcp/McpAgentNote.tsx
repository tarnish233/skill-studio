import { HelpPopover } from "@/components/common/HelpPopover";
import { mcpAgentName } from "@/lib/mcpAgents";

function NoteText({ agent }: { agent: string }) {
  return agent === "pi" ? (
    <>
      Pi 0.99.0 及以上原生支持 MCP，无需安装扩展。保存后在 Pi 中运行{" "}
      <code className="select-text">/reload</code>，通过{" "}
      <code className="select-text">/mcp</code> 查看连接状态。项目配置需先在 Pi
      中信任该项目。
    </>
  ) : (
    <>
      适用于 OpenCode v2；修改后请在 OpenCode 中重新加载 MCP。旧版配置需先迁移到
      v2。
    </>
  );
}

export function McpAgentNote({
  agent,
  compact = false,
}: {
  agent: string;
  compact?: boolean;
}) {
  if (agent !== "pi" && agent !== "opencode") return null;
  if (!compact)
    return (
      <p className="rounded-xl border border-border-default bg-muted/40 p-3 text-xs text-muted-foreground">
        <NoteText agent={agent} />
      </p>
    );
  return (
    <HelpPopover label={`${mcpAgentName(agent)} MCP 使用说明`}>
      <NoteText agent={agent} />
    </HelpPopover>
  );
}
