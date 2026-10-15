import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { GroupsPage } from "@/pages/GroupsPage";
import {
  agentState,
  handlers,
  calls,
  makeGroup,
  makeSkill,
  claudeAgent,
  codexAgent,
} from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

function setup() {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("scan_skills", () => [
    makeSkill({ id: "a", name: "Alpha" }),
    makeSkill({ id: "b", name: "Beta" }),
  ]);
  handlers.set("get_config", () => ({ activeGroups: {} }));
}
/** 切到「已安装 / 未托管」标签，再把 Agent 选择器指到 codex —— 两个标签都是单 Agent 视角。 */
async function selectCodexInstalledTab(tabName: RegExp) {
  fireEvent.mouseDown(await screen.findByRole("tab", { name: tabName }), {
    button: 0,
    ctrlKey: false,
  });
  fireEvent.change(screen.getByLabelText("选择 Agent"), {
    target: { value: "codex" },
  });
}

it("creates a shared draft and saves the full rapid selection without activating", async () => {
  setup();
  handlers.set("save_group", (args) => makeGroup(args));
  renderWithProviders(<GroupsPage />);
  fireEvent.click(screen.getByRole("button", { name: "新建分组" }));
  fireEvent.change(screen.getByLabelText("分组名称"), {
    target: { value: "Dev" },
  });
  fireEvent.click(await screen.findByRole("checkbox", { name: "选择 Alpha" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "选择 Beta" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() =>
    expect(calls.find((c) => c.command === "save_group")?.args).toEqual({
      groupId: null,
      name: "Dev",
      skillIds: ["a", "b"],
    }),
  );
  expect(calls.some((c) => c.command === "activate_agent_group")).toBe(false);
});
it("分组卡片给出组内 token 体量，空组不显示这个徽标", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("scan_skills", () => [
    makeSkill({ id: "a", name: "Alpha", tokens: { skillMd: 400, extras: 0 } }),
    makeSkill({ id: "b", name: "Beta", tokens: { skillMd: 1200, extras: 0 } }),
  ]);
  handlers.set("get_config", () => ({ activeGroups: {} }));
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a", "b"] }),
    makeGroup({ id: "e", name: "Empty", skillIds: [] }),
  ]);
  renderWithProviders(<GroupsPage />);
  expect(await screen.findByText("≈ 1.6k tokens")).toBeInTheDocument();
  // 分组页顶部不再按单一 Agent 统计 token；一个 skill 都没有的空组也不该顶一个徽标
  expect(screen.getAllByText(/tokens$/)).toHaveLength(1);
});

it("顶部统计卡片只数真正生效的东西，且不受分组卡片的组内合计影响", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("scan_skills", () => [
    // 真身就在 codex 目录里、没被停用 → 计入
    makeSkill({
      id: "a",
      name: "Alpha",
      tokens: { skillMd: 400, extras: 100 },
      agents: { codex: agentState("source") },
    }),
    // 被 agent 的原生开关停用 → 文件在，但用不上，不计
    makeSkill({
      id: "b",
      name: "Beta",
      tokens: { skillMd: 1200, extras: 0 },
      agents: { codex: agentState("copied", true) },
    }),
    // 同名目录是用户自己放的，本工具没管 → 不计
    makeSkill({
      id: "c",
      name: "Gamma",
      tokens: { skillMd: 900, extras: 0 },
      agents: { codex: agentState("foreign") },
    }),
  ]);
  handlers.set("list_groups", () => [
    makeGroup({ id: "g", name: "Dev", skillIds: ["a", "b"] }),
  ]);
  handlers.set("get_config", () => ({
    activeGroups: {
      codex: { groupId: "g", skillIds: ["a", "b"], entries: [] },
    },
  }));
  renderWithProviders(<GroupsPage />);
  await selectCodexInstalledTab(/已安装 skill/);
  expect(await screen.findByText("已启用 1 个 skill")).toBeInTheDocument();
  // 只有 Alpha 生效：400 + 100 附带文件
  expect(screen.getByText("≈ 500 tokens")).toBeInTheDocument();
  // 分组卡片仍然按组内全部成员算，两个数字不该互相污染
  fireEvent.mouseDown(screen.getByRole("tab", { name: /^分组/ }), {
    button: 0,
    ctrlKey: false,
  });
  expect(screen.getByText("≈ 1.7k tokens")).toBeInTheDocument();
});

/**
 * 后端的估算恒定来自真身，而 copy 漂移的三态里 agent 读到的是另一份内容。
 * 不能把它们从计数里剔掉（agent 确实加载了），只能把不确定性说清楚。
 */
it("副本与源漂移时，token 胶囊在 tooltip 里交代这个数字可能偏高或偏低", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("scan_skills", () => [
    makeSkill({
      id: "a",
      name: "Alpha",
      tokens: { skillMd: 400, extras: 0 },
      agents: { codex: agentState("source") },
    }),
    // 源从 5000 改成 200 却没重新应用：codex 读的还是旧副本
    makeSkill({
      id: "b",
      name: "Beta",
      tokens: { skillMd: 200, extras: 0 },
      agents: { codex: agentState("copyStale") },
    }),
  ]);
  handlers.set("list_groups", () => []);
  handlers.set("get_config", () => ({ activeGroups: {} }));
  renderWithProviders(<GroupsPage />);
  await selectCodexInstalledTab(/已安装 skill/);
  const pill = await screen.findByText("≈ 600 tokens");
  // 计数不变：两个 skill 都算进去了
  expect(screen.getByText("已启用 2 个 skill")).toBeInTheDocument();
  expect(pill.getAttribute("title")).toContain("1 个 skill 的副本与源已不一致");
  expect(pill.getAttribute("title")).toContain("应用修改");
});

it("没有漂移时 tooltip 不说这句话，免得每次都像在报警", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("scan_skills", () => [
    makeSkill({
      id: "a",
      name: "Alpha",
      tokens: { skillMd: 400, extras: 0 },
      agents: { codex: agentState("source") },
    }),
    makeSkill({
      id: "b",
      name: "Beta",
      tokens: { skillMd: 200, extras: 0 },
      agents: { codex: agentState("copied") },
    }),
  ]);
  handlers.set("list_groups", () => []);
  handlers.set("get_config", () => ({ activeGroups: {} }));
  renderWithProviders(<GroupsPage />);
  await selectCodexInstalledTab(/已安装 skill/);
  const pill = await screen.findByText("≈ 600 tokens");
  expect(pill.getAttribute("title")).not.toContain("副本与源已不一致");
});

it("copyConflict 既计入已启用，又让分组卡片标出「需要检查」", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("scan_skills", () => [
    makeSkill({
      id: "a",
      name: "Alpha",
      tokens: { skillMd: 400, extras: 0 },
      agents: { codex: agentState("source") },
    }),
    makeSkill({
      id: "b",
      name: "Beta",
      tokens: { skillMd: 200, extras: 0 },
      agents: { codex: agentState("copyConflict") },
    }),
  ]);
  handlers.set("list_groups", () => [
    makeGroup({ id: "g", name: "Dev", skillIds: ["a", "b"] }),
  ]);
  handlers.set("get_config", () => ({
    activeGroups: {
      codex: { groupId: "g", skillIds: ["a", "b"], entries: [] },
    },
  }));
  renderWithProviders(<GroupsPage />);
  expect(await screen.findByText("需要检查")).toBeInTheDocument();
  await selectCodexInstalledTab(/已安装 skill/);
  expect(screen.getByText("已启用 2 个 skill")).toBeInTheDocument();
});

it("统计胶囊用主题边框，深色模式下不是硬编码的近白色", async () => {
  setup();
  handlers.set("list_groups", () => []);
  renderWithProviders(<GroupsPage />);
  await selectCodexInstalledTab(/已安装 skill/);
  for (const label of ["已启用 0 个 skill", /^≈ .* tokens$/]) {
    const pill = await screen.findByText(label);
    expect(pill).toHaveClass("border-border-default");
    // 这一行没有筛选语义，胶囊不该变成可点的按钮
    expect(pill.tagName).not.toBe("BUTTON");
  }
});

it("shows every shared group at once, and each Agent's icon only activates that Agent", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Codex group", skillIds: ["a"] }),
    makeGroup({ id: "x", name: "Claude group", skillIds: ["b"] }),
  ]);
  renderWithProviders(<GroupsPage />);
  await screen.findByText("Codex group");
  // 分组不再归属某个 Agent：两个组同时可见，不会互相遮挡
  expect(screen.getByText("Claude group")).toBeInTheDocument();
  // 两张卡片都有自己的“为 Codex 启用”图标，必须在对应卡片内找
  const codexCard = screen
    .getByText("Codex group")
    .closest('[class*="rounded-xl"]') as HTMLElement;
  fireEvent.click(
    within(codexCard).getByRole("button", { name: "为 Codex 启用" }),
  );
  await waitFor(() =>
    expect(
      calls.find((c) => c.command === "activate_agent_group")?.args,
    ).toEqual({ agentId: "codex", groupId: "c" }),
  );
});
it("cancel discards unsaved selection and failed saves preserve the editable draft", async () => {
  setup();
  handlers.set("save_group", () => {
    throw new Error("disk full");
  });
  renderWithProviders(<GroupsPage />);
  fireEvent.click(screen.getByRole("button", { name: "新建分组" }));
  fireEvent.change(screen.getByLabelText("分组名称"), {
    target: { value: "Dev" },
  });
  fireEvent.click(await screen.findByRole("checkbox", { name: "选择 Alpha" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("disk full");
  expect(screen.getByLabelText("分组名称")).toHaveValue("Dev");
  expect(screen.getByRole("checkbox", { name: "选择 Alpha" })).toBeChecked();
  fireEvent.click(screen.getByRole("button", { name: "取消" }));
  fireEvent.click(screen.getByRole("button", { name: "新建分组" }));
  expect(screen.getByLabelText("分组名称")).toHaveValue("");
  expect(
    screen.getByRole("checkbox", { name: "选择 Alpha" }),
  ).not.toBeChecked();
});
it("editing an active combination shows pending changes, and re-clicking the Agent icon re-applies instead of toggling off", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a", "b"] }),
  ]);
  handlers.set("get_config", () => ({
    activeGroups: { codex: { groupId: "c", skillIds: ["a"], entries: [] } },
  }));
  renderWithProviders(<GroupsPage />);
  expect(await screen.findByText("有待应用修改")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "删除 Dev" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "为 Codex 重新应用" }));
  await waitFor(() =>
    expect(
      calls.find((c) => c.command === "activate_agent_group")?.args,
    ).toEqual({ agentId: "codex", groupId: "c" }),
  );
});

it.each([false, true])(
  "reorders the shared list via the drag handle and rolls back on failure=%s",
  async (fail) => {
    setup();
    let saved = [
      makeGroup({ id: "a", name: "First", sortOrder: 0 }),
      makeGroup({ id: "x", name: "Other", sortOrder: 1 }),
      makeGroup({ id: "b", name: "Second", sortOrder: 2 }),
    ];
    handlers.set("list_groups", () => saved);
    handlers.set("reorder_groups", (args) => {
      if (fail) throw new Error("disk full");
      saved = (args.groupIds as string[]).map((id, sortOrder) => ({
        ...saved.find((g) => g.id === id)!,
        sortOrder,
      }));
      return saved;
    });
    renderWithProviders(<GroupsPage />);
    const handle = await screen.findByRole("button", {
      name: "拖拽排序 First",
    });
    expect(screen.queryByRole("textbox", { name: "搜索分组" })).toBeNull();
    const handles = screen.getAllByRole("button", { name: /拖拽排序/ });
    // jsdom has no layout: give the actual sortable nodes realistic card bounds.
    const nodes = handles.map((h) => h.parentElement!.parentElement!);
    const rect = vi
      .spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockImplementation(function (this: HTMLElement) {
        const index = nodes.indexOf(this);
        const top = Math.max(0, index) * 100;
        return {
          x: 0,
          y: top,
          top,
          bottom: top + 80,
          left: 0,
          right: 800,
          width: 800,
          height: 80,
          toJSON: () => ({}),
        };
      });
    try {
      handle.focus();
      fireEvent.keyDown(handle, { code: "Space" });
      await waitFor(() =>
        expect(handle).toHaveAttribute("aria-pressed", "true"),
      );
      fireEvent.keyDown(document, { code: "ArrowDown" });
      fireEvent.keyDown(document, { code: "Space" });
      await waitFor(() =>
        expect(calls.find((c) => c.command === "reorder_groups")?.args).toEqual(
          { groupIds: ["x", "a", "b"] },
        ),
      );
      await waitFor(() =>
        expect(
          screen.getAllByRole("button", { name: /拖拽排序/ })[0],
        ).toHaveAccessibleName(fail ? "拖拽排序 First" : "拖拽排序 Other"),
      );
      expect(calls.some((c) => c.command === "activate_agent_group")).toBe(
        false,
      );
    } finally {
      rect.mockRestore();
    }
  },
);

it("uses the same Agent icon to activate and stop a group after status refresh", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a"] }),
  ]);
  let active = false;
  handlers.set("get_config", () => ({
    activeGroups: active
      ? { codex: { groupId: "c", skillIds: ["a"], entries: [] } }
      : {},
  }));
  handlers.set("activate_agent_group", (args) => {
    active = args.groupId !== null;
  });
  renderWithProviders(<GroupsPage />);
  fireEvent.click(await screen.findByRole("button", { name: "为 Codex 启用" }));
  const stop = await screen.findByRole("button", { name: "为 Codex 停用" });
  await waitFor(() => expect(stop).not.toBeDisabled());
  expect(screen.queryByRole("button", { name: "为 Codex 启用" })).toBeNull();
  fireEvent.click(stop);
  await screen.findByRole("button", { name: "为 Codex 启用" });
  expect(
    calls
      .filter((c) => c.command === "activate_agent_group")
      .map((c) => c.args),
  ).toEqual([
    { agentId: "codex", groupId: "c" },
    { agentId: "codex", groupId: null },
  ]);
});

it("installed tab uses the top add action and omits redundant agent metadata", async () => {
  setup();
  handlers.set("scan_skills", () => [
    makeSkill({
      id: "available",
      name: "Available",
      agents: { codex: agentState("notLinked") },
    }),
  ]);
  handlers.set("register_skills", () => ({ success: [], failed: [] }));
  renderWithProviders(<GroupsPage />);
  await selectCodexInstalledTab(/已安装 skill/);
  const add = await screen.findByRole("button", { name: "添加 skill" });
  expect(
    screen.queryByText(codexAgent.globalSkillDirs[0]),
  ).not.toBeInTheDocument();
  expect(screen.queryByText(/共.*个 skill/)).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: /添加（/ }),
  ).not.toBeInTheDocument();
  fireEvent.click(add);
  expect(await screen.findByRole("dialog")).toHaveTextContent(
    "添加 skill 到 Codex",
  );
  fireEvent.click(screen.getByRole("button", { name: "Available" }));
  await waitFor(() =>
    expect(
      calls.find((c) => c.command === "register_skills")?.args,
    ).toMatchObject({ skillIds: ["available"], agentIds: ["codex"] }),
  );
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
});

it("manual installation tab counts and filters manual skills including policy-disabled entries", async () => {
  setup();
  handlers.set("scan_skills", () => [
    makeSkill({
      id: "manual",
      name: "Manual",
      agents: {
        codex: {
          ...agentState("source"),
          manual: true,
          disabled: true,
          policyBlocked: true,
        },
      },
    }),
    makeSkill({
      id: "studio",
      name: "Studio",
      agents: { codex: { ...agentState("copied"), manual: false } },
    }),
  ]);
  renderWithProviders(<GroupsPage />);
  await selectCodexInstalledTab(/已安装 skill/);
  const manualTab = await screen.findByRole("tab", { name: /未托管 skill.*1/ });
  expect(await screen.findByText("Manual")).toBeInTheDocument();
  expect(
    within(screen.getByText("Manual").parentElement!).getByText("未托管"),
  ).toBeInTheDocument();
  expect(screen.getByText("Studio")).toBeInTheDocument();
  fireEvent.mouseDown(manualTab, { button: 0, ctrlKey: false });
  expect(screen.queryByText("Studio")).not.toBeInTheDocument();
  const toggle = screen.getByRole("switch", { name: "启用 Manual" });
  expect(toggle).not.toBeChecked();
  expect(toggle).toBeDisabled();
});

it("keeps a consistent card until both activation config and skill status finish refreshing", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a"] }),
  ]);
  let active = false;
  let release!: (value: unknown) => void;
  const pendingSkills = new Promise((resolve) => {
    release = resolve;
  });
  handlers.set("get_config", () => ({
    activeGroups: active
      ? { codex: { groupId: "c", skillIds: ["a"], entries: [] } }
      : {},
  }));
  handlers.set("scan_skills", () =>
    active
      ? pendingSkills
      : [makeSkill({ id: "a", agents: { codex: agentState("source", true) } })],
  );
  handlers.set("activate_agent_group", () => {
    active = true;
  });
  renderWithProviders(<GroupsPage />);
  const enable = await screen.findByRole("button", { name: "为 Codex 启用" });
  fireEvent.click(enable);
  await waitFor(() =>
    expect(
      calls.filter((c) => c.command === "get_config").length,
    ).toBeGreaterThan(1),
  );
  expect(enable).toBeDisabled();
  expect(screen.queryByText("需要检查")).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "为 Codex 停用" }),
  ).not.toBeInTheDocument();
  await act(async () =>
    release([makeSkill({ id: "a", agents: { codex: agentState("source") } })]),
  );
  expect(
    await screen.findByRole("button", { name: "为 Codex 停用" }),
  ).toBeInTheDocument();
  expect(screen.getByText("使用中")).toBeInTheDocument();
  expect(screen.queryByText("需要检查")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "为 Codex 停用" })).toBe(enable);
});
