import type { Group, ProjectBinding, SkillView, TokenEstimate } from "@/types";

/**
 * token 体量估算的前端侧汇总。
 *
 * 每个 skill 的估算值由后端在扫描时算好（`services::tokens`），这里只负责按分组 /
 * 项目 / agent 把它们加起来 —— 口径集中在一处，前后端不会算出两个数。
 *
 * 两半分开保留：`SKILL.md` 被调用时必然进上下文，`references/` 之类是按需打开的，
 * 合计给个总数，tooltip 里再拆开说明。
 */

export interface TokenSum extends TokenEstimate {
  total: number;
}

const EMPTY: TokenEstimate = { skillMd: 0, extras: 0 };

/** skill id → 估算值。查表比每次 `skills.find` 少一层嵌套遍历。 */
export function tokenIndex(skills: SkillView[]): Map<string, TokenEstimate> {
  return new Map(skills.map((s) => [s.id, s.tokens ?? EMPTY]));
}

/** 汇总一批 skill id；重复 id 只算一次，真身已删的按 0 计。 */
export function sumTokens(
  index: Map<string, TokenEstimate>,
  ids: Iterable<string>,
): TokenSum {
  let skillMd = 0;
  let extras = 0;
  for (const id of new Set(ids)) {
    const est = index.get(id);
    if (!est) continue;
    skillMd += est.skillMd;
    extras += est.extras;
  }
  return { skillMd, extras, total: skillMd + extras };
}

/**
 * 一个项目实际会写进去的 skill，口径必须与后端 `Studio::write_project` 一致：
 * 分组不归属任何单个 agent，后端对 project.agentIds 里每个 agent 都取同一份
 * “直接绑定的 skillIds + 所有已选分组成员”并集，不按 agent 过滤。
 * agentIds 为空时后端什么都不写，这里也就是 0。
 * 同一个 skill 既单独绑定又在组里，只占一份文件，也只该算一次 token。
 */
export function projectSkillIds(
  project: ProjectBinding,
  groups: Group[],
): string[] {
  if (project.agentIds.length === 0) return [];
  const ids = new Set<string>(project.skillIds);
  for (const groupId of project.groupIds) {
    const group = groups.find((g) => g.id === groupId);
    if (!group) continue;
    for (const id of group.skillIds) ids.add(id);
  }
  return [...ids];
}

/** 徽标用的短格式：820 / 1.2k / 24k / 1.5M */
export function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  const k = n / 1000;
  // 999.5k 起会四舍五入成"1000k"——三位数的 k 该换单位了
  return k < 999.5 ? scaled(k, "k") : scaled(n / 1_000_000, "M");
}

/** 小于 10 保留一位小数（1.2k），再大就取整（24k）——徽标只有一行的宽度 */
function scaled(v: number, unit: string): string {
  return v < 10
    ? `${v.toFixed(1).replace(/\.0$/, "")}${unit}`
    : `${Math.round(v)}${unit}`;
}

/**
 * hover 说明：这个数字是怎么来的，以及两半各占多少。
 *
 * `driftedCount` 是参与合计的 skill 里"副本与源已不一致"的个数（见
 * `linkStatus.isCopyDrifted`）。这个数字恒定按源估算，而 agent 读的是副本，
 * 所以有漂移时必须把误差方向说出来，光给一个精确的数字反而更误导。
 */
export function tokenTitle(
  sum: TokenSum,
  scope: string,
  driftedCount = 0,
): string {
  const base =
    `${scope}的 token 估算：SKILL.md ≈ ${formatTokens(sum.skillMd)}，` +
    `references / scripts 等文本附带文件 ≈ ${formatTokens(sum.extras)}。` +
    `二进制与隐藏文件、目录软链指向的内容都不计。估算值，非精确分词。`;
  if (driftedCount <= 0) return base;
  return (
    `${base}\n其中 ${driftedCount} 个 skill 的副本与源已不一致：这个数字按源算，` +
    `agent 读的是副本，可能偏高也可能偏低 —— 在分组卡片上点「应用修改」即可对上。`
  );
}
