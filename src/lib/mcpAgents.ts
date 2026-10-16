/** IDs here match the MCP backend; Skill uses claude-code instead of claude. */
export const MCP_AGENTS = [
  { id: "claude", appId: "claude-code", name: "Claude Code" },
  { id: "codex", appId: "codex", name: "Codex" },
  { id: "opencode", appId: "opencode", name: "OpenCode" },
  { id: "pi", appId: "pi", name: "Pi" },
  { id: "grok", appId: "grok", name: "Grok Build" },
] as const;
export const mcpAgentName = (id?: string) =>
  MCP_AGENTS.find((a) => a.id === id)?.name ?? id ?? "Agent";
export const mcpAppId = (id: string) =>
  MCP_AGENTS.find((a) => a.id === id)?.appId ?? id;
/** 反向映射：Skill 侧 appId（如 "claude-code"）→ MCP 后端的 agent id（如 "claude"） */
export const mcpAgentId = (appId: string) =>
  MCP_AGENTS.find((a) => a.appId === appId)?.id ?? appId;
export const supportsMcp = (appId: string) =>
  MCP_AGENTS.some((a) => a.appId === appId);
export const MCP_PROJECT_SUFFIXES = [
  "/.mcp.json",
  "/.codex/config.toml",
  "/opencode.json",
  "/opencode.jsonc",
  "/.opencode/opencode.json",
  "/.opencode/opencode.jsonc",
  "/.pi/mcp.json",
  "/.grok/config.toml",
];

const normalize = (path: string) => path.replace(/\/+$/, "");
/** 这条接入是否写在该项目里（项目本地或项目配置文件），而不是用户全局 */
export const inProject = (
  binding: { path: string; project: string | null },
  project: { root: string },
) =>
  binding.project === project.root ||
  MCP_PROJECT_SUFFIXES.some(
    (suffix) => binding.path === normalize(project.root) + suffix,
  );
