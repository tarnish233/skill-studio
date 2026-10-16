import type { Settings } from "@/types";

export type ViewId = "hub" | "groups" | "projects" | "settings" | "install";
export type Resource = "skills" | "mcp";
export const VIEW_STORAGE_KEY = "skill-studio-view";
export const RESOURCE_STORAGE_KEY = "skill-studio-resource";

/** Resolve persisted navigation only against the loaded settings. */
export function readInitialView(
  settings?: Settings,
  targetId = "local",
): ViewId {
  const stored = localStorage.getItem(VIEW_STORAGE_KEY);
  const view: ViewId =
    stored === "projects" || stored === "groups" ? stored : "hub";
  // Remote MCP has no group or project management; open its Hub instead.
  if (
    targetId !== "local" &&
    view !== "hub" &&
    initialResource(settings) === "mcp"
  )
    return "hub";
  return view;
}

export function initialResource(settings?: Settings): Resource {
  if (settings?.manageMcp === false) return "skills";
  return localStorage.getItem(RESOURCE_STORAGE_KEY) === "mcp"
    ? "mcp"
    : "skills";
}
