import { invoke } from "./transport";
export interface McpServer {
  id: string;
  name: string;
  transport: "stdio" | "http";
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string | null;
  url?: string;
  headers?: Record<string, string>;
  enabled: boolean;
  oauth: boolean;
  authorized?: boolean;
  authRequired?: boolean;
  clientId?: string | null;
  scopes?: string[];
}
export const mcpRequest = <T>(method: string, params: unknown = {}) =>
  invoke<T>("mcp_request", { method, params });

export interface McpProjectOverride {
  path: string;
  scope: string;
  key: string;
  /** Null when the override changes exposure without setting enabled. */
  enabled: boolean | null;
}
export interface McpSource {
  id: string;
  project: string | null;
  definition: Record<string, unknown>;
  agent: string;
  path: string;
  scope: string;
  key: string;
  enabled: boolean;
  gateway: boolean;
  /** Pi project entries that only override this user-level server. */
  projectOverrides?: McpProjectOverride[];
}
export interface DiscoveredMcp {
  id: string;
  name: string;
  server: McpServer | null;
  sources: McpSource[];
  issue: string | null;
  managedId: string | null;
}
