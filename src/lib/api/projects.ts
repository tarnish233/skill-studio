import { invoke } from "@/lib/api/transport";
import type {
  LinkMode,
  LinkReport,
  ProjectBinding,
  TokenEstimate,
} from "@/types";

/** 项目成员：单独选的 skill、绑定的分组与写入方式 */
export interface ProjectMembers {
  skillIds: string[];
  groupIds: string[];
  linkMode: LinkMode;
}

export interface ProjectLocalSkill {
  tokens?: TokenEstimate;
  disabled: boolean;
  storagePath: string;
  collected: boolean;
  managed: boolean;
  name: string;
  path: string;
  agentId: string;
  frontmatter?: {
    description?: string;
    malformed: boolean;
    error?: string;
  };
}

export const projectsApi = {
  async collectLocal(projectId: string, path: string): Promise<void> {
    return await invoke("collect_project_skill", { projectId, path });
  },
  async setLocalEnabled(
    projectId: string,
    path: string,
    enabled: boolean,
  ): Promise<void> {
    return await invoke("set_project_skill_enabled", {
      projectId,
      path,
      enabled,
    });
  },
  async deleteLocalSkill(projectId: string, path: string): Promise<void> {
    return await invoke("delete_project_local_skill", { projectId, path });
  },
  async localSkills(projectId: string): Promise<ProjectLocalSkill[]> {
    return await invoke("list_project_skills", { projectId });
  },
  /** 为一个 Agent 启用或停用项目，成员用已保存的；后端按最新状态计算，不影响其他 Agent */
  async setAgent(
    projectId: string,
    agentId: string,
    enabled: boolean,
  ): Promise<LinkReport> {
    return await invoke("set_project_agent", { projectId, agentId, enabled });
  },
  /** 按已保存的成员重新写入项目已启用的全部 Agent（应用修改） */
  async reapply(projectId: string): Promise<LinkReport> {
    return await invoke("reapply_project", { projectId });
  },
  /**
   * 保存成员并写入项目已启用的 Agent（由后端按当前部署确定），还没启用时只保存。
   * `expected` 是打开详情时的成员：别处改过就拒绝保存，不覆盖
   */
  async save(
    projectId: string,
    members: ProjectMembers,
    expected: ProjectMembers | null,
  ): Promise<LinkReport> {
    return await invoke("save_project", { projectId, members, expected });
  },
  async reorder(projectIds: string[]): Promise<ProjectBinding[]> {
    return await invoke("reorder_projects", { projectIds });
  },
  async list(): Promise<ProjectBinding[]> {
    return await invoke("list_projects");
  },

  async create(name: string, root: string): Promise<ProjectBinding> {
    return await invoke("create_project", { name, root });
  },

  async update(
    projectId: string,
    patch: {
      name?: string;
      agentIds?: string[];
      skillIds?: string[];
      groupIds?: string[];
      linkMode?: LinkMode;
    },
  ): Promise<ProjectBinding> {
    return await invoke("update_project", {
      projectId,
      name: patch.name ?? null,
      agentIds: patch.agentIds ?? null,
      skillIds: patch.skillIds ?? null,
      groupIds: patch.groupIds ?? null,
      linkMode: patch.linkMode ?? null,
    });
  },

  /** 只删绑定，不动已写进项目目录的文件（可能已提交进 git） */
  async remove(projectId: string): Promise<void> {
    await invoke("delete_project", { projectId });
  },

  async apply(
    projectId: string,
    selection?: Pick<
      ProjectBinding,
      "agentIds" | "skillIds" | "groupIds" | "linkMode"
    >,
  ): Promise<LinkReport> {
    return await invoke("apply_project", {
      projectId,
      selection: selection ?? null,
    });
  },

  async unapply(
    projectId: string,
    skillIds: string[],
    force = false,
  ): Promise<LinkReport> {
    return await invoke("unapply_project", { projectId, skillIds, force });
  },

  async writeGitignore(projectId: string): Promise<boolean> {
    return await invoke("write_project_gitignore", { projectId });
  },

  async pickDirectory(defaultPath?: string): Promise<string | null> {
    return await invoke("pick_directory", {
      defaultPath: defaultPath ?? null,
    });
  },
};
