import {
  mcpRequest,
  type McpServer,
  type McpSource,
  type DiscoveredMcp,
} from "./mcp";
export type Definition = Record<string, unknown>;
export const MCP_STATUS_STALE_TIME = 30_000;
export const MCP_STATUS_POLL_INTERVAL = 30_000;
export interface ParsedMcp {
  name: string;
  definition: Definition;
  agent?: string;
  scope?: "user" | "project" | "local";
}
export interface McpInstallResult {
  gatewayStarted?: boolean;
  loginUrl?: string;
  setupError?: string;
  installed?: boolean;
}
export interface ManagedBinding {
  id: string;
  agent: string;
  path: string;
  project: string | null;
  key: string;
  original: Definition | null;
  installed: Definition;
}
export interface ManagedMcp {
  id: string;
  name: string;
  mode: "direct" | "gateway";
  definition: Definition;
  oauth: boolean;
  clientId: string | null;
  scopes: string[];
  bindings: ManagedBinding[];
}
export interface McpGroup {
  references?: ManagedMcp[];
  id: string;
  agent: string;
  name: string;
  entryIds: string[];
  sortOrder: number;
}
export interface ActiveMcpGroup {
  groupId: string;
  entries: ManagedMcp[];
  bindings: ManagedBinding[];
}
export interface ManagementStatus {
  backups?: {
    id: string;
    paths: string[];
    createdAt: number;
  }[];
  builtins?: { name: string; agent: string; path: string; scope: string }[];
  groupIssues?: Record<string, string>;
  groups?: McpGroup[];
  activeGroups?: Record<string, ActiveMcpGroup>;
  gatewayOutdated?: boolean;
  running: boolean;
  entries: ManagedMcp[];
  servers: McpServer[];
  discovered: DiscoveredMcp[];
  scanWarnings: string[];
}
export interface McpRow {
  entry: ManagedMcp;
  managed: boolean;
  sources: McpSource[];
  authorized?: boolean;
  authRequired?: boolean;
}
export function canonical(
  definition: Definition,
  agent = "claude",
): Definition {
  const value = { ...definition };
  if (agent === "codex" && "http_headers" in value) {
    value.headers = value.http_headers;
    delete value.http_headers;
  }
  if (agent === "opencode" && typeof value.disabled === "boolean") {
    value.enabled = !value.disabled;
    delete value.disabled;
  }
  if (agent === "opencode") {
    if (Array.isArray(value.command) && value.command.length) {
      const [command, ...args] = value.command;
      value.command = command;
      value.args = args;
    }
    if ("environment" in value) {
      value.env = value.environment;
      delete value.environment;
    }
    if (value.type === "local") value.type = "stdio";
    else if (value.type === "remote") value.type = "http";
  }
  value.type ??= value.url ? "http" : "stdio";
  if (value.type === "streamable-http") value.type = "http";
  return value;
}
function stable(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stable).join(",")}]`;
  if (value && typeof value === "object")
    return `{${Object.entries(value)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([k, v]) => `${JSON.stringify(k)}:${stable(v)}`)
      .join(",")}}`;
  return JSON.stringify(value);
}
export function rows(status?: ManagementStatus): McpRow[] {
  if (!status) return [];
  const entries = status.entries ?? [];
  const result: McpRow[] = entries.map((entry) => ({
    entry,
    managed: true,
    sources: [],
    authorized: status.servers?.find((s) => s.id === entry.id)?.authorized,
    authRequired: status.servers?.find((s) => s.id === entry.id)?.authRequired,
  }));
  for (const discovered of status.discovered ?? []) {
    const reference = (status.groups ?? [])
      .flatMap((g) => g.references ?? [])
      .find(
        (entry) =>
          entry.id === discovered.id ||
          entry.bindings.some((b) =>
            discovered.sources.some(
              (source) =>
                b.agent === source.agent &&
                b.path === source.path &&
                b.key === source.key &&
                b.project === (source.project ?? null),
            ),
          ) ||
          Object.values(status.activeGroups ?? {}).some((group) =>
            group.bindings.some(
              (b) =>
                b.id === entry.id &&
                discovered.sources.some(
                  (source) =>
                    b.agent === source.agent &&
                    b.path === source.path &&
                    b.key === source.key &&
                    b.project === (source.project ?? null),
                ),
            ),
          ),
      );
    const source =
      discovered.sources.find((source) =>
        reference?.bindings.some(
          (b) =>
            b.agent === source.agent &&
            b.path === source.path &&
            b.key === source.key &&
            b.project === (source.project ?? null),
        ),
      ) ?? discovered.sources[0];
    const definition = canonical(source?.definition ?? {}, source?.agent);
    const match = result.find(
      (row) =>
        row.entry.id === discovered.managedId ||
        [
          ...row.entry.bindings,
          ...Object.values(status.activeGroups ?? {}).flatMap((g) =>
            g.bindings.filter((b) => b.id === row.entry.id),
          ),
        ].some((b) =>
          discovered.sources.some(
            (s) =>
              b.agent === s.agent &&
              b.path === s.path &&
              b.project === (s.project ?? null) &&
              b.key === s.key,
          ),
        ) ||
        stable(canonical(row.entry.definition)) === stable(definition),
    );
    if (match) {
      match.sources.push(...discovered.sources);
      continue;
    }
    result.push({
      managed: false,
      sources: discovered.sources,
      entry: {
        id: reference?.id ?? discovered.id,
        name: discovered.name,
        mode: "direct",
        definition,
        oauth: false,
        clientId: null,
        scopes: [],
        bindings: [],
      },
    });
  }
  return result;
}
export const managementApi = {
  list: () => mcpRequest<ManagementStatus>("list"),
  parse: (text: string) => mcpRequest<ParsedMcp[]>("parse", { text }),
};
