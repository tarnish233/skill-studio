import type { Settings } from "@/types";

export type ViewId = "hub" | "groups" | "projects" | "settings" | "install";
export type Resource = "skills" | "mcp" | "plugins";
export const VIEW_STORAGE_KEY = "skill-studio-view";
export const RESOURCE_STORAGE_KEY = "skill-studio-resource";

/** 记下的导航：本机和服务器都有 Hub、分组、项目，不认识的值回到 Hub */
export function readInitialView(): ViewId {
  const stored = localStorage.getItem(VIEW_STORAGE_KEY);
  return stored === "projects" || stored === "groups" ? stored : "hub";
}

export function initialResource(settings?: Settings): Resource {
  const stored = localStorage.getItem(RESOURCE_STORAGE_KEY);
  if (stored === "plugins") return "plugins";
  return stored === "mcp" && settings?.manageMcp !== false ? "mcp" : "skills";
}
