import type { Settings } from "@/types";
import { supportsMcp } from "@/lib/mcpAgents";

export type StaticView = "hub" | "projects" | "settings" | "install";
export type ViewId = StaticView | `agent:${string}`;
export type Resource = "skills" | "mcp";
export const VIEW_STORAGE_KEY = "skill-studio-view";
export const RESOURCE_STORAGE_KEY = "skill-studio-resource";

/** Resolve persisted navigation only against the loaded settings. */
export function readInitialView(
  settings?: Settings,
  targetId = "local",
): ViewId {
  const stored = localStorage.getItem(VIEW_STORAGE_KEY);
  // The old two-Hub navigation persisted "library" or "mcp" as a view.
  const view: ViewId =
    stored === "projects" || stored?.startsWith("agent:")
      ? (stored as ViewId)
      : "hub";
  const mcp = initialResource(view, settings) === "mcp";
  if (view.startsWith("agent:")) {
    const agent = view.slice("agent:".length);
    if (
      settings?.disabledAgents?.includes(agent) ||
      (mcp && !supportsMcp(agent))
    )
      return "hub";
  }
  if (
    targetId !== "local" &&
    mcp &&
    (view === "projects" || view.startsWith("agent:"))
  )
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
