import { invoke as nativeInvoke } from "@tauri-apps/api/core";

export interface ServerProfile {
  id: string;
  name: string;
  host: string;
  user: string | null;
  port: number | null;
  identityFile: string | null;
  jumpHost: string | null;
  passwordAuth: boolean;
  helperBinary: string | null;
}
export interface Target {
  id: string;
  name: string;
  connected: boolean;
}
let target: Target = { id: "local", name: "本机", connected: true };
let pending = 0;
const listeners = new Set<() => void>();
export const subscribeTransport = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
export const getPending = () => pending;
export const getTarget = () => target;
export function setTarget(next: Target) {
  target = next;
  listeners.forEach((f) => f());
}
function busy(delta: number) {
  pending += delta;
  listeners.forEach((f) => f());
}
let pickRemote: (path?: string) => Promise<string | null> = async () => {
  throw new Error("远程目录选择器尚未准备好");
};
let revealRemote: (path: string) => Promise<void> = async () => {
  throw new Error("远程目录浏览器尚未准备好");
};
export function setRemoteDialogs(
  pick: typeof pickRemote,
  reveal: typeof revealRemote,
) {
  pickRemote = pick;
  revealRemote = reveal;
}

export async function requestRemote<T>(
  serverId: string,
  method: string,
  params: Record<string, unknown> = {},
): Promise<T> {
  try {
    return await nativeInvoke<T>("remote_request", {
      serverId,
      method,
      params,
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (message.startsWith("REMOTE_DISCONNECTED:") && target.id === serverId) {
      setTarget({ ...target, connected: false });
    }
    throw error;
  }
}

/** Snapshot the destination before starting IO; an in-flight request never follows a UI switch. */
export async function invoke<T>(
  method: string,
  params: Record<string, unknown> = {},
): Promise<T> {
  const destination = target;
  const desktop = [
    "get_app_version",
    "get_cli_info",
    "set_window_theme",
    "search_catalog_skills",
  ];
  if (desktop.includes(method)) return nativeInvoke<T>(method, params);
  if (destination.id === "local") {
    busy(1);
    try {
      return await nativeInvoke<T>(method, params);
    } finally {
      busy(-1);
    }
  }
  if (method === "get_init_error") return null as T;
  if (!destination.connected) throw new Error("服务器已断开，请重新连接");
  busy(1);
  try {
    if (method === "pick_directory")
      return (await pickRemote(params.defaultPath as string | undefined)) as T;
    if (method === "reveal_path")
      return (await revealRemote(params.path as string)) as T;
    if (method === "update_settings") {
      const patch = { ...(params.patch as Record<string, unknown>) };
      const globalPatch: Record<string, unknown> = {};
      for (const key of ["theme", "language"])
        if (key in patch) {
          globalPatch[key] = patch[key];
          delete patch[key];
        }
      if (Object.keys(globalPatch).length)
        await nativeInvoke("update_settings", { patch: globalPatch });
      if (Object.keys(patch).length)
        await requestRemote(destination.id, method, { patch });
      return (await settings(destination.id)) as T;
    }
    if (method === "get_settings") return (await settings(destination.id)) as T;
    if (method === "get_config") {
      const config = await requestRemote<Record<string, unknown>>(
        destination.id,
        method,
        params,
      );
      return { ...config, settings: await settings(destination.id) } as T;
    }
    return await requestRemote<T>(destination.id, method, params);
  } finally {
    busy(-1);
  }
}

async function settings(id: string) {
  const [remote, local] = await Promise.all([
    requestRemote<Record<string, unknown>>(id, "get_settings"),
    nativeInvoke<{ theme: string; language: string }>("get_settings"),
  ]);
  return { ...remote, theme: local.theme, language: local.language };
}

/** Bind the upload to the server before opening the desktop picker. */
export async function uploadLocalSkill(
  pick: () => Promise<string | string[] | null>,
): Promise<boolean> {
  const destination = target;
  if (destination.id === "local" || !destination.connected)
    throw new Error("请先连接目标服务器");
  busy(1);
  try {
    const path = await pick();
    if (typeof path !== "string") return false;
    if (target !== destination)
      throw new Error("管理目标已变化，请重新选择上传目录");
    await requestRemote(destination.id, "upload_local_skill", { path });
    return true;
  } finally {
    busy(-1);
  }
}
