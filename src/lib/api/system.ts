import { invoke } from "@/lib/api/transport";

export interface CliInfo {
  /** App 自带的命令行程序；这个版本没有附带时为 null */
  bundled: string | null;
  /** 终端里可以直接运行的命令名：skill-studio（Homebrew 链接）或 skill-studio-cli */
  command: string | null;
  /** 命令所在的位置 */
  location: string | null;
  /** 这个路径能不能长期使用（AppImage 运行时在临时挂载点里） */
  stable: boolean;
}

export const systemApi = {
  /** 启动期错误；非空时前端渲染恢复提示而不是主界面 */
  async getInitError(): Promise<string | null> {
    return await invoke("get_init_error");
  },

  async getVersion(): Promise<string> {
    return await invoke("get_app_version");
  },

  /** App 自带的命令行程序在哪、终端里能不能直接运行（只看本机） */
  async getCliInfo(): Promise<CliInfo> {
    return await invoke("get_cli_info");
  },

  async setWindowTheme(theme: string): Promise<void> {
    await invoke("set_window_theme", { theme });
  },

  /** 用系统默认程序打开路径（Finder / 资源管理器） */
  async revealPath(path: string): Promise<void> {
    await invoke("reveal_path", { path });
  },
};
