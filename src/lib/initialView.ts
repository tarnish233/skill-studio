import type { Settings } from "@/types";

export type StaticView = "hub" | "groups" | "projects" | "settings" | "install";
export type ViewId = StaticView;
export type Resource = "skills" | "mcp";
export const VIEW_STORAGE_KEY = "skill-studio-view";
export const RESOURCE_STORAGE_KEY = "skill-studio-resource";

/** Resolve persisted navigation only against the loaded settings. */
export function readInitialView(
  settings?: Settings,
  targetId = "local",
): ViewId {
  const stored = localStorage.getItem(VIEW_STORAGE_KEY);
  // The old per-Agent navigation persisted "agent:<id>"; Agent-specific controls
  // now live inside the shared "groups" view, so any such value lands there.
  const view: ViewId =
    stored === "projects"
      ? "projects"
      : stored === "groups" || stored?.startsWith("agent:")
        ? "groups"
        : "hub";
  const mcp = initialResource(view, settings) === "mcp";
  if (targetId !== "local" && mcp && (view === "projects" || view === "groups"))
    return "hub";
  return view;
}

export function initialResource(view: ViewId, settings?: Settings): Resource {
  if (settings?.manageMcp === false) return "skills";
  // Honor the legacy Hub even if the old resource preference disagrees.
  if (view === "hub") {
    const storedView = localStorage.getItem(VIEW_STORAGE_KEY);
    if (storedView === "mcp") return "mcp";
    if (storedView === "library") return "skills";
  }
  return localStorage.getItem(RESOURCE_STORAGE_KEY) === "mcp"
    ? "mcp"
    : "skills";
}
