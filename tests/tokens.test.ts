import { describe, expect, it } from "vitest";
import {
  formatTokens,
  projectSkillIds,
  sumTokens,
  tokenIndex,
  tokenTitle,
} from "@/lib/tokens";
import { makeGroup, makeProject, makeSkill } from "./mocks/tauri";

describe("token 体量汇总", () => {
  const skills = [
    makeSkill({ id: "a", tokens: { skillMd: 400, extras: 100 } }),
    makeSkill({ id: "b", tokens: { skillMd: 1200, extras: 0 } }),
  ];
  const index = tokenIndex(skills);

  it("SKILL.md 与附带文件分别累加，总数是两者之和", () => {
    expect(sumTokens(index, ["a", "b"])).toEqual({
      skillMd: 1600,
      extras: 100,
      total: 1700,
    });
  });

  it("重复 id 只算一次 —— 同一个 skill 既单独绑定又在组里，只占一份文件", () => {
    expect(sumTokens(index, ["a", "b", "a"]).total).toBe(1700);
  });

  it("真身已删的 id 按 0 计，不让整个数字变成 NaN", () => {
    expect(sumTokens(index, ["a", "已缺失"]).total).toBe(500);
  });

  it("项目的口径是「直接绑定 + 分组带进来」的并集", () => {
    const groups = [
      makeGroup({ id: "g1", skillIds: ["a", "b"] }),
      makeGroup({ id: "g2", skillIds: ["b"] }),
    ];
    const project = makeProject({
      agentIds: ["claude-code"],
      skillIds: ["a"],
      groupIds: ["g1", "g2"],
    });
    expect(projectSkillIds(project, groups).sort()).toEqual(["a", "b"]);
    expect(sumTokens(index, projectSkillIds(project, groups)).total).toBe(1700);
  });

  it("绑定了已删除的分组时按空处理", () => {
    const project = makeProject({
      agentIds: ["claude-code"],
      skillIds: ["a"],
      groupIds: ["没了"],
    });
    expect(projectSkillIds(project, [])).toEqual(["a"]);
  });

  it("分组不区分当年用哪个 Agent 创建，绑定后其成员全部并进来", () => {
    const groups = [makeGroup({ id: "g1", skillIds: ["b"] })];
    const project = makeProject({
      agentIds: ["claude-code"],
      skillIds: ["a"],
      groupIds: ["g1"],
    });
    expect(projectSkillIds(project, groups).sort()).toEqual(["a", "b"]);
  });

  it("绑定多个分组求并集，重叠成员只算一次", () => {
    const groups = [
      makeGroup({ id: "g1", skillIds: ["a"] }),
      makeGroup({ id: "g2", skillIds: ["a", "b"] }),
    ];
    const project = makeProject({
      agentIds: ["claude-code", "codex"],
      skillIds: [],
      groupIds: ["g1", "g2"],
    });
    expect(projectSkillIds(project, groups).sort()).toEqual(["a", "b"]);
    expect(sumTokens(index, projectSkillIds(project, groups)).total).toBe(1700);
  });

  it("还没有 Agent 启用的项目也按成员计算，与分组卡片一致", () => {
    const groups = [makeGroup({ id: "g1", skillIds: ["b"] })];
    const project = makeProject({
      agentIds: [],
      skillIds: ["a"],
      groupIds: ["g1"],
    });
    expect(projectSkillIds(project, groups)).toEqual(["a", "b"]);
  });

  it.each([
    [0, "0"],
    [820, "820"],
    [1000, "1k"],
    [1240, "1.2k"],
    [9950, "9.9k"],
    // 9.99 → toFixed(1) 是 "10.0"，尾巴上的 .0 要被剪掉，不能显示 "10.0k"
    [9990, "10k"],
    [24400, "24k"],
    // k 到了三位数就该换单位，否则 999999 与 1000000 都写成 "1000k"
    [999499, "999k"],
    [999999, "1M"],
    [1_000_000, "1M"],
    [1_500_000, "1.5M"],
    [12_500_000, "13M"],
  ])("徽标格式：%i → %s", (n, expected) => {
    expect(formatTokens(n)).toBe(expected);
  });
});

describe("token tooltip", () => {
  const sum = { skillMd: 400, extras: 100, total: 500 };

  it("没有副本漂移时不额外声明偏差", () => {
    const title = tokenTitle(sum, "当前已启用的 skill");
    expect(title).toContain("当前已启用的 skill");
    expect(title).toContain("SKILL.md ≈ 400");
    expect(title).not.toContain("副本与源已不一致");
  });

  it("有副本漂移时说明这个数字按源估算、可能偏高或偏低", () => {
    const title = tokenTitle(sum, "当前已启用的 skill", 2);
    // 两半的拆分还在，只是后面追加了不确定性说明
    expect(title).toContain("SKILL.md ≈ 400");
    expect(title).toContain("2 个 skill 的副本与源已不一致");
    expect(title).toContain("应用修改");
  });
});
