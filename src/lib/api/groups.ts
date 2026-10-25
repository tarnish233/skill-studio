import { invoke } from "@/lib/api/transport";
import type { Group, UntouchedCopy } from "@/types";

export const groupsApi = {
  /**
   * 分组不属于某个 Agent：保存名称和成员，是否对某 Agent 生效由 enable 单独决定。
   * `expected` 是打开编辑时的样子：别处（另一个窗口、命令行）改过就拒绝保存，不覆盖
   */
  async save(
    groupId: string | null,
    name: string,
    skillIds: string[],
    expected: { name: string; skillIds: string[] } | null = null,
  ): Promise<Group> {
    return await invoke("save_group", {
      groupId,
      name,
      skillIds,
      expected,
    });
  },
  /**
   * 为一个 Agent 启用或停用一个分组，它叠加的其他分组不变。后端按最新的启用记录计算，
   * 不会撤掉别处刚启用的分组。返回原样留下的副本
   */
  async enable(
    agentId: string,
    groupId: string,
    enabled: boolean,
  ): Promise<UntouchedCopy[]> {
    return await invoke("set_agent_group", { agentId, groupId, enabled });
  },
  /** 按最新的启用记录重新写入这个 Agent 叠加的全部分组（应用修改） */
  async reapply(agentId: string): Promise<UntouchedCopy[]> {
    return await invoke("reapply_agent_groups", { agentId });
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
