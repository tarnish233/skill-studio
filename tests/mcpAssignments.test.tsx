import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, it } from "vitest";
import { McpAssignments } from "@/components/mcp/McpAssignments";
import { renderWithProviders } from "./utils/render";
import { calls, handlers } from "./mocks/tauri";
import { setTarget } from "@/lib/api/transport";
import type { ManagedMcp, ManagementStatus } from "@/lib/api/mcpManagement";
const project = { id: "p1", name: "Work", root: "/work" };
let entry: ManagedMcp;
let status: ManagementStatus;
const saved = () =>
  (
    calls.find(
      (c) =>
        c.command === "mcp_request" &&
        (c.args as { method: string })?.method === "saveEntry",
    )?.args as { params: unknown } | undefined
  )?.params;
beforeEach(() => {
  setTarget({ id: "local", name: "本机", connected: true });
  entry = {
    id: "one",
    name: "Tools",
    mode: "direct",
    definition: { type: "stdio", command: "tools" },
    oauth: false,
    clientId: null,
    scopes: [],
    bindings: [],
  };
  status = {
    running: false,
    entries: [entry],
    servers: [],
    discovered: [],
    scanWarnings: [],
  };
  handlers.set("list_projects", () => [project]);
  handlers.set("mcp_request", ({ method }) =>
    method === "list" ? status : null,
  );
});
async function open() {
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "添加 MCP" })).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole("button", { name: "添加 MCP" }));
  fireEvent.change(screen.getByLabelText("选择 Hub MCP"), {
    target: { value: "one" },
  });
}
it("adds a Claude local scope from the project and restricts Codex to project configuration", async () => {
  renderWithProviders(<McpAssignments project={project} />);
  await open();
  fireEvent.change(screen.getByLabelText("MCP 保存位置"), {
    target: { value: "local" },
  });
  fireEvent.change(screen.getByLabelText("MCP Agent"), {
    target: { value: "codex" },
  });
  expect(screen.getByLabelText("MCP 保存位置")).toHaveValue("project");
  expect(
    screen.queryByRole("option", { name: /项目本地/ }),
  ).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("MCP Agent"), {
    target: { value: "claude" },
  });
  fireEvent.change(screen.getByLabelText("MCP 保存位置"), {
    target: { value: "local" },
  });
  fireEvent.click(screen.getByRole("button", { name: "添加并启用" }));
  await waitFor(() =>
    expect(saved()).toMatchObject({
      agents: ["claude"],
      projectId: "p1",
      scope: "local",
    }),
  );
});
it("removes only the selected project binding while preserving global and other project bindings", async () => {
  entry.bindings = [
    {
      id: "global",
      agent: "claude",
      path: "/home/.claude.json",
      project: null,
      key: "Tools",
      original: null,
      installed: entry.definition,
    },
    {
      id: "local",
      agent: "claude",
      path: "/home/.claude.json",
      project: "/work",
      key: "Tools",
      original: null,
      installed: entry.definition,
    },
    {
      id: "other",
      agent: "codex",
      path: "/other/.codex/config.toml",
      project: null,
      key: "Tools",
      original: null,
      installed: entry.definition,
    },
  ];
  renderWithProviders(<McpAssignments project={project} />);
  fireEvent.click(
    await screen.findByRole("button", { name: "移除 Tools 的 claude 接入" }),
  );
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "移除",
    }),
  );
  await waitFor(() =>
    expect(saved()).toMatchObject({
      bindingIds: ["global", "other"],
      agents: [],
      projectId: "",
      entry,
    }),
  );
});
it("keeps an installation error visible and allows retry", async () => {
  handlers.set("mcp_request", ({ method }) => {
    if (method === "list") return status;
    throw new Error("配置已被修改");
  });
  renderWithProviders(<McpAssignments project={project} />);
  await open();
  fireEvent.click(screen.getByRole("button", { name: "添加并启用" }));
  await waitFor(() =>
    expect(
      within(screen.getByRole("dialog")).getByRole("alert"),
    ).toHaveTextContent("配置已被修改"),
  );
  expect(screen.getByRole("button", { name: "添加并启用" })).toBeEnabled();
});
it("does not expose local configurations on remote targets", () => {
  setTarget({ id: "remote", name: "远程", connected: true });
  renderWithProviders(<McpAssignments project={project} />);
  expect(screen.getByText("MCP 管理当前仅支持本机。")).toBeVisible();
  expect(calls.some((c) => c.command === "mcp_request")).toBe(false);
});
it("adopts an existing source at its original location without creating another agent entry", async () => {
  const source = {
    id: "source",
    agent: "claude",
    path: "/work/.mcp.json",
    project: null,
    scope: "项目 · Work",
    key: "Tools",
    enabled: true,
    gateway: false,
    definition: entry.definition,
  };
  status.entries = [];
  status.discovered = [
    {
      id: "discovered",
      name: "Tools",
      sources: [source],
      managedId: null,
      server: null,
      issue: null,
    },
  ];
  renderWithProviders(<McpAssignments project={project} />);
  fireEvent.click(await screen.findByRole("button", { name: "纳入管理" }));
  await waitFor(() =>
    expect(saved()).toMatchObject({
      expectedEntry: null,
      sources: [{ id: "source", definition: entry.definition }],
      agents: [],
      projectId: "",
      scope: "user",
    }),
  );
});
it.each(["opencode", "pi", "grok"])(
  "adds %s project MCP and recognizes existing project bindings",
  async (agent) => {
    renderWithProviders(<McpAssignments project={project} />);
    await open();
    fireEvent.change(screen.getByLabelText("MCP Agent"), {
      target: { value: agent },
    });
    expect(
      screen.queryByRole("option", { name: /项目本地/ }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "添加并启用" }));
    await waitFor(() =>
      expect(saved()).toMatchObject({
        agents: [agent],
        scope: "project",
        projectId: "p1",
      }),
    );
  },
);

it.each([
  ["opencode", "/work/.opencode/opencode.jsonc"],
  ["pi", "/work/.pi/mcp.json"],
  ["grok", "/work/.grok/config.toml"],
])(
  "shows the %s name and project binding without allowing duplicate installs",
  async (agent, path) => {
    entry.bindings = [
      {
        id: "project",
        agent,
        path,
        project: null,
        key: "Tools",
        original: null,
        installed: entry.definition,
      },
    ];
    renderWithProviders(<McpAssignments project={project} />);
    expect(await screen.findByText(path)).toBeVisible();
    await open();
    fireEvent.change(screen.getByLabelText("MCP Agent"), {
      target: { value: agent },
    });
    expect(screen.getByRole("button", { name: "添加并启用" })).toBeDisabled();
  },
);
