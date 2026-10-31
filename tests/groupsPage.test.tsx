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
  defaultSettings,
} from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

/** 分组页启停与重新应用发出的命令 */
const SWITCHES = ["set_agent_group", "reapply_agent_groups"];
const switched = () => calls.some((c) => SWITCHES.includes(c.command));
const setCalls = () =>
  calls.filter((c) => c.command === "set_agent_group").map((c) => c.args);

function setup() {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("scan_skills", () => [
    makeSkill({ id: "a", name: "Alpha" }),
    makeSkill({ id: "b", name: "Beta" }),
  ]);
  handlers.set("get_config", () => ({ activeGroups: {} }));
}

it("creates a shared draft and saves the full rapid selection without activating", async () => {
  setup();
  handlers.set("save_group", (args) => makeGroup(args));
  renderWithProviders(<GroupsPage />);
  fireEvent.click(screen.getByRole("button", { name: "新建分组" }));
  fireEvent.change(screen.getByLabelText("分组名称"), {
    target: { value: "Dev" },
  });
  fireEvent.click(await screen.findByRole("checkbox", { name: "Alpha" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Beta" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() =>
    expect(calls.find((c) => c.command === "save_group")?.args).toEqual({
      groupId: null,
      name: "Dev",
      skillIds: ["a", "b"],
      expected: null,
    }),
  );
  expect(switched()).toBe(false);
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

it("copyConflict 让分组卡片标出「需要检查」", async () => {
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
      codex: [{ groupId: "g", skillIds: ["a", "b"], entries: [] }],
    },
  }));
  renderWithProviders(<GroupsPage />);
  expect(await screen.findByText("需要检查")).toBeInTheDocument();
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
  // 每张卡片都有自己的 Codex 图标，名称里带上分组名，读屏也分得清
  expect(
    screen.getByRole("button", { name: "为 Codex 启用「Claude group」" }),
  ).toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", { name: "为 Codex 启用「Codex group」" }),
  );
  await waitFor(() =>
    expect(setCalls()).toEqual([
      { agentId: "codex", groupId: "c", enabled: true },
    ]),
  );
});
it("failed saves keep the editable draft, and going back discards it only after confirming", async () => {
  setup();
  handlers.set("save_group", () => {
    throw new Error("disk full");
  });
  renderWithProviders(<GroupsPage />);
  fireEvent.click(screen.getByRole("button", { name: "新建分组" }));
  fireEvent.change(screen.getByLabelText("分组名称"), {
    target: { value: "Dev" },
  });
  fireEvent.click(await screen.findByRole("checkbox", { name: "Alpha" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "保存失败，修改已保留：disk full",
  );
  expect(screen.getByLabelText("分组名称")).toHaveValue("Dev");
  expect(screen.getByRole("checkbox", { name: "Alpha" })).toBeChecked();
  fireEvent.click(screen.getByRole("button", { name: "返回" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "放弃修改并离开" }),
  );
  fireEvent.click(screen.getAllByRole("button", { name: "新建分组" })[0]);
  expect(await screen.findByLabelText("分组名称")).toHaveValue("");
  expect(screen.getByRole("checkbox", { name: "Alpha" })).not.toBeChecked();
});
it("pending changes get their own apply button, and the Agent icon still stops the group when re-applying is rejected", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a", "b"] }),
  ]);
  handlers.set("get_config", () => ({
    activeGroups: {
      codex: [{ groupId: "c", skillIds: ["a"], entries: [] }],
    },
  }));
  // 新成员在 Codex 上不可用：后端拒绝重新应用，但停用照常成功
  handlers.set("reapply_agent_groups", () => {
    throw new Error("Beta 在此 Agent 中不可用或被手动停用");
  });
  const activations = () =>
    calls
      .filter((c) => SWITCHES.includes(c.command))
      .map((c) => ({
        command: c.command,
        ...(c.args as Record<string, unknown>),
      }));
  renderWithProviders(<GroupsPage />);
  expect(await screen.findByText("有待应用修改")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "删除 Dev" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "应用修改「Dev」" }));
  await waitFor(() =>
    expect(activations()).toEqual([
      { command: "reapply_agent_groups", agentId: "codex" },
    ]),
  );
  const stop = screen.getByRole("button", { name: "为 Codex 停用「Dev」" });
  await waitFor(() => expect(stop).not.toBeDisabled());
  fireEvent.click(stop);
  await waitFor(() =>
    expect(activations()).toEqual([
      { command: "reapply_agent_groups", agentId: "codex" },
      {
        command: "set_agent_group",
        agentId: "codex",
        groupId: "c",
        enabled: false,
      },
    ]),
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
      expect(switched()).toBe(false);
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
      ? { codex: [{ groupId: "c", skillIds: ["a"], entries: [] }] }
      : {},
  }));
  handlers.set("set_agent_group", (args) => {
    active = args.enabled;
  });
  renderWithProviders(<GroupsPage />);
  fireEvent.click(
    await screen.findByRole("button", { name: "为 Codex 启用「Dev」" }),
  );
  const stop = await screen.findByRole("button", {
    name: "为 Codex 停用「Dev」",
  });
  await waitFor(() => expect(stop).not.toBeDisabled());
  expect(
    screen.queryByRole("button", { name: "为 Codex 启用「Dev」" }),
  ).toBeNull();
  fireEvent.click(stop);
  await screen.findByRole("button", { name: "为 Codex 启用「Dev」" });
  expect(setCalls()).toEqual([
    { agentId: "codex", groupId: "c", enabled: true },
    { agentId: "codex", groupId: "c", enabled: false },
  ]);
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
      ? { codex: [{ groupId: "c", skillIds: ["a"], entries: [] }] }
      : {},
  }));
  handlers.set("scan_skills", () =>
    active
      ? pendingSkills
      : [makeSkill({ id: "a", agents: { codex: agentState("source", true) } })],
  );
  handlers.set("set_agent_group", () => {
    active = true;
  });
  renderWithProviders(<GroupsPage />);
  const enable = await screen.findByRole("button", {
    name: "为 Codex 启用「Dev」",
  });
  fireEvent.click(enable);
  await waitFor(() =>
    expect(
      calls.filter((c) => c.command === "get_config").length,
    ).toBeGreaterThan(1),
  );
  expect(enable).toBeDisabled();
  expect(screen.queryByText("需要检查")).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "为 Codex 停用「Dev」" }),
  ).not.toBeInTheDocument();
  await act(async () =>
    release([makeSkill({ id: "a", agents: { codex: agentState("source") } })]),
  );
  expect(
    await screen.findByRole("button", { name: "为 Codex 停用「Dev」" }),
  ).toBeInTheDocument();
  expect(screen.getByText("使用中")).toBeInTheDocument();
  expect(screen.queryByText("需要检查")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "为 Codex 停用「Dev」" })).toBe(
    enable,
  );
});

it("hides Agents whose management is off, but keeps a stop-only icon while one still uses the group", async () => {
  setup();
  handlers.set("get_settings", () => ({
    ...defaultSettings(),
    disabledAgents: ["codex"],
  }));
  handlers.set("scan_skills", () => [
    makeSkill({
      id: "a",
      name: "Alpha",
      agents: { codex: agentState("source") },
    }),
    makeSkill({ id: "b", name: "Beta" }),
  ]);
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a"] }),
    makeGroup({ id: "o", name: "Other", skillIds: ["b"] }),
  ]);
  handlers.set("get_config", () => ({
    activeGroups: {
      codex: [{ groupId: "c", skillIds: ["a"], entries: [] }],
    },
  }));
  renderWithProviders(<GroupsPage />);
  // 没在用的卡片上不显示已关闭管理的 Agent
  expect(
    await screen.findByRole("button", { name: "为 Claude Code 启用「Other」" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: /为 Codex .*「Other」/ }),
  ).toBeNull();
  // 它仍在用 Dev：保留一个只能停用的图标，分组按在用处理，不能删
  const stop = screen.getByRole("button", {
    name: "为 Codex 停用「Dev」（已关闭管理）",
  });
  expect(screen.getByText("使用中")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "删除 Dev" })).toBeDisabled();
  fireEvent.click(stop);
  await waitFor(() =>
    expect(setCalls()).toEqual([
      { agentId: "codex", groupId: "c", enabled: false },
    ]),
  );
});

it("disables every Agent icon while any switch is in progress", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a"] }),
  ]);
  handlers.set("set_agent_group", () => new Promise(() => {}));
  renderWithProviders(<GroupsPage />);
  fireEvent.click(
    await screen.findByRole("button", { name: "为 Codex 启用「Dev」" }),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "为 Claude Code 启用「Dev」" }),
    ).toBeDisabled(),
  );
  expect(
    screen.getByRole("button", { name: "为 Codex 启用「Dev」" }),
  ).toBeDisabled();
});

it("does not report pending changes when only the member order differs", async () => {
  setup();
  handlers.set("scan_skills", () => [
    makeSkill({
      id: "a",
      name: "Alpha",
      agents: { codex: agentState("source") },
    }),
    makeSkill({
      id: "b",
      name: "Beta",
      agents: { codex: agentState("source") },
    }),
  ]);
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["b", "a"] }),
  ]);
  handlers.set("get_config", () => ({
    activeGroups: {
      codex: [{ groupId: "c", skillIds: ["a", "b"], entries: [] }],
    },
  }));
  renderWithProviders(<GroupsPage />);
  expect(await screen.findByText("使用中")).toBeInTheDocument();
  expect(screen.queryByText("有待应用修改")).toBeNull();
  expect(screen.queryByRole("button", { name: "应用修改「Dev」" })).toBeNull();
});

it("dims Agents that are not installed instead of hiding them", async () => {
  setup();
  handlers.set("list_agents", () => [
    claudeAgent,
    { ...codexAgent, detected: false },
  ]);
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a"] }),
  ]);
  renderWithProviders(<GroupsPage />);
  const icon = await screen.findByRole("button", {
    name: "为 Codex 启用「Dev」（未检测到安装）",
  });
  expect(icon.querySelector("svg")).toHaveClass("grayscale", "opacity-25");
  expect(icon).toBeEnabled();
  expect(
    screen
      .getByRole("button", { name: "为 Claude Code 启用「Dev」" })
      .querySelector("svg"),
  ).toHaveClass("grayscale", "opacity-40");
});

/** 分组页顶部的 Agent 视角胶囊 */
async function viewChip(name: string | RegExp) {
  return within(
    await screen.findByRole("group", { name: "按 Agent 查看" }),
  ).findByRole("button", { name });
}

it("switches to one Agent's view: that Agent's buttons only, its own stats, and stacking adds up", async () => {
  setup();
  handlers.set("scan_skills", () => [
    makeSkill({
      id: "a",
      name: "Alpha",
      tokens: { skillMd: 400, extras: 0 },
      agents: { codex: agentState("copied") },
    }),
    makeSkill({ id: "b", name: "Beta", tokens: { skillMd: 200, extras: 0 } }),
    // 不在任何分组里、手动装在 Codex 的 skill 也算进这个 Agent 的统计
    makeSkill({
      id: "m",
      name: "Manual",
      tokens: { skillMd: 300, extras: 0 },
      agents: { codex: agentState("source") },
    }),
  ]);
  const stack = [
    makeGroup({ id: "o", name: "Other", sortOrder: 0, skillIds: ["b"] }),
    makeGroup({ id: "c", name: "Dev", sortOrder: 1, skillIds: ["a"] }),
  ];
  handlers.set("list_groups", () => stack);
  let active: Record<string, string[]> = { codex: ["c"] };
  // 启用记录是「每个 agent 一组分组记录」，和配置里的形状一致
  const records = () =>
    Object.fromEntries(
      Object.entries(active).map(([agent, ids]) => [
        agent,
        ids.map((id) => ({
          groupId: id,
          skillIds: stack.find((g) => g.id === id)?.skillIds ?? [],
          entries: [],
        })),
      ]),
    );
  handlers.set("get_config", () => ({ activeGroups: records() }));
  // 与后端一致：只改这一个分组，其余按当时的启用记录保留
  handlers.set("set_agent_group", (args) => {
    const ids = active[args.agentId] ?? [];
    active = {
      ...active,
      [args.agentId]: args.enabled
        ? [...ids, args.groupId]
        : ids.filter((id) => id !== args.groupId),
    };
  });
  renderWithProviders(<GroupsPage />);
  fireEvent.click(await viewChip("Codex 1"));
  expect(
    screen.getByRole("button", { name: "为 Codex 停用「Dev」" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "为 Codex 启用「Other」" }),
  ).toBeInTheDocument();
  // 卡片上只有这个 Agent 的按钮
  expect(screen.queryByRole("button", { name: /为 Claude Code/ })).toBeNull();
  // 右侧是这个 Agent 自己的统计
  expect(screen.getByText("已启用 2")).toBeInTheDocument();
  expect(screen.getByText("合计 ≈ 700 tokens")).toBeInTheDocument();
  // 视角里不能拖动排序（排序属于「全部」视角）
  expect(screen.getByRole("button", { name: "拖拽排序 Dev" })).toBeDisabled();
  fireEvent.click(
    screen.getByRole("button", { name: "为 Codex 启用「Other」" }),
  );
  await waitFor(() =>
    expect(setCalls().at(-1)).toEqual({
      agentId: "codex",
      groupId: "o",
      enabled: true,
    }),
  );
  expect(active.codex).toEqual(["c", "o"]);
  // 叠加后两张卡片都是"停用"，胶囊的数字跟着变
  await viewChip("Codex 2");
  expect(
    await screen.findByRole("button", { name: "为 Codex 停用「Other」" }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "为 Codex 停用「Dev」" }));
  await waitFor(() =>
    expect(setCalls().at(-1)).toEqual({
      agentId: "codex",
      groupId: "c",
      enabled: false,
    }),
  );
  expect(active.codex).toEqual(["o"]);
});

it("remembers the Agent view after leaving the page, and 全部 brings back the icon rows", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a"] }),
  ]);
  const first = renderWithProviders(<GroupsPage />);
  fireEvent.click(await viewChip("Codex 0"));
  expect(
    screen.getByRole("button", { name: "为 Codex 启用「Dev」" }),
  ).toBeInTheDocument();
  first.unmount();
  renderWithProviders(<GroupsPage />);
  expect(
    await screen.findByRole("button", { name: "为 Codex 启用「Dev」" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "为 Claude Code 启用「Dev」" }),
  ).toBeNull();
  fireEvent.click(await viewChip(/^全部/));
  expect(
    screen.getByRole("button", { name: "为 Claude Code 启用「Dev」" }),
  ).toBeInTheDocument();
});

it("applies pending changes for the selected Agent from its view", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a", "b"] }),
  ]);
  handlers.set("get_config", () => ({
    activeGroups: {
      codex: [{ groupId: "c", skillIds: ["a"], entries: [] }],
    },
  }));
  renderWithProviders(<GroupsPage />);
  fireEvent.click(await viewChip("Codex 1"));
  expect(screen.getByText("有待应用修改")).toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", { name: "为 Codex 应用修改「Dev」" }),
  );
  await waitFor(() =>
    expect(
      calls.find((c) => c.command === "reapply_agent_groups")?.args,
    ).toEqual({ agentId: "codex" }),
  );
});

it("stacks two groups for one Agent from the 全部 view, and stopping one keeps the shared member", async () => {
  setup();
  handlers.set("scan_skills", () => [
    makeSkill({ id: "a", name: "Alpha" }),
    makeSkill({ id: "b", name: "Beta" }),
    makeSkill({ id: "c", name: "Gamma" }),
  ]);
  const stack = [
    makeGroup({ id: "x", name: "X", skillIds: ["a", "b"] }),
    makeGroup({ id: "y", name: "Y", skillIds: ["b", "c"] }),
  ];
  handlers.set("list_groups", () => stack);
  let active: Record<string, string[]> = {};
  const records = () =>
    Object.fromEntries(
      Object.entries(active).map(([agent, ids]) => [
        agent,
        ids.map((id) => ({
          groupId: id,
          skillIds: stack.find((g) => g.id === id)?.skillIds ?? [],
          entries: [],
        })),
      ]),
    );
  handlers.set("get_config", () => ({ activeGroups: records() }));
  // 与后端一致：只改这一个分组，其余按当时的启用记录保留
  handlers.set("set_agent_group", (args) => {
    const ids = active[args.agentId] ?? [];
    active = {
      ...active,
      [args.agentId]: args.enabled
        ? [...ids, args.groupId]
        : ids.filter((id) => id !== args.groupId),
    };
  });
  renderWithProviders(<GroupsPage />);
  // 两个分组各点一次：两次都变成「已启用」，而不是互相顶掉
  fireEvent.click(
    await screen.findByRole("button", { name: "为 Codex 启用「X」" }),
  );
  await screen.findByRole("button", { name: "为 Codex 停用「X」" });
  fireEvent.click(screen.getByRole("button", { name: "为 Codex 启用「Y」" }));
  await screen.findByRole("button", { name: "为 Codex 停用「Y」" });
  expect(active.codex).toEqual(["x", "y"]);
  // 停用 X 只停这一个，Y 和共用的 Beta 不受影响
  fireEvent.click(screen.getByRole("button", { name: "为 Codex 停用「X」" }));
  await waitFor(() =>
    expect(setCalls().at(-1)).toEqual({
      agentId: "codex",
      groupId: "x",
      enabled: false,
    }),
  );
  expect(active.codex).toEqual(["y"]);
  expect(
    await screen.findByRole("button", { name: "为 Codex 停用「Y」" }),
  ).toBeInTheDocument();
  await viewChip("Codex 1");
});
it("分组卡片按名字生成图标：首字不同、同一个分组前后一致", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "前端", skillIds: ["a"] }),
    makeGroup({ id: "d", name: "LLM", skillIds: ["b"] }),
  ]);
  renderWithProviders(<GroupsPage />);
  await screen.findByText("前端");
  // 首字直接来自分组名，两张卡片因此不再是同一个图标
  const first = screen.getByText("前");
  const second = screen.getByText("L");
  expect(first).toBeInTheDocument();
  expect(second).toBeInTheDocument();
  // 记号靠名字算出来，不依赖任何分组上的字段
  expect(first.parentElement?.className).toMatch(/bg-\S+ text-/);
});

it("整张分组卡片可点：点卡片在页内展开编辑，卡片上的图标不会误开", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a"] }),
  ]);
  renderWithProviders(<GroupsPage />);
  // 卡片本身就是进入编辑的入口，不再有单独的铅笔按钮
  fireEvent.click(await screen.findByRole("button", { name: "编辑 Dev" }));
  expect(await screen.findByLabelText("分组名称")).toHaveValue("Dev");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "返回" }));
  // 行内的就地按钮（启用图标、删除）仍然只做自己的事
  fireEvent.click(
    await screen.findByRole("button", { name: "为 Codex 启用「Dev」" }),
  );
  await waitFor(() =>
    expect(setCalls()).toEqual([
      { agentId: "codex", groupId: "c", enabled: true },
    ]),
  );
  expect(screen.queryByLabelText("分组名称")).not.toBeInTheDocument();
});

it("启停只告诉后端这一个分组：别处刚启用、页面还没刷新出来的分组不会被撤掉", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a"] }),
    makeGroup({ id: "o", name: "Other", skillIds: ["b"] }),
  ]);
  let active: string[] = [];
  handlers.set("get_config", () => ({
    activeGroups: {
      codex: active.map((groupId) => ({ groupId, skillIds: [], entries: [] })),
    },
  }));
  handlers.set("set_agent_group", (args) => {
    active = args.enabled
      ? [...active, args.groupId]
      : active.filter((id) => id !== args.groupId);
    return [];
  });
  renderWithProviders(<GroupsPage />);
  const enable = await screen.findByRole("button", {
    name: "为 Codex 启用「Dev」",
  });
  // 命令行这时为 Codex 启用了 Other，页面还没收到刷新
  active = ["o"];
  fireEvent.click(enable);
  await waitFor(() => expect(active).toEqual(["o", "c"]));
});

it("保存时带上打开编辑时的样子：别处改过就提示，修改留在页面上；保存成功后以刚存的为准", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a"] }),
  ]);
  let conflict = true;
  handlers.set("save_group", (args) => {
    if (conflict)
      throw new Error(
        "分组已被别处修改（另一个窗口或命令行），请按最新内容重新修改",
      );
    return makeGroup({
      id: args.groupId,
      name: args.name,
      skillIds: args.skillIds,
    });
  });
  const saves = () =>
    calls.filter((c) => c.command === "save_group").map((c) => c.args);
  renderWithProviders(<GroupsPage />);
  fireEvent.click(await screen.findByRole("button", { name: "编辑 Dev" }));
  fireEvent.click(await screen.findByRole("checkbox", { name: "Beta" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "保存失败，修改已保留：分组已被别处修改",
  );
  expect(saves()).toEqual([
    {
      groupId: "c",
      name: "Dev",
      skillIds: ["a", "b"],
      expected: { name: "Dev", skillIds: ["a"] },
    },
  ]);
  expect(screen.getByRole("checkbox", { name: "Beta" })).toBeChecked();

  conflict = false;
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(saves()).toHaveLength(2));
  fireEvent.click(await screen.findByRole("checkbox", { name: "Alpha" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() =>
    expect(saves().at(-1)).toEqual({
      groupId: "c",
      name: "Dev",
      skillIds: ["b"],
      expected: { name: "Dev", skillIds: ["a", "b"] },
    }),
  );
});

it("正在使用的分组不能清空成员：说明先停用，改动留在页面上", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a"] }),
  ]);
  handlers.set("get_config", () => ({
    activeGroups: {
      codex: [{ groupId: "c", skillIds: ["a"], entries: [] }],
    },
  }));
  handlers.set("save_group", (args) => {
    if ((args.skillIds as string[]).length === 0)
      throw new Error(
        "分组「Dev」正在 Codex 上使用，清空成员前请先为这些 Agent 停用",
      );
    return makeGroup({
      id: args.groupId,
      name: args.name,
      skillIds: args.skillIds as string[],
    });
  });
  renderWithProviders(<GroupsPage />);
  fireEvent.click(await screen.findByRole("button", { name: "编辑 Dev" }));
  fireEvent.click(await screen.findByRole("checkbox", { name: "Alpha" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "清空成员前请先为这些 Agent 停用",
  );
  // 拒绝时什么都没保存：未勾选的草稿留着，返回或重新保存都能继续
  expect(screen.getByRole("checkbox", { name: "Alpha" })).not.toBeChecked();
  expect(
    calls.filter((c) => c.command === "save_group").at(-1)?.args,
  ).toMatchObject({ skillIds: [] });
  expect(screen.getByRole("button", { name: "保存" })).not.toBeDisabled();
});
