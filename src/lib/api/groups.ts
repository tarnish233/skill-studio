import { invoke } from "@/lib/api/transport";
import type { Group, GroupApplyMode, LinkReport } from "@/types";

export const groupsApi = {
  /** 分组不属于某个 Agent：保存名称和成员，是否对某 Agent 生效由 activate 单独决定 */
  async save(
    groupId: string | null,
    name: string,
    skillIds: string[],
  ): Promise<Group> {
    return await invoke("save_group", {
      groupId,
      name,
      skillIds,
    });
  },
  async activate(agentId: string, groupId: string | null): Promise<void> {
    return await invoke("activate_agent_group", { agentId, groupId });
  },
  async list(): Promise<Group[]> {
    return await invoke("list_groups");
  },

  async create(
    name: string,
    description?: string,
    icon?: string,
  ): Promise<Group> {
    return await invoke("create_group", {
      name,
      description: description ?? null,
      icon: icon ?? null,
    });
  },

  async update(
    groupId: string,
    patch: { name?: string; description?: string; icon?: string },
  ): Promise<Group> {
    return await invoke("update_group", {
      groupId,
      name: patch.name ?? null,
      description: patch.description ?? null,
      icon: patch.icon ?? null,
    });
  },

  /** 只删元数据，不撤销已有注册 */
  async remove(groupId: string): Promise<void> {
    await invoke("delete_group", { groupId });
  },

  async setSkills(groupId: string, skillIds: string[]): Promise<Group> {
    return await invoke("set_group_skills", { groupId, skillIds });
  },

  async reorder(groupIds: string[]): Promise<Group[]> {
    return await invoke("reorder_groups", { groupIds });
  },

  /** 一次性 Add / Remove */
  async apply(
    groupId: string,
    agentIds: string[],
    mode: GroupApplyMode,
    force = false,
  ): Promise<LinkReport> {
    return await invoke("apply_group", { groupId, agentIds, mode, force });
  },
};
