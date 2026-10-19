import { invoke } from "@/lib/api/transport";
import type {
  CatalogSkill,
  CatalogInstallResult,
  HubBackup,
  LocalSkill,
  LinkMode,
  LinkReport,
  Skill,
  SkillView,
} from "@/types";

export const skillsApi = {
  async discoverLocal(path: string): Promise<LocalSkill[]> {
    return await invoke("discover_local_skills", { path });
  },
  async importLocal(path: string): Promise<Skill> {
    return await invoke("import_local_skill", { path });
  },
  async searchCatalog(query: string): Promise<CatalogSkill[]> {
    return await invoke("search_catalog_skills", { query });
  },
  async installCatalog(
    source: string,
    skillId: string,
    repositoryPath?: string,
    repositoryPaths?: string[],
  ): Promise<CatalogInstallResult> {
    return await invoke("install_catalog_skill", {
      source,
      skillId,
      repositoryPath: repositoryPath ?? null,
      repositoryPaths: repositoryPaths ?? null,
    });
  },
  async scan(): Promise<SkillView[]> {
    return await invoke("scan_skills");
  },

  async register(
    skillIds: string[],
    agentIds: string[],
    mode?: LinkMode,
    force = false,
  ): Promise<LinkReport> {
    return await invoke("register_skills", {
      skillIds,
      agentIds,
      mode: mode ?? null,
      force,
    });
  },

  async unregister(
    skillIds: string[],
    agentIds: string[],
    force = false,
  ): Promise<LinkReport> {
    return await invoke("unregister_skills", { skillIds, agentIds, force });
  },

  /** 走 agent 原生配置启停，不动文件 */
  async setEnabled(
    skillId: string,
    agentId: string,
    enabled: boolean,
  ): Promise<void> {
    await invoke("set_skill_enabled", { skillId, agentId, enabled });
  },

  async adoptToHub(skillId: string): Promise<Skill> {
    return await invoke("adopt_to_hub", { skillId });
  },

  async releaseFromHub(skillId: string): Promise<Skill> {
    return await invoke("release_from_hub", { skillId });
  },

  /** 用系统默认程序打开预览的 SKILL.md（目录由后端拼接，不在这里算路径） */
  async openDocument(path: string): Promise<void> {
    await invoke("open_skill_document", { path });
  },

  async prune(): Promise<number> {
    return await invoke("prune_missing");
  },

  async listHubBackups(): Promise<HubBackup[]> {
    return await invoke("list_hub_backups");
  },

  async deleteHubBackups(ids: string[]): Promise<number> {
    return await invoke("delete_hub_backups", { ids });
  },
};
