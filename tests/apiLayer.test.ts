import { describe, expect, it } from "vitest";
import { groupsApi, projectsApi, settingsApi, skillsApi } from "@/lib/api";
import { calls } from "./mocks/tauri";

/**
 * API 层契约：断言每个方法真正传给 invoke 的命令名与参数形状。
 *
 * 页面里那些藏在 Radix 菜单后面的动作没法在 jsdom 里稳定点开（见
 * tests/libraryPage.test.tsx 的说明），所以参数正确性在这一层验。
 * 命令名与后端签名是否对得上，由 tests/ipcContract.test.ts 静态比对。
 */
function lastCall() {
  return calls[calls.length - 1];
}

describe("skillsApi", () => {
  it("register 透传 skillIds / agentIds，未指定 mode 时传 null 让后端用默认值", async () => {
    await skillsApi.register(["s1", "s2"], ["codex"]);
    expect(lastCall()).toEqual({
      command: "register_skills",
      args: {
        skillIds: ["s1", "s2"],
        agentIds: ["codex"],
        mode: null,
        force: false,
      },
    });
  });

  it("register 显式指定 mode 与 force 时如实传递", async () => {
    await skillsApi.register(["s1"], ["claude-code"], "copy", true);
    expect(lastCall().args).toMatchObject({ mode: "copy", force: true });
  });

  it("unregister 默认不 force —— 用户手工放的内容不该被静默删掉", async () => {
    await skillsApi.unregister(["s1"], ["codex"]);
    expect(lastCall()).toEqual({
      command: "unregister_skills",
      args: { skillIds: ["s1"], agentIds: ["codex"], force: false },
    });
  });

  it("setEnabled 传 skillId，由后端解析实际入口和原生配置选择器", async () => {
    await skillsApi.setEnabled("deploy", "claude-code", false);
    expect(lastCall()).toEqual({
      command: "set_skill_enabled",
      args: { skillId: "deploy", agentId: "claude-code", enabled: false },
    });
  });

  it("adoptToHub / prune 命令名正确", async () => {
    await skillsApi.adoptToHub("s1");
    expect(lastCall()).toEqual({
      command: "adopt_to_hub",
      args: { skillId: "s1" },
    });
    await skillsApi.prune();
    expect(lastCall().command).toBe("prune_missing");
  });
});

describe("groupsApi", () => {
  it("save 新建时 groupId 传 null，成员按给定顺序提交", async () => {
    await groupsApi.save(null, "前端", ["b", "a"]);
    expect(lastCall()).toEqual({
      command: "save_group",
      args: { groupId: null, name: "前端", skillIds: ["b", "a"] },
    });
  });

  it("activate 传 null 表示为该 Agent 停用当前分组", async () => {
    await groupsApi.activate("codex", null);
    expect(lastCall()).toEqual({
      command: "activate_agent_group",
      args: { agentId: "codex", groupId: null },
    });
  });
});

describe("projectsApi", () => {
  it("update 只传要改的字段，其余为 null", async () => {
    await projectsApi.update("p1", { agentIds: ["codex"] });
    expect(lastCall()).toEqual({
      command: "update_project",
      args: {
        projectId: "p1",
        name: null,
        agentIds: ["codex"],
        skillIds: null,
        groupIds: null,
        linkMode: null,
      },
    });
  });

  it("pickDirectory 的 defaultPath 缺省传 null", async () => {
    await projectsApi.pickDirectory();
    expect(lastCall()).toEqual({
      command: "pick_directory",
      args: { defaultPath: null },
    });
  });
});

describe("settingsApi", () => {
  it("update 把补丁包在 patch 字段里（后端签名是单个结构体）", async () => {
    await settingsApi.update({ defaultLinkMode: "symlink" });
    expect(lastCall()).toEqual({
      command: "update_settings",
      args: { patch: { defaultLinkMode: "symlink" } },
    });
  });

  it("restoreBackup 传路径", async () => {
    await settingsApi.restoreBackup("/tmp/config-1.json");
    expect(lastCall()).toEqual({
      command: "restore_backup",
      args: { path: "/tmp/config-1.json" },
    });
  });
});
