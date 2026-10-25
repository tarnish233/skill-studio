import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, it } from "vitest";
import { McpGroupsPage } from "@/pages/McpGroupsPage";
import { renderWithProviders } from "./utils/render";
import {
  handlers,
  calls,
  claudeAgent,
  codexAgent,
  defaultSettings,
} from "./mocks/tauri";
import { setTarget } from "@/lib/api/transport";
import type { ManagementStatus } from "@/lib/api/mcpManagement";
import { rows } from "@/lib/api/mcpManagement";

let status: ManagementStatus;
const entry = {
  id: "one",
  name: "Work",
  mode: "direct" as const,
  definition: { type: "stdio", command: "work" },
  oauth: false,
  clientId: null,
  scopes: [],
  bindings: [],
};
// 分组不归属某个 Agent：名称和成员只保存一次，是否对某个 Agent 生效由
// activeGroups 里各自的记录独立决定（见下面 beforeEach 的 setGroup mock）。
const group = {
  id: "g1",
  name: "开发",
  entryIds: ["one"],
  sortOrder: 0,
};
const methods = () =>
  calls
    .filter((c) => c.command === "mcp_request")
    .map((c) => (c.args as { method: string }).method);
const mcpCalls = (method: string) =>
  calls
    .filter(
      (c) =>
        c.command === "mcp_request" &&
        (c.args as { method: string }).method === method,
    )
    .map((c) => (c.args as { params: Record<string, unknown> }).params);
/** 每次启停只带这一个分组 */
const activations = () => mcpCalls("setGroup");
const reapplies = () => mcpCalls("reapplyGroups");
function card(name: string) {
  return screen.getByText(name).closest('[class*="rounded-xl"]') as HTMLElement;
}
beforeEach(() => {
  setTarget({ id: "local", name: "本机", connected: true });
  status = {
    running: false,
    entries: [entry],
    servers: [],
    discovered: [],
    scanWarnings: [],
    groups: [group],
    activeGroups: {},
  };
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("get_config", () => ({ activeGroups: {} }));
  handlers.set("mcp_request", ({ method, params }) => {
    if (method === "list") return status;
    if (method === "saveGroup")
      status = {
        ...status,
        groups: [
          ...(status.groups ?? []).filter((g) => g.id !== params.group.id),
          params.group,
        ],
      };
    if (method === "setGroup") {
      // 后端在锁内按最新的启用记录增减这一个分组
      const agent = params.agent as string;
      const current = (status.activeGroups?.[agent] ?? []).filter(
        (g) => g.groupId !== params.groupId,
      );
      const ids = [
        ...current,
        ...(params.enabled
          ? [
              {
                groupId: params.groupId as string,
                entries: [entry],
                bindings: [],
              },
            ]
          : []),
      ];
      const next = { ...status.activeGroups };
      if (ids.length) next[agent] = ids;
      else delete next[agent];
      status = { ...status, activeGroups: next };
      return [];
    }
    if (method === "reapplyGroups") return [];
    return null;
  });
});
async function open() {
  renderWithProviders(<McpGroupsPage />);
  await screen.findByRole("button", { name: "编辑 开发" });
}
it("shows a shared MCP group once, and each Agent's icon toggles only that Agent, without starting the gateway", async () => {
  status.groups!.push({ ...group, id: "other", name: "Codex only" });
  await open();
  // 分组不再按 Agent 过滤：两个组同时可见
  expect(screen.getByText("Codex only")).toBeInTheDocument();
  fireEvent.click(
    within(card("开发")).getByRole("button", {
      name: "为 Claude Code 启用「开发」",
    }),
  );
  await screen.findByText("使用中");
  expect(screen.getByRole("button", { name: "删除 开发" })).toBeDisabled();
  fireEvent.click(
    within(card("开发")).getByRole("button", {
      name: "为 Claude Code 停用「开发」",
    }),
  );
  await waitFor(() =>
    expect(screen.queryByText("使用中")).not.toBeInTheDocument(),
  );
  expect(methods()).not.toContain("start");
  expect(activations()).toEqual([
    { agent: "claude", groupId: "g1", enabled: true },
    { agent: "claude", groupId: "g1", enabled: false },
  ]);
});
it("lets two Agents activate the same shared group independently", async () => {
  await open();
  fireEvent.click(
    within(card("开发")).getByRole("button", {
      name: "为 Claude Code 启用「开发」",
    }),
  );
  await screen.findByText("使用中");
  fireEvent.click(
    within(card("开发")).getByRole("button", { name: "为 Codex 启用「开发」" }),
  );
  await waitFor(() =>
    expect(activations()).toEqual([
      { agent: "claude", groupId: "g1", enabled: true },
      { agent: "codex", groupId: "g1", enabled: true },
    ]),
  );
  // 两个 Agent 的图标都停在"已启用"，互不覆盖
  expect(
    within(card("开发")).getByRole("button", {
      name: "为 Claude Code 停用「开发」",
    }),
  ).toBeInTheDocument();
  expect(
    within(card("开发")).getByRole("button", { name: "为 Codex 停用「开发」" }),
  ).toBeInTheDocument();
});
it("saving writes the group to the Agents already using it, and the Agent icon still stops it", async () => {
  const second = {
    ...entry,
    id: "two",
    name: "Two",
    definition: { type: "stdio", command: "two" },
  };
  status.entries = [entry, second];
  status.activeGroups = {
    claude: [{ groupId: "g1", entries: [entry], bindings: [] }],
  };
  await open();
  fireEvent.click(screen.getByRole("button", { name: "编辑 开发" }));
  fireEvent.click(await screen.findByRole("checkbox", { name: "Two" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(reapplies()).toEqual([{ agent: "claude" }]));
  expect(methods().indexOf("saveGroup")).toBeLessThan(
    methods().indexOf("reapplyGroups"),
  );
  // 带上打开编辑时的样子，别处改过时后端拒绝保存
  expect(mcpCalls("saveGroup")[0]).toMatchObject({
    group: { id: "g1", entryIds: ["one", "two"] },
    expected: { name: "开发", entryIds: ["one"] },
  });
  // 留在整页编辑里：标题栏的图标照样能停用
  const stop = await screen.findByRole("button", {
    name: "为 Claude Code 停用「开发」",
  });
  await waitFor(() => expect(stop).toBeEnabled());
  fireEvent.click(stop);
  await waitFor(() =>
    expect(activations()).toEqual([
      { agent: "claude", groupId: "g1", enabled: false },
    ]),
  );
});
it("keeps the stop action when an active member is gone and re-applying cannot succeed", async () => {
  // "one" 已不在目录里：后端会以「分组包含已缺失的 MCP」拒绝重新应用
  status.entries = [];
  status.activeGroups = {
    claude: [{ groupId: "g1", entries: [entry], bindings: [] }],
  };
  await open();
  expect(screen.getByText("成员已缺失")).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "应用修改「开发」" }),
  ).toBeDisabled();
  fireEvent.click(
    screen.getByRole("button", { name: "为 Claude Code 停用「开发」" }),
  );
  await waitFor(() =>
    expect(activations()).toEqual([
      { agent: "claude", groupId: "g1", enabled: false },
    ]),
  );
});
it("hides Agents whose management is off, but keeps a stop-only icon while one still uses the group", async () => {
  handlers.set("get_settings", () => ({
    ...defaultSettings(),
    disabledAgents: ["claude-code"],
  }));
  status.groups!.push({ ...group, id: "other", name: "其他" });
  status.activeGroups = {
    claude: [{ groupId: "g1", entries: [entry], bindings: [] }],
  };
  await open();
  // 没在用的卡片上不显示已关闭管理的 Agent
  expect(
    screen.getByRole("button", { name: "为 Codex 启用「其他」" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: /为 Claude Code .*「其他」/ }),
  ).toBeNull();
  // 它仍在用「开发」：保留一个只能停用的图标，分组按在用处理，不能删
  const stop = screen.getByRole("button", {
    name: "为 Claude Code 停用「开发」（已关闭管理）",
  });
  expect(screen.getByText("使用中")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "删除 开发" })).toBeDisabled();
  fireEvent.click(stop);
  await waitFor(() =>
    expect(activations()).toEqual([
      { agent: "claude", groupId: "g1", enabled: false },
    ]),
  );
});
it("records discovered MCP references without adopting or activating on save", async () => {
  status.discovered = [
    {
      id: "scanned",
      name: "Scan",
      sources: [
        {
          id: "s1",
          agent: "codex",
          path: "/test/config.toml",
          scope: "用户全局",
          key: "Scan",
          enabled: true,
          gateway: false,
          project: null,
          definition: { command: "scan" },
        },
      ],
      server: null,
      issue: null,
      managedId: null,
    },
  ];
  await open();
  fireEvent.click(screen.getByRole("button", { name: "新建分组" }));
  fireEvent.change(screen.getByRole("textbox", { name: "分组名称" }), {
    target: { value: "浏览器" },
  });
  fireEvent.click(await screen.findByRole("checkbox", { name: "Scan" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(methods()).toContain("saveGroup"));
  expect(
    calls.find(
      (c) =>
        c.command === "mcp_request" && (c.args as any).method === "saveGroup",
    )?.args,
  ).toMatchObject({
    params: {
      group: { name: "浏览器", entryIds: ["scanned"] },
      imports: [
        { id: "scanned", definition: { type: "stdio", command: "scan" } },
      ],
      // 新建的分组没有可核对的旧样子
      expected: null,
    },
  });
  expect(methods()).not.toContain("setGroup");
});
it("does not duplicate Hub entries when group activation changes the native enabled field", () => {
  const binding = {
    id: entry.id,
    agent: "codex",
    path: "/test/config.toml",
    project: null,
    key: "Work",
    original: null,
    installed: { command: "work", enabled: true },
  };
  status.activeGroups = {
    codex: [{ groupId: "g1", entries: [entry], bindings: [binding] }],
  };
  status.discovered = [
    {
      id: "changed",
      name: "Work",
      sources: [
        {
          ...binding,
          id: "source",
          scope: "用户全局",
          enabled: true,
          gateway: false,
          definition: binding.installed,
        },
      ],
      server: null,
      issue: null,
      managedId: null,
    },
  ];
  expect(rows(status)).toHaveLength(1);
  expect(rows(status)[0].sources).toHaveLength(1);
});

it("shows only group cards: no tabs and no Agent picker, single MCPs are added from the Hub", async () => {
  await open();
  expect(screen.queryByRole("tab")).not.toBeInTheDocument();
  expect(screen.queryByLabelText("选择 Agent")).not.toBeInTheDocument();
  expect(
    screen.getByRole("textbox", { name: "搜索分组…" }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "新建分组" }));
  expect(await screen.findByRole("textbox", { name: "分组名称" })).toHaveValue(
    "",
  );
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(methods()).not.toContain("saveEntry");
  expect(methods()).not.toContain("setGroup");
});

it("keeps reference identity and original definition after deployment changes scan order", () => {
  const source = {
    id: "source",
    agent: "codex",
    path: "/codex/config.toml",
    project: null,
    key: "Work",
    scope: "用户全局",
    gateway: false,
    enabled: true,
    definition: { command: "work" },
  };
  const reference = {
    ...entry,
    id: "original",
    bindings: [
      { ...source, original: source.definition, installed: source.definition },
    ],
  };
  status.entries = [];
  status.groups = [
    { ...group, entryIds: ["original"], references: [reference] },
  ];
  status.discovered = [
    {
      id: "new-scan-id",
      name: "Work",
      server: null,
      issue: null,
      managedId: null,
      sources: [
        {
          ...source,
          id: "deployed",
          agent: "claude",
          path: "/claude.json",
          definition: { command: "work", enabled: true },
        },
        source,
      ],
    },
  ];
  const result = rows(status);
  expect(result).toHaveLength(1);
  expect(result[0].managed).toBe(false);
  expect(result[0].entry.id).toBe("original");
  expect(result[0].entry.definition).toEqual({
    type: "stdio",
    command: "work",
  });
});

it("switches to one Agent's view with its own MCP count and a single enable button", async () => {
  status.discovered = [
    {
      id: "work",
      name: "Work",
      sources: [
        {
          id: "s1",
          agent: "codex",
          path: "/test/config.toml",
          scope: "用户全局",
          key: "Work",
          enabled: true,
          gateway: false,
          project: null,
          definition: { command: "work" },
        },
      ],
      server: null,
      issue: null,
      managedId: "one",
    },
  ];
  await open();
  fireEvent.click(
    within(screen.getByRole("group", { name: "按 Agent 查看" })).getByRole(
      "button",
      { name: "Codex 0" },
    ),
  );
  expect(screen.getByText("已启用 1")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /为 Claude Code/ })).toBeNull();
  fireEvent.click(
    screen.getByRole("button", { name: "为 Codex 启用「开发」" }),
  );
  await waitFor(() =>
    expect(activations()).toEqual([
      { agent: "codex", groupId: "g1", enabled: true },
    ]),
  );
});

it("stacks two MCP groups for one Agent instead of replacing the first", async () => {
  status.groups!.push({
    ...group,
    id: "g2",
    name: "文档",
    entryIds: ["one"],
    sortOrder: 1,
  });
  await open();
  fireEvent.click(
    within(card("开发")).getByRole("button", {
      name: "为 Claude Code 启用「开发」",
    }),
  );
  await waitFor(() =>
    expect(activations()).toEqual([
      { agent: "claude", groupId: "g1", enabled: true },
    ]),
  );
  fireEvent.click(
    within(card("文档")).getByRole("button", {
      name: "为 Claude Code 启用「文档」",
    }),
  );
  await waitFor(() =>
    expect(activations().at(-1)).toEqual({
      agent: "claude",
      groupId: "g2",
      enabled: true,
    }),
  );
  // 两个分组都停在"已启用"，胶囊显示叠加数量
  expect(
    within(card("开发")).getByRole("button", {
      name: "为 Claude Code 停用「开发」",
    }),
  ).toBeInTheDocument();
  expect(
    within(card("文档")).getByRole("button", {
      name: "为 Claude Code 停用「文档」",
    }),
  ).toBeInTheDocument();
  expect(
    within(screen.getByRole("group", { name: "按 Agent 查看" })).getByRole(
      "button",
      { name: "Claude Code 2" },
    ),
  ).toBeInTheDocument();
  // 停用一个，另一个继续生效
  fireEvent.click(
    within(card("开发")).getByRole("button", {
      name: "为 Claude Code 停用「开发」",
    }),
  );
  await waitFor(() =>
    expect(activations().at(-1)).toEqual({
      agent: "claude",
      groupId: "g1",
      enabled: false,
    }),
  );
  expect(status.activeGroups?.claude?.map((g) => g.groupId)).toEqual(["g2"]);
  expect(
    within(card("文档")).getByRole("button", {
      name: "为 Claude Code 停用「文档」",
    }),
  ).toBeInTheDocument();
});
it("整张 MCP 分组卡片可点：点卡片在页内展开编辑，不再有铅笔按钮", async () => {
  status.groups = [{ ...group, id: "g1", name: "开发", entryIds: ["one"] }];
  await open();
  fireEvent.click(screen.getByRole("button", { name: "编辑 开发" }));
  expect(await screen.findByLabelText("分组名称")).toHaveValue("开发");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "返回" }));
  expect(
    await screen.findByRole("button", { name: "编辑 开发" }),
  ).toBeInTheDocument();
});

it("启停只告诉后端这一个分组：别处刚启用、页面还没刷新出来的分组不会被撤掉", async () => {
  status.groups!.push({ ...group, id: "g2", name: "文档", sortOrder: 1 });
  await open();
  const enable = within(card("开发")).getByRole("button", {
    name: "为 Claude Code 启用「开发」",
  });
  // 命令行这时为 Claude Code 启用了「文档」，页面还没收到刷新
  status = {
    ...status,
    activeGroups: {
      claude: [{ groupId: "g2", entries: [entry], bindings: [] }],
    },
  };
  fireEvent.click(enable);
  await waitFor(() =>
    expect(status.activeGroups?.claude?.map((g) => g.groupId)).toEqual([
      "g2",
      "g1",
    ]),
  );
  expect(methods()).not.toContain("activateGroup");
});

it("保存时别处改过：提示冲突，修改留在页面上；保存成功后以刚存的为准", async () => {
  const second = {
    ...entry,
    id: "two",
    name: "Two",
    definition: { type: "stdio", command: "two" },
  };
  status.entries = [entry, second];
  let conflict = true;
  const answer = handlers.get("mcp_request")!;
  handlers.set("mcp_request", (args) => {
    if (args.method === "saveGroup" && conflict)
      throw new Error(
        "分组已被别处修改（另一个窗口或命令行），请按最新内容重新修改",
      );
    return answer(args);
  });
  await open();
  fireEvent.click(screen.getByRole("button", { name: "编辑 开发" }));
  fireEvent.click(await screen.findByRole("checkbox", { name: "Two" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "保存失败，修改已保留：分组已被别处修改",
  );
  expect(screen.getByRole("checkbox", { name: "Two" })).toBeChecked();
  conflict = false;
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(mcpCalls("saveGroup")).toHaveLength(2));
  fireEvent.click(await screen.findByRole("checkbox", { name: "Work" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() =>
    expect(mcpCalls("saveGroup").at(-1)).toMatchObject({
      group: { id: "g1", entryIds: ["two"] },
      expected: { name: "开发", entryIds: ["one", "two"] },
    }),
  );
});
