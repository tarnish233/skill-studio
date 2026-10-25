// Navigation reuses snapshots; explicit invalidation still refreshes immediately.
export const NAVIGATION_STALE_TIME = 30_000;

/** 集中管理 query key，避免各处手写字符串对不上 */
export const queryKeys = {
  agents: ["agents"] as const,
  agentProbe: ["agents", "cli-probe"] as const,
  skills: ["skills"] as const,
  groups: ["groups"] as const,
  projects: ["projects"] as const,
  settings: ["settings"] as const,
  config: ["config"] as const,
  skillBackups: ["skill-backups"] as const,
  backups: ["backups"] as const,
  /** 纳入托管、移出 Hub 时保存的完整备份 */
  hubBackups: ["hub-backups"] as const,
  mcpBackups: ["mcp-backups"] as const,
  /** MCP 状态：Hub、分组、项目页共用一份，轮询与事件刷新都按这个 key */
  mcpStatus: ["mcp", "local"] as const,
  version: ["version"] as const,
  /** 本机命令行程序的位置 */
  cliInfo: ["cli-info"] as const,
};
