import {
  cleanup,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";
import App from "@/App";
import { McpPage } from "@/pages/McpPage";
import { setTarget } from "@/lib/api/transport";
import { renderWithProviders } from "./utils/render";
import {
  calls,
  claudeAgent,
  codexAgent,
  defaultSettings,
  handlers,
  makeProject,
  skillOnlyAgents,
} from "./mocks/tauri";

const SERVER = "server-a";

/** 服务器上的 MCP：托管的直连服务，加一处扫描到的原有配置（请求头和本机一样传回桌面编辑） */
function serverStatus(over: Record<string, unknown> = {}) {
  return {
    running: false,
    entries: [
      {
        id: "server-tool",
        name: "Server tool",
        mode: "direct",
        definition: { type: "http", url: "https://tools.example/mcp" },
        oauth: false,
        clientId: null,
        scopes: [],
        bindings: [
          {
            id: "b1",
            agent: "codex",
            path: "/home/srv/.codex/config.toml",
            project: null,
            key: "server-tool",
            original: null,
            installed: { url: "https://tools.example/mcp" },
          },
        ],
      },
    ],
    servers: [],
    discovered: [
      {
        id: "shared",
        name: "Shared MCP",
        server: null,
        issue: null,
        managedId: null,
        sources: [
          {
            id: "src-1",
            project: null,
            agent: "codex",
            path: "/home/srv/.codex/config.toml",
            scope: "user",
            key: "shared",
            enabled: true,
            gateway: false,
            definition: {
              url: "https://shared.example/mcp",
              http_headers: { Authorization: "Bearer server-token" },
            },
          },
        ],
      },
    ],
    groups: [],
    activeGroups: {},
    projects: {},
    backups: [],
    scanWarnings: [],
    ...over,
  };
}

type RemoteMcpCall = {
  serverId: string;
  params: { method: string; params: Record<string, unknown> };
};
/** 经服务器连接发出的 MCP 请求 */
const remoteMcp = () =>
  calls
    .filter(
      (c) =>
        c.command === "remote_request" &&
        (c.args as { method: string }).method === "mcp_request",
    )
    .map((c) => c.args as RemoteMcpCall);
const remoteMethods = () => remoteMcp().map((c) => c.params.method);
const remoteParams = (method: string) =>
  remoteMcp().filter((c) => c.params.method === method)[0]?.params.params;
/** 发给本机桌面应用的 MCP 请求 */
const localMcp = () =>
  calls
    .filter((c) => c.command === "mcp_request")
    .map((c) => (c.args as { method: string }).method);

let serverMcp: (method: string) => unknown;
beforeEach(() => {
  setTarget({ id: SERVER, name: "A", connected: true });
  serverMcp = (method) =>
    method === "list"
      ? serverStatus()
      : method === "gatewayCheck"
        ? { issue: null }
        : null;
  handlers.set("remote_request", ({ method, params }) => {
    if (method === "mcp_request") return serverMcp(params.method);
    if (method === "get_settings") return defaultSettings();
    if (method === "list_agents")
      return [claudeAgent, codexAgent, ...skillOnlyAgents];
    if (method === "get_config")
      return { activeGroups: {}, settings: defaultSettings() };
    if (method === "list_projects")
      return [
        makeProject({
          id: "srv-project",
          name: "服务器项目",
          root: "/srv/work",
        }),
      ];
    return [];
  });
});
afterEach(() => {
  cleanup();
  setTarget({ id: "local", name: "本机", connected: true });
});

it("manages server MCP on the same Hub page through the server connection", async () => {
  renderWithProviders(<McpPage />);
  expect(await screen.findByText("Server tool")).toBeVisible();
  expect(screen.getByText("Shared MCP")).toBeVisible();
  // 与本机一样能托管、删除和添加
  expect(
    screen.getByRole("button", { name: "托管 Shared MCP 到 Hub" }),
  ).toBeEnabled();
  expect(
    screen.getByRole("button", { name: "删除 Server tool" }),
  ).toBeEnabled();
  expect(screen.getByRole("button", { name: "添加 MCP" })).not.toHaveAttribute(
    "aria-disabled",
  );
  // 网关留在原位，但服务器上不能开启，悬停写明原因
  const gateway = screen.getByRole("button", { name: "MCP 网关" });
  expect(gateway).toHaveAttribute("aria-disabled", "true");
  expect(gateway).toHaveAttribute(
    "title",
    expect.stringContaining("服务器上暂不支持 Studio 网关"),
  );
  fireEvent.click(gateway);
  expect(remoteMethods()).toContain("list");
  expect(remoteMethods()).not.toContain("start");
  expect(remoteMcp().every((c) => c.serverId === SERVER)).toBe(true);
  expect(localMcp()).toEqual([]);
});

it("adopts an existing server config and adds direct MCP without offering the gateway", async () => {
  renderWithProviders(<McpPage />);
  fireEvent.click(
    await screen.findByRole("button", { name: "托管 Shared MCP 到 Hub" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "托管" }));
  await waitFor(() => expect(remoteMethods()).toContain("saveEntry"));
  expect(remoteParams("saveEntry")).toMatchObject({
    entry: {
      name: "Shared MCP",
      mode: "direct",
      definition: {
        type: "http",
        url: "https://shared.example/mcp",
        headers: { Authorization: "Bearer server-token" },
      },
    },
    sources: [
      {
        id: "src-1",
        definition: {
          url: "https://shared.example/mcp",
          http_headers: { Authorization: "Bearer server-token" },
        },
      },
    ],
  });

  fireEvent.click(screen.getByRole("button", { name: "添加 MCP" }));
  const proxy = screen.getByRole("radio", { name: /^Studio 代理/ });
  expect(proxy).toBeDisabled();
  expect(screen.getByText(/服务器上暂不支持 Studio 网关/)).toBeVisible();
  expect(screen.getByRole("radio", { name: /^Agent 直连/ })).toBeChecked();
  fireEvent.change(screen.getByLabelText("名称"), {
    target: { value: "Docs" },
  });
  fireEvent.change(screen.getByLabelText("MCP 地址"), {
    target: { value: "https://docs.example/mcp" },
  });
  fireEvent.click(screen.getByRole("button", { name: "添加到 Hub" }));
  await waitFor(() =>
    expect(remoteMethods().filter((m) => m === "saveEntry")).toHaveLength(2),
  );
  expect(
    remoteMcp().filter((c) => c.params.method === "saveEntry")[1].params.params,
  ).toMatchObject({
    entry: {
      name: "Docs",
      mode: "direct",
      definition: { type: "http", url: "https://docs.example/mcp" },
    },
    agents: [],
  });
  expect(localMcp()).toEqual([]);
});

it("searches the official registry on the desktop and saves the choice to the server", async () => {
  handlers.set("mcp_request", ({ method }) =>
    method === "searchRegistry"
      ? {
          items: [
            {
              name: "io.example/docs",
              description: "文档检索",
              version: "1.0.0",
              definition: { type: "http", url: "https://registry.example/mcp" },
              source: "remote",
            },
          ],
          nextCursor: null,
        }
      : null,
  );
  renderWithProviders(<McpPage />);
  fireEvent.click(await screen.findByRole("button", { name: "添加 MCP" }));
  fireEvent.click(screen.getByRole("button", { name: "官方目录" }));
  fireEvent.change(screen.getByLabelText("搜索官方 MCP 目录"), {
    target: { value: "docs" },
  });
  fireEvent.click(screen.getByRole("button", { name: "搜索" }));
  fireEvent.click(await screen.findByRole("button", { name: "填入" }));
  fireEvent.click(screen.getByRole("button", { name: "添加到 Hub" }));
  await waitFor(() => expect(remoteMethods()).toContain("saveEntry"));
  expect(remoteParams("saveEntry")).toMatchObject({
    entry: {
      name: "io.example/docs",
      definition: { url: "https://registry.example/mcp" },
    },
  });
  // 服务器可能连不上外网：只有目录搜索留在桌面
  expect(localMcp()).toEqual(["searchRegistry"]);
  expect(remoteMethods()).not.toContain("searchRegistry");
});

it("tests a server MCP on the server and says where it was verified", async () => {
  serverMcp = (method) =>
    method === "list"
      ? serverStatus()
      : method === "testDirect"
        ? [{ name: "lookup", description: "查资料" }]
        : null;
  renderWithProviders(<McpPage />);
  fireEvent.click(
    await screen.findByRole("button", { name: "编辑 Server tool" }),
  );
  expect(screen.getByText(/测试在服务器上进行/)).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "测试连接" }));
  expect(await screen.findByText("连接成功 · 1 个工具")).toBeInTheDocument();
  expect(remoteParams("testDirect")).toEqual({ id: "server-tool" });
  expect(screen.getByText("服务器连接已验证")).toBeInTheDocument();
  expect(screen.queryByText("本机连接已验证")).toBeNull();
  expect(localMcp()).toEqual([]);
});

it("opens MCP groups and projects for a server from the same navigation", async () => {
  localStorage.setItem("skill-studio-view", "groups");
  localStorage.setItem("skill-studio-resource", "mcp");
  serverMcp = (method) =>
    method === "list"
      ? serverStatus({
          groups: [
            {
              id: "g1",
              name: "服务器分组",
              entryIds: ["server-tool"],
              sortOrder: 0,
            },
          ],
        })
      : null;
  renderWithProviders(<App />);
  expect(await screen.findByText("服务器分组")).toBeVisible();
  const nav = within(screen.getByRole("navigation", { name: "主导航" }));
  const groups = nav.getByRole("button", { name: "分组" });
  const projects = nav.getByRole("button", { name: "项目" });
  for (const button of [groups, projects])
    expect(button).not.toHaveAttribute("aria-disabled");
  fireEvent.click(projects);
  expect(await screen.findByText("服务器项目")).toBeVisible();
  expect(remoteMcp().every((c) => c.serverId === SERVER)).toBe(true);
  expect(localMcp()).toEqual([]);
});

it("does not fall back to the local MCP while the server is disconnected", async () => {
  setTarget({ id: SERVER, name: "A", connected: false });
  renderWithProviders(<McpPage />);
  expect(await screen.findByRole("alert")).toHaveTextContent("服务器已断开");
  expect(localMcp()).toEqual([]);
  expect(remoteMcp()).toEqual([]);
});
