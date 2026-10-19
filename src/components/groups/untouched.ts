import type { McpUntouched } from "@/lib/api/mcpManagement";
import { mcpAgentName } from "@/lib/mcpAgents";
import type { UntouchedCopy } from "@/types";

/**
 * 启停分组后附在提示里的一句话：部署后被改过的副本 / 接入，Studio 没有删除或覆盖。
 * 还在用的分组继续管理它们（卡片上显示需要检查），不再需要的交还给用户。
 */
export function untouchedSkillNote(list?: UntouchedCopy[] | null) {
  if (!list?.length) return undefined;
  const items = list.map((u) => `${u.name}（${u.reason}）`).join("、");
  return `${items}：Studio 没有删除或覆盖，保持原样${
    list.some((u) => !u.tracked)
      ? "；不再由分组管理的留在 Agent 目录里，可在 Skill Hub 处理"
      : ""
  }`;
}

export function untouchedMcpNote(list?: McpUntouched[] | null) {
  if (!list?.length) return undefined;
  const items = list
    .map((u) => `${mcpAgentName(u.agent)}（${u.path}）里的「${u.key}」`)
    .join("、");
  return `${items}已被其他程序改写，Studio 没有删除或覆盖，保持原样${
    list.some((u) => !u.tracked)
      ? "；不再由分组管理的会作为原有配置出现在 MCP Hub 里"
      : ""
  }`;
}
