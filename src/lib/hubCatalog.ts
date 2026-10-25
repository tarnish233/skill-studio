import { isRegistered } from "@/lib/linkStatus";
import type { SkillView } from "@/types";

export interface HubCard {
  key: string;
  skill: SkillView;
  sources: SkillView[];
  unavailable: boolean;
}

/** Display aggregation only. Source IDs and filesystem ownership stay unchanged. */
export function hubCatalog(skills: SkillView[]): HubCard[] {
  const cards = new Map<string, HubCard>();
  for (const skill of [...skills].sort((a, b) =>
    a.sourcePath.localeCompare(b.sourcePath),
  )) {
    const unavailable = !!skill.diagnostics?.length;
    const key =
      !skill.installation?.variants?.length &&
      !unavailable &&
      !skill.malformedFrontmatter &&
      skill.contentHash
        ? JSON.stringify([skill.name, skill.contentHash])
        : `source:${skill.id}`;
    const card = cards.get(key);
    if (card) card.sources.push(skill);
    else cards.set(key, { key, skill, sources: [skill], unavailable });
  }
  return [...cards.values()].sort((a, b) =>
    a.skill.name.localeCompare(b.skill.name),
  );
}

/** 随应用提供的内置 skill；它没有托管历史，但也不该被当成「来源待确认」 */
export function isBuiltinSource(source: SkillView): boolean {
  return (source.sourceIds ?? source.provenance?.sourceIds ?? []).includes(
    "builtin",
  );
}

export function sourceAgentIds(source: SkillView): string[] {
  const ids = new Set<string>();
  if (source.origin.kind === "inPlace") ids.add(source.origin.ownerAgent);
  for (const [id, state] of Object.entries(source.agents)) {
    // Foreign same-name entries and copies do not prove the origin of this source.
    if (["source", "linked", "brokenLink"].includes(state.status)) ids.add(id);
  }
  return [...ids].sort();
}

/** Hide only Agent-owned sources; Hub and shared roots have independent ownership. */
export function isSkillVisible(
  source: SkillView,
  disabledAgents: readonly string[] = [],
): boolean {
  if (!disabledAgents.length || source.origin.kind === "hub") return true;
  const sourceIds = source.sourceIds ?? source.provenance?.sourceIds ?? [];
  const shared = (path: string) =>
    /(^|\/)\.agents?(\/|$)/.test(path.replace(/\\/g, "/"));
  if (
    sourceIds.includes("agent") ||
    shared(source.sourcePath) ||
    shared(source.root)
  )
    return true;
  const owners = new Set(sourceAgentIds(source));
  for (const id of sourceIds) {
    if (!["studio", "builtin", "agent", "external", "unknown"].includes(id))
      owners.add(id);
  }
  for (const [id, state] of Object.entries(source.agents)) {
    if (isRegistered(state.status)) owners.add(id);
  }
  return !owners.size || [...owners].some((id) => !disabledAgents.includes(id));
}

/** Installation sources per merged card; each source contributes at most once. */
export function hubSourceIds(card: HubCard): string[] {
  const ids = new Set<string>();
  const shared = (path: string) =>
    /(^|\/)\.agents?(\/|$)/.test(path.replace(/\\/g, "/"));
  for (const source of card.sources) {
    if (source.sourceIds) {
      source.sourceIds.forEach((id) => ids.add(id));
      continue;
    }
    if (source.provenance) {
      source.provenance.sourceIds.forEach((id) => ids.add(id));
      continue;
    }
    // Older backends cannot prove the origins of existing Hub content.
    if (source.origin.kind === "hub") {
      ids.add("unknown");
      continue;
    }
    if (shared(source.sourcePath) || shared(source.root)) ids.add("agent");
    if (
      source.origin.kind === "inPlace" &&
      !shared(source.sourcePath) &&
      !shared(source.root)
    ) {
      ids.add(source.origin.ownerAgent);
    }
    for (const [id, state] of Object.entries(source.agents)) {
      if (!["source", "linked"].includes(state.status)) continue;
      const paths = state.entryPaths?.length
        ? state.entryPaths
        : [state.targetPath];
      if (paths.some(shared)) ids.add("agent");
      if (paths.some((path) => !shared(path))) ids.add(id);
    }
  }
  return [...ids];
}
