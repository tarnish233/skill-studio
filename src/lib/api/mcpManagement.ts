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
  /** Stable add/adoption history, independent of current Agent connections. Missing on older records. */
  provenance?: {
    sourceIds: string[];
    originalSources: {
      agent: string;
      path: string;
      project: string | null;
      key: string;
    }[];
  } | null;
  bindings: ManagedBinding[];
}
/** 分组不归属任何单个 agent：同一个分组可以被不同 agent 各自独立激活 */
export interface McpGroup {
  references?: ManagedMcp[];
  id: string;
  name: string;
  entryIds: string[];
  sortOrder: number;
}
/** 一个项目的 MCP：成员只保存一次，每个 Agent 独立决定是否写进这个项目 */
export interface McpProject {
  root: string;
  entryIds: string[];
  groupIds: string[];
  /** 选中的原有配置引用（不托管、不改动） */
  references?: ManagedMcp[];
  /** Claude Code 写到项目本地（~/.claude.json 里这个项目的配置），而不是项目里的 .mcp.json */
  claudeLocal: boolean;
  /** MCP agent id → 写进这个项目的成员快照与接入 */
  active: Record<string, { entries: ManagedMcp[]; bindings: ManagedBinding[] }>;
}
export interface ActiveMcpGroup {
  groupId: string;
  entries: ManagedMcp[];
  bindings: ManagedBinding[];
}
/** 删除托管、移除项目接入时原样留下的接入：配置里同名的那项已被其他程序改写，不是 Studio 写的了 */
export interface McpForeign {
  agent: string;
  path: string;
  project: string | null;
  key: string;
}
/** 启停分组时 Studio 没有动的接入：部署后已被其他程序改写（包括被手动停用） */
export interface McpUntouched extends McpForeign {
  /** 仍在用它的分组继续管理它；false 表示分组已不再管理，作为原有配置留下 */
  tracked: boolean;
}
export interface McpRemoval {
  /** Studio 撤回（或还原成原配置）的接入数 */
  removed: number;
  foreign: McpForeign[];
}
/** 一次 MCP 写入的备份。状态在打开备份列表时按当前配置判断。 */
export interface McpBackupView {
  id: string;
  createdAt: number;
  paths: string[];
  size: number;
  /** 现在能恢复 / 先恢复更新的备份才能恢复 / 已无法恢复 */
  status: "restorable" | "afterNewer" | "unrestorable";
  reason?: string | null;
}
/** 旧版本写在配置文件旁边、不在任何备份记录里的文件 */
export interface McpLegacyBackup {
  path: string;
  size: number;
}
export interface McpBackupList {
  backups: McpBackupView[];
  legacy: McpLegacyBackup[];
}
export interface ManagementStatus {
  backups?: {
    id: string;
    paths: string[];
    createdAt: number;
  }[];
  builtins?: { name: string; agent: string; path: string; scope: string }[];
  /** agent → 分组 id → 这个分组在该 Agent 上的部署问题（接入被外部修改或移除） */
  groupIssues?: Record<string, Record<string, string>>;
  groups?: McpGroup[];
  /** agent → 当前叠加的分组记录；部署内容是这些分组成员的并集 */
  activeGroups?: Record<string, ActiveMcpGroup[]>;
  /** 项目 id → 项目的 MCP 成员与各 Agent 的部署 */
  projects?: Record<string, McpProject>;
  /** 项目 id → MCP agent id → 写进去的接入已被外部修改或移除 */
  projectIssues?: Record<string, Record<string, string>>;
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
/** Same source semantics as Skill Hub: stable history, not current Agent connections. */
export function mcpSourceIds(row: McpRow): string[] {
  const ids = row.managed
    ? row.entry.provenance?.sourceIds
    : row.sources.map((source) => source.agent);
  return [
    ...new Set(
      ids?.length
        ? ids.map((id) => (id === "builtin" ? "studio" : id))
        : ["unknown"],
    ),
  ];
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
/** 部署记录里的成员快照与 Hub 里现在的定义比较：变了就是「有待应用修改」 */
export function entrySnapshot(entry: ManagedMcp) {
  return JSON.stringify([
    entry.id,
    entry.name,
    entry.mode,
    entry.definition,
    entry.oauth,
    entry.clientId,
    entry.scopes,
  ]);
}

/** 分组和项目写进 Agent 配置的接入：它们不属于 Hub 条目，由启用它们的分组 / 项目管理 */
export function ownedBindings(status?: ManagementStatus): ManagedBinding[] {
  return [
    ...Object.values(status?.activeGroups ?? {})
      .flat()
      .flatMap((g) => g.bindings),
    ...Object.values(status?.projects ?? {})
      .flatMap((p) => Object.values(p.active ?? {}))
      .flatMap((d) => d.bindings),
  ];
}

export function rows(status?: ManagementStatus): McpRow[] {
  if (!status) return [];
  const owned = ownedBindings(status);
  const entries = status.entries ?? [];
  const result: McpRow[] = entries.map((entry) => ({
    entry,
    managed: true,
    sources: [],
    authorized: status.servers?.find((s) => s.id === entry.id)?.authorized,
    authRequired: status.servers?.find((s) => s.id === entry.id)?.authRequired,
  }));
  for (const discovered of status.discovered ?? []) {
    const reference = [
      ...(status.groups ?? []).flatMap((g) => g.references ?? []),
      ...Object.values(status.projects ?? {}).flatMap(
        (p) => p.references ?? [],
      ),
    ].find(
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
        owned.some(
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
          ...owned.filter((b) => b.id === row.entry.id),
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
