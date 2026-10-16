import { invoke } from "@/lib/api/transport";
import type { Group } from "@/types";

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
  /** 设置这个 Agent 当前叠加的全部分组；空数组表示全部停用 */
  async activate(agentId: string, groupIds: string[]): Promise<void> {
    return await invoke("activate_agent_group", { agentId, groupIds });
  },
  async list(): Promise<Group[]> {
    return await invoke("list_groups");
  },
  /** 正在被任一 Agent 使用的分组需先停用；只删分组记录和项目绑定，不动文件 */
  async remove(groupId: string): Promise<void> {
    await invoke("delete_group", { groupId });
  },
  async reorder(groupIds: string[]): Promise<Group[]> {
    return await invoke("reorder_groups", { groupIds });
  },
};
