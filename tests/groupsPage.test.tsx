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
    expect(
      calls.find((c) => c.command === "activate_agent_group")?.args,
    ).toEqual({ agentId: "codex", groupIds: ["c"] }),
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
  fireEvent.click(screen.getAllByRole("button", { name: "新建分组" })[0]);
  expect(screen.getByLabelText("分组名称")).toHaveValue("");
  expect(
    screen.getByRole("checkbox", { name: "选择 Alpha" }),
  ).not.toBeChecked();
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
  handlers.set("activate_agent_group", (args) => {
    if (args.groupIds.length)
      throw new Error("Beta 在此 Agent 中不可用或被手动停用");
  });
  const activations = () =>
    calls
      .filter((c) => c.command === "activate_agent_group")
      .map((c) => c.args);
  renderWithProviders(<GroupsPage />);
  expect(await screen.findByText("有待应用修改")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "删除 Dev" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "应用修改「Dev」" }));
  await waitFor(() =>
    expect(activations()).toEqual([{ agentId: "codex", groupIds: ["c"] }]),
  );
  const stop = screen.getByRole("button", { name: "为 Codex 停用「Dev」" });
  await waitFor(() => expect(stop).not.toBeDisabled());
  fireEvent.click(stop);
  await waitFor(() =>
    expect(activations()).toEqual([
      { agentId: "codex", groupIds: ["c"] },
      { agentId: "codex", groupIds: [] },
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
      ? { codex: [{ groupId: "c", skillIds: ["a"], entries: [] }] }
      : {},
  }));
  handlers.set("activate_agent_group", (args) => {
    active = args.groupIds.length > 0;
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
  expect(
    calls
      .filter((c) => c.command === "activate_agent_group")
      .map((c) => c.args),
  ).toEqual([
    { agentId: "codex", groupIds: ["c"] },
    { agentId: "codex", groupIds: [] },
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
  handlers.set("activate_agent_group", () => {
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
    expect(
      calls.find((c) => c.command === "activate_agent_group")?.args,
    ).toEqual({ agentId: "codex", groupIds: [] }),
  );
});

it("disables every Agent icon while any switch is in progress", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a"] }),
  ]);
  handlers.set("activate_agent_group", () => new Promise(() => {}));
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
  expect(icon).toHaveClass("opacity-40");
  expect(icon).toBeEnabled();
  expect(
    screen.getByRole("button", { name: "为 Claude Code 启用「Dev」" }),
  ).not.toHaveClass("opacity-40");
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
  handlers.set("activate_agent_group", (args) => {
    active = { ...active, [args.agentId]: args.groupIds };
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
    expect(
      calls.filter((c) => c.command === "activate_agent_group").at(-1)?.args,
    ).toEqual({ agentId: "codex", groupIds: ["c", "o"] }),
  );
  // 叠加后两张卡片都是"停用"，胶囊的数字跟着变
  await viewChip("Codex 2");
  expect(
    await screen.findByRole("button", { name: "为 Codex 停用「Other」" }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "为 Codex 停用「Dev」" }));
  await waitFor(() =>
    expect(
      calls.filter((c) => c.command === "activate_agent_group").at(-1)?.args,
    ).toEqual({ agentId: "codex", groupIds: ["o"] }),
  );
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
      calls.find((c) => c.command === "activate_agent_group")?.args,
    ).toEqual({ agentId: "codex", groupIds: ["c"] }),
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
  handlers.set("activate_agent_group", (args) => {
    active = { ...active, [args.agentId]: args.groupIds };
  });
  renderWithProviders(<GroupsPage />);
  // 两个分组各点一次：两次都变成「已启用」，而不是互相顶掉
  fireEvent.click(
    await screen.findByRole("button", { name: "为 Codex 启用「X」" }),
  );
  await screen.findByRole("button", { name: "为 Codex 停用「X」" });
  fireEvent.click(screen.getByRole("button", { name: "为 Codex 启用「Y」" }));
  await screen.findByRole("button", { name: "为 Codex 停用「Y」" });
  await waitFor(() =>
    expect(
      calls.filter((c) => c.command === "activate_agent_group").at(-1)?.args,
    ).toEqual({ agentId: "codex", groupIds: ["x", "y"] }),
  );
  // 停用 X 只发一次含 Y 的列表，共用的 Beta 不受影响
  fireEvent.click(screen.getByRole("button", { name: "为 Codex 停用「X」" }));
  await waitFor(() =>
    expect(
      calls.filter((c) => c.command === "activate_agent_group").at(-1)?.args,
    ).toEqual({ agentId: "codex", groupIds: ["y"] }),
  );
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

it("整张分组卡片可点：点卡片打开编辑分组，卡片上的图标与删除按钮不会误开", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "c", name: "Dev", skillIds: ["a"] }),
  ]);
  renderWithProviders(<GroupsPage />);
  // 卡片本身就是进入编辑的入口，不再有单独的铅笔按钮
  fireEvent.click(await screen.findByRole("button", { name: "编辑 Dev" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByLabelText("分组名称")).toHaveValue("Dev");
  fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
  // 行内的就地按钮（启用图标、删除）仍然只做自己的事
  fireEvent.click(screen.getByRole("button", { name: "为 Codex 启用「Dev」" }));
  await waitFor(() =>
    expect(
      calls.find((c) => c.command === "activate_agent_group")?.args,
    ).toEqual({ agentId: "codex", groupIds: ["c"] }),
  );
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
