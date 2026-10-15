import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, it } from "vitest";
import { McpGroupsPage } from "@/pages/McpGroupsPage";
import { renderWithProviders } from "./utils/render";
import { handlers, calls, claudeAgent, codexAgent } from "./mocks/tauri";
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
// activeGroups 里各自的记录独立决定（见下面 beforeEach 的 activateGroup mock）。
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
    if (method === "activateGroup") {
      const next = { ...status.activeGroups };
      if (params.id)
        next[params.agent as string] = {
          groupId: params.id,
          entries: [entry],
          bindings: [],
        };
      else delete next[params.agent as string];
      status = { ...status, activeGroups: next };
    }
    return null;
  });
});
async function open() {
  renderWithProviders(<McpGroupsPage />);
  expect(screen.getByRole("tab", { name: /^分组/ })).toHaveAttribute(
    "data-state",
    "active",
  );
  await screen.findByRole("button", { name: "编辑 开发" });
}
it("shows a shared MCP group once, and each Agent's icon toggles only that Agent, without starting the gateway", async () => {
  status.groups!.push({ ...group, id: "other", name: "Codex only" });
  await open();
  // 分组不再按 Agent 过滤：两个组同时可见
  expect(screen.getByText("Codex only")).toBeInTheDocument();
  fireEvent.click(
    within(card("开发")).getByRole("button", { name: "为 Claude Code 启用" }),
  );
  await screen.findByText("使用中");
  expect(screen.getByRole("button", { name: "删除 开发" })).toBeDisabled();
  fireEvent.click(
    within(card("开发")).getByRole("button", { name: "为 Claude Code 停用" }),
  );
  await waitFor(() =>
    expect(screen.queryByText("使用中")).not.toBeInTheDocument(),
  );
  expect(methods()).not.toContain("start");
  expect(calls.some((c) => c.command === "activate_agent_group")).toBe(false);
  expect(
    calls
      .filter(
        (c) =>
          c.command === "mcp_request" &&
          (c.args as any).method === "activateGroup",
      )
      .map((c) => (c.args as any).params),
  ).toEqual([
    { agent: "claude", id: "g1" },
    { agent: "claude", id: null },
  ]);
});
it("lets two Agents activate the same shared group independently", async () => {
  await open();
  fireEvent.click(
    within(card("开发")).getByRole("button", { name: "为 Claude Code 启用" }),
  );
  await screen.findByText("使用中");
  fireEvent.click(
    within(card("开发")).getByRole("button", { name: "为 Codex 启用" }),
  );
  await waitFor(() =>
    expect(
      calls
        .filter(
          (c) =>
            c.command === "mcp_request" &&
            (c.args as any).method === "activateGroup",
        )
        .map((c) => (c.args as any).params),
    ).toEqual([
      { agent: "claude", id: "g1" },
      { agent: "codex", id: "g1" },
    ]),
  );
  // 两个 Agent 的图标都停在"已启用"，互不覆盖
  expect(
    within(card("开发")).getByRole("button", { name: "为 Claude Code 停用" }),
  ).toBeInTheDocument();
  expect(
    within(card("开发")).getByRole("button", { name: "为 Codex 停用" }),
  ).toBeInTheDocument();
});
it("saves membership without applying it, and re-clicking the Agent icon re-applies instead of toggling off", async () => {
  status.activeGroups = {
    claude: { groupId: "g1", entries: [entry], bindings: [] },
  };
  await open();
  fireEvent.click(screen.getByRole("button", { name: "编辑 开发" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "选择 Work" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await screen.findByText("有待应用修改");
  expect(methods()).toContain("saveGroup");
  expect(methods()).not.toContain("activateGroup");
  expect(
    screen.getByRole("button", { name: "为 Claude Code 重新应用" }),
  ).toBeEnabled();
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
  fireEvent.click(screen.getByRole("button", { name: "新建 MCP 分组" }));
  fireEvent.change(screen.getByRole("textbox", { name: "MCP 分组名称" }), {
    target: { value: "浏览器" },
  });
  fireEvent.click(screen.getByRole("checkbox", { name: "选择 Scan" }));
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
    },
  });
  expect(methods()).not.toContain("activateGroup");
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
    codex: { groupId: "g1", entries: [entry], bindings: [binding] },
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

it("uses the same group-first navigation and toolbar flow as Skills", async () => {
  await open();
  expect(screen.getByRole("tab", { name: /^分组/ })).toHaveAttribute(
    "data-state",
    "active",
  );
  expect(
    screen.getByRole("button", { name: "新建 MCP 分组" }),
  ).toBeInTheDocument();
  fireEvent.mouseDown(screen.getByRole("tab", { name: /^已配置 MCP/ }), {
    button: 0,
    ctrlKey: false,
  });
  expect(screen.getAllByRole("button", { name: "添加 MCP" })).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "添加 MCP" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
  fireEvent.mouseDown(screen.getByRole("tab", { name: /^未托管 MCP/ }), {
    button: 0,
    ctrlKey: false,
  });
  expect(
    screen.getByRole("textbox", { name: "搜索已配置 MCP…" }),
  ).toBeInTheDocument();
  expect(screen.queryByRole("tab", { name: "Skill" })).not.toBeInTheDocument();
  expect(methods()).not.toContain("saveEntry");
  expect(methods()).not.toContain("activateGroup");
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
