import type { AgentInfo } from "@/types";

// Older remote helpers predate projectSkillDir and only support these two Agents.
const LEGACY_PROJECT_DIRS: Record<string, string> = {
  "claude-code": ".claude/skills",
  codex: ".agents/skills",
};

export function projectSkillDirectory(agent: AgentInfo): string | null {
  if (!agent.supportsProjectSkills) return null;
  return agent.projectSkillDir ?? LEGACY_PROJECT_DIRS[agent.id] ?? null;
}

/**
 * 未检测到 Agent 时的补充说明，与旧侧栏的「未装 / 检测中 / 检测失败」同口径。
 * `notInstalled` 只在检测已完成且确实没找到时为真 —— 检测中或检测失败时不能断定没装。
 */
export function agentDetection(
  agent: AgentInfo,
  probe: { isDetecting: boolean; detectionFailed: boolean },
): { note?: string; notInstalled: boolean } {
  if (agent.detected) return { notInstalled: false };
  if (probe.isDetecting) return { note: "检测中", notInstalled: false };
  if (probe.detectionFailed) return { note: "检测失败", notInstalled: false };
  return { note: "未检测到安装", notInstalled: true };
}
