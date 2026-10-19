import { invoke } from "@/lib/api/transport";
import type { AppConfig, Settings, SettingsPatch } from "@/types";

export const settingsApi = {
  async getConfig(): Promise<AppConfig> {
    return await invoke("get_config");
  },

  async get(): Promise<Settings> {
    return await invoke("get_settings");
  },

  async update(patch: SettingsPatch): Promise<Settings> {
    return await invoke("update_settings", { patch });
  },

  async listBackups(): Promise<string[]> {
    return await invoke("list_backups");
  },

  async restoreBackup(path: string): Promise<AppConfig> {
    return await invoke("restore_backup", { path });
  },

  async deleteBackup(path: string): Promise<void> {
    await invoke("delete_backup", { path });
  },

  async getConfigDir(): Promise<string> {
    return await invoke("get_config_dir");
  },
};
