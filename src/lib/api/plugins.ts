import { invoke } from "./transport";

export interface PluginComponent {
  kind: string;
  name: string;
  path: string | null;
  issue: string | null;
}
export interface PluginPackage {
  name: string;
  version: string | null;
  description: string | null;
  format: string;
  root: string;
  components: PluginComponent[];
  extensions: string[];
  warnings: string[];
  error: string | null;
}
export type PluginAction =
  "install" | "enable" | "disable" | "update" | "uninstall" | "forget";
export interface PluginView {
  id: string;
  agentId: string;
  nativeId: string;
  source: string;
  scope: string;
  profile: string | null;
  configPath: string;
  available: boolean;
  configured: boolean;
  enabled: boolean | null;
  revision: string;
  actions: PluginAction[];
  readOnlyReason: string | null;
  package: PluginPackage;
}
export interface PluginInventory {
  plugins: PluginView[];
  warnings: string[];
  pending: boolean;
}
export interface PluginCapability {
  agentId: string;
  cliVersion: string | null;
  install: boolean;
  reason: string | null;
}
export interface PluginOperation {
  action: PluginAction;
  agentId: string;
  pluginId?: string;
  expectedRevision?: string;
  source?: string;
  marketplace?: string;
  confirmed: boolean;
}
export interface PluginOperationResult {
  changed: boolean;
  pending: boolean;
  message: string;
  warnings: string[];
  backupPath: string | null;
}
export const pluginsApi = {
  scan: () => invoke<PluginInventory>("scan_plugins"),
  inspect: (path: string, agentId: string | null = null) =>
    invoke<PluginPackage>("inspect_plugin", { path, agentId }),
  capabilities: () => invoke<PluginCapability[]>("probe_plugin_cli"),
  recover: (confirmed: boolean) =>
    invoke<PluginOperationResult>("recover_plugin_write", { confirmed }),
  operate: (input: PluginOperation) =>
    invoke<PluginOperationResult>("plugin_operation", { input }),
};
