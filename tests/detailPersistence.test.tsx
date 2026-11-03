import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { beforeEach, expect, it } from "vitest";
import { GroupsPage } from "@/pages/GroupsPage";
import { McpGroupsPage } from "@/pages/McpGroupsPage";
import { ProjectsPage } from "@/pages/ProjectsPage";
import { McpProjectsPage } from "@/pages/McpProjectsPage";
import { McpPage } from "@/pages/McpPage";
import { queryKeys } from "@/lib/queryKeys";
import { setTarget } from "@/lib/api/transport";
import type { ManagedMcp, ManagementStatus } from "@/lib/api/mcpManagement";
import {
  calls,
  claudeAgent,
  codexAgent,
  defaultSettings,
  handlers,
  makeGroup,
  makeProject,
  makeSkill,
} from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

const entry = (id: string, name: string): ManagedMcp => ({
  id,
  name,
  mode: "direct",
  definition: { type: "stdio", command: name.toLowerCase() },
  oauth: false,
  clientId: null,
  scopes: [],
  bindings: [],
});

const method = (call: (typeof calls)[number]) =>
  (call.args as { method?: string } | undefined)?.method;

beforeEach(() => setTarget({ id: "local", name: "本机", connected: true }));
function setup() {
  const data = {
    groups: [makeGroup({ id: "g", name: "Dev", skillIds: ["a"] })],
    projects: [makeProject({ id: "p", name: "webapp", skillIds: ["a"] })],
    status: {
      running: false,
      entries: [entry("m", "Work"), entry("extra", "Extra")],
      servers: [],
      discovered: [],
      scanWarnings: [],
      groups: [{ id: "g", name: "Dev", entryIds: ["m"], sortOrder: 0 }],
      activeGroups: {},
      projects: {
        p: {
          root: "/work/webapp",
          entryIds: ["m"],
          groupIds: [],
          claudeLocal: false,
          active: {},
        },
      },
    } as ManagementStatus,
  };
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("get_config", () => ({ activeGroups: {} }));
  handlers.set("list_groups", () => data.groups);
  handlers.set("list_projects", () => data.projects);
  handlers.set("scan_skills", () => [
    makeSkill({ id: "a", name: "Alpha" }),
    makeSkill({ id: "b", name: "Beta" }),
  ]);
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? data.status
      : method === "gatewayCheck"
        ? { issue: null }
        : null,
  );
  return data;
}
function renderPage(Page: typeof GroupsPage) {
  let client!: QueryClient;
  function Capture() {
    client = useQueryClient();
    return null;
  }
  renderWithProviders(
    <>
      <Capture />
      <Page />
    </>,
  );
  return () => client;
}

it.each([
  {
    kind: "skill-group",
    Page: GroupsPage,
    name: "Dev",
    key: queryKeys.groups,
    label: "分组名称",
  },
  {
    kind: "mcp-group",
    Page: McpGroupsPage,
    name: "Dev",
    key: queryKeys.mcpStatus,
    label: "分组名称",
  },
  {
    kind: "skill-project",
    Page: ProjectsPage,
    name: "webapp",
    key: queryKeys.projects,
    member: "Beta",
  },
  {
    kind: "mcp-project",
    Page: McpProjectsPage,
    name: "webapp",
    key: queryKeys.projects,
    member: "Extra",
  },
  {
    kind: "mcp-entry",
    Page: McpPage,
    name: "Work",
    key: queryKeys.mcpStatus,
    label: "名称",
  },
])(
  "$kind retains its editor and guard when its record is deleted elsewhere",
  async ({ kind, Page, name, key, label, member }) => {
    const data = setup();
    const client = renderPage(Page);
    fireEvent.click(
      await screen.findByRole("button", { name: `编辑 ${name}` }),
    );
    const field = label
      ? await screen.findByLabelText(label)
      : await screen.findByRole("checkbox", { name: member });
    if (label) fireEvent.change(field, { target: { value: "Local draft" } });
    else fireEvent.click(field);
    if (kind === "skill-group") data.groups = [];
    else if (kind === "mcp-group") data.status = { ...data.status, groups: [] };
    else if (kind.endsWith("project")) data.projects = [];
    else
      data.status = {
        ...data.status,
        entries: data.status.entries.filter((e) => e.id !== "m"),
      };
    await act(async () => {
      await client().invalidateQueries({ queryKey: key });
    });
    expect(await screen.findByRole("alert")).toHaveTextContent("已在别处删除");
    expect(
      label
        ? screen.getByLabelText(label)
        : screen.getByRole("checkbox", { name: member }),
    ).toBe(field);
    if (label) expect(field).toHaveValue("Local draft");
    else expect(field).toBeChecked();
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(
      await screen.findByRole("dialog", { name: "放弃未保存的修改？" }),
    ).toBeInTheDocument();
    expect(
      calls.some(
        (call) =>
          call.command === "save_group" ||
          call.command === "save_project" ||
          (call.command === "mcp_request" &&
            ["saveGroup", "saveProject", "saveEntry"].includes(
              method(call) ?? "",
            )),
      ),
    ).toBe(false);
  },
);

it("MCP project retains its draft when only its MCP record is deleted", async () => {
  const data = setup();
  const client = renderPage(McpProjectsPage);
  fireEvent.click(await screen.findByRole("button", { name: "编辑 webapp" }));
  const member = await screen.findByRole("checkbox", { name: "Extra" });
  fireEvent.click(member);
  data.status = { ...data.status, projects: {} };
  await act(async () => {
    await client().invalidateQueries({ queryKey: queryKeys.mcpStatus });
  });
  expect(await screen.findByRole("alert")).toHaveTextContent("已在别处删除");
  expect(screen.getByRole("checkbox", { name: "Extra" })).toBe(member);
  expect(member).toBeChecked();
  expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
});

it("MCP editing keeps the opening revision rather than the externally refreshed entry", async () => {
  const data = setup();
  const opened = JSON.parse(
    JSON.stringify(data.status.entries[0]),
  ) as ManagedMcp;
  const client = renderPage(McpPage);
  fireEvent.click(await screen.findByRole("button", { name: "编辑 Work" }));
  const field = await screen.findByLabelText("名称");
  fireEvent.change(field, { target: { value: "Mine" } });
  data.status = {
    ...data.status,
    entries: [
      {
        ...opened,
        name: "Theirs",
        definition: { type: "stdio", command: "external" },
      },
    ],
  };
  handlers.set("mcp_request", ({ method }) => {
    if (method === "list") return data.status;
    if (method === "saveEntry") throw new Error("MCP 已被其他操作修改");
    return null;
  });
  await act(async () => {
    await client().invalidateQueries({ queryKey: queryKeys.mcpStatus });
  });
  await screen.findByText("external"); // Wait for the live row, not merely the retained form state.
  expect(screen.getByLabelText("名称")).toBe(field);
  expect(field).toHaveValue("Mine");
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() =>
    expect(
      calls.find(
        (c) => c.command === "mcp_request" && method(c) === "saveEntry",
      )?.args,
    ).toMatchObject({ params: { expectedEntry: opened } }),
  );
  expect(screen.getByLabelText("名称")).toHaveValue("Mine");
  expect(await screen.findByText(/保存失败，修改已保留/)).toBeInTheDocument();
});

it("MCP adoption excludes opted-out sources without altering managed binding IDs", async () => {
  const data = setup();
  handlers.set("get_settings", () => ({
    ...defaultSettings(),
    disabledAgents: ["codex"],
  }));
  data.status = {
    ...data.status,
    entries: [],
    groups: [],
    projects: {},
    discovered: [
      {
        id: "native",
        name: "Native",
        managedId: null,
        server: null,
        issue: null,
        sources: ["claude", "codex"].map((agent) => ({
          id: `source-${agent}`,
          agent,
          key: "native",
          path: `/config/${agent}`,
          scope: "用户全局",
          project: null,
          enabled: true,
          gateway: false,
          definition: { type: "stdio", command: "native" },
        })),
      },
    ],
  };
  renderPage(McpPage);
  fireEvent.click(await screen.findByRole("button", { name: "编辑 Native" }));
  fireEvent.click(await screen.findByRole("button", { name: "托管到 Hub" }));
  fireEvent.click(await screen.findByRole("button", { name: "托管" }));
  await waitFor(() =>
    expect(
      calls.find(
        (c) => c.command === "mcp_request" && method(c) === "saveEntry",
      )?.args,
    ).toMatchObject({
      params: {
        bindingIds: [],
        sources: [
          {
            id: "source-claude",
            definition: { type: "stdio", command: "native" },
          },
        ],
      },
    }),
  );
});

it("a successful MCP save uses its committed bindings, not a later refresh, as the next revision", async () => {
  const data = setup();
  const binding = {
    id: "b",
    agent: "codex",
    path: "/config.toml",
    project: null,
    key: "work",
    original: null,
    installed: { command: "work" },
  };
  data.status.entries[0] = { ...data.status.entries[0], bindings: [binding] };
  let committed!: ManagedMcp;
  let saves = 0;
  handlers.set("mcp_request", ({ method, params }) => {
    if (method === "list") return data.status;
    if (method === "saveEntry") {
      if (++saves > 1) throw new Error("MCP 已被其他操作修改");
      const submitted = params.entry as ManagedMcp;
      committed = {
        ...submitted,
        bindings: [{ ...binding, installed: { command: "mine" } }],
      };
      // Another editor wins after this commit but before the refresh completes.
      data.status = {
        ...data.status,
        entries: [{ ...committed, name: "Theirs" }],
      };
      return committed;
    }
    return null;
  });
  renderPage(McpPage);
  fireEvent.click(await screen.findByRole("button", { name: "编辑 Work" }));
  fireEvent.change(await screen.findByLabelText("启动命令"), {
    target: { value: "mine" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await screen.findByText("mine");
  expect(await screen.findByRole("alert")).toHaveTextContent("已在别处修改");
  fireEvent.change(screen.getByLabelText("名称"), {
    target: { value: "Next draft" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() =>
    expect(
      calls
        .filter((c) => c.command === "mcp_request" && method(c) === "saveEntry")
        .at(-1)?.args,
    ).toMatchObject({ params: { expectedEntry: committed } }),
  );
  expect(screen.getByLabelText("名称")).toHaveValue("Next draft");
});

it.each([
  { kind: "skill", Page: GroupsPage },
  { kind: "mcp", Page: McpGroupsPage },
])(
  "new $kind groups keep the same editor after saving and use the saved revision",
  async ({ kind, Page }) => {
    const data = setup();
    data.groups = [];
    data.status = { ...data.status, groups: [] };
    handlers.set("save_group", (args) => {
      const group = makeGroup({
        id: args.groupId ?? "created",
        name: args.name,
        skillIds: args.skillIds,
      });
      data.groups = [group];
      return group;
    });
    handlers.set("mcp_request", ({ method, params }) => {
      if (method === "list") return data.status;
      if (method === "saveGroup")
        data.status = { ...data.status, groups: [params.group] };
      return null;
    });
    renderPage(Page);
    fireEvent.click(screen.getByRole("button", { name: "新建分组" }));
    const field = await screen.findByLabelText("分组名称");
    fireEvent.change(field, { target: { value: "First" } });
    const member = kind === "skill" ? "Alpha" : "Work";
    fireEvent.click(await screen.findByRole("checkbox", { name: member }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "保存" })).toBeDisabled(),
    );
    await waitFor(() => expect(field).toBeEnabled());
    expect(screen.getByLabelText("分组名称")).toBe(field);
    fireEvent.change(field, { target: { value: "Second" } });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "保存" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => {
      const writes = calls.filter((c) =>
        kind === "skill"
          ? c.command === "save_group"
          : c.command === "mcp_request" && method(c) === "saveGroup",
      );
      expect(writes).toHaveLength(2);
      const args = writes[1].args;
      if (kind === "skill")
        expect(args).toMatchObject({
          groupId: "created",
          expected: { name: "First", skillIds: ["a"] },
        });
      else
        expect(
          (args as { params: Record<string, unknown> })?.params,
        ).toMatchObject({
          group: { id: data.status.groups?.[0].id },
          expected: { name: "First", entryIds: ["m"] },
        });
    });
    expect(screen.getByLabelText("分组名称")).toBe(field);
  },
);
