import { screen, fireEvent, waitFor, cleanup } from "@testing-library/react";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { toast } from "sonner";
import App from "@/App";
import { McpPage } from "@/pages/McpPage";
import { calls, handlers, defaultSettings } from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";
import { setTarget } from "@/lib/api/transport";

const empty = {
  running: false,
  entries: [],
  servers: [],
  discovered: [],
  scanWarnings: [],
};
const methods = () =>
  calls
    .filter((c) => c.command === "mcp_request")
    .map((c) => (c.args as { method: string }).method);
beforeEach(() => {
  setTarget({ id: "local", name: "本机", connected: true });
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? empty
      : method === "gatewayCheck"
        ? { issue: null }
        : null,
  );
});
afterEach(() => {
  cleanup();
  setTarget({ id: "local", name: "本机", connected: true });
});
it("saves direct configuration to the Hub without installing to agents", async () => {
  renderWithProviders(<McpPage />);
  await screen.findByText("网关已关闭");
  fireEvent.click(screen.getByRole("button", { name: "添加 MCP" }));
  fireEvent.change(screen.getByLabelText("名称"), {
    target: { value: "Work" },
  });
  fireEvent.change(screen.getByLabelText("MCP 地址"), {
    target: { value: "https://example.com/mcp" },
  });
  fireEvent.click(screen.getByRole("button", { name: "添加到 Hub" }));
  await waitFor(() => expect(methods()).toContain("saveEntry"));
  const save = calls.find(
    (c) =>
      c.command === "mcp_request" &&
      (c.args as { method: string }).method === "saveEntry",
  );
  expect(save?.args).toMatchObject({
    params: {
      entry: {
        name: "Work",
        mode: "direct",
        definition: { type: "http", url: "https://example.com/mcp" },
      },
      agents: [],
      projectId: "",
      expectedEntry: null,
    },
  });
  expect(methods()).not.toContain("start");
  expect(methods()).not.toContain("login");
});
it("fills a local process in the same page without templates or JSON", async () => {
  renderWithProviders(<McpPage />);
  fireEvent.click(screen.getByRole("button", { name: "添加 MCP" }));
  expect(screen.queryByText("常用模板")).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("名称"), {
    target: { value: "Memory" },
  });
  fireEvent.click(screen.getByRole("radio", { name: /本地进程/ }));
  fireEvent.change(screen.getByLabelText("启动命令"), {
    target: { value: "npx" },
  });
  fireEvent.click(screen.getByRole("button", { name: "添加参数" }));
  fireEvent.change(screen.getByLabelText("参数 1"), {
    target: { value: " argument with spaces " },
  });
  fireEvent.click(screen.getByRole("button", { name: "添加到 Hub" }));
  await waitFor(() => expect(methods()).toContain("saveEntry"));
  expect(
    calls.find(
      (c) =>
        c.command === "mcp_request" &&
        (c.args as { method: string }).method === "saveEntry",
    )?.args,
  ).toMatchObject({
    params: {
      entry: {
        definition: {
          type: "stdio",
          command: "npx",
          args: [" argument with spaces "],
        },
      },
    },
  });
});
it("parses pasted configurations in the unified add flow and lets users choose a service", async () => {
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? empty
      : method === "parse"
        ? [
            { name: "first", definition: { type: "stdio", command: "one" } },
            {
              name: "second",
              definition: {
                type: "stdio",
                command: "two",
                startup_timeout_sec: 120,
              },
            },
          ]
        : null,
  );
  renderWithProviders(<McpPage />);
  fireEvent.click(screen.getByRole("button", { name: "添加 MCP" }));
  fireEvent.change(screen.getByLabelText("粘贴安装命令、网址或配置"), {
    target: { value: "[mcp_servers.first]\ncommand='one'" },
  });
  fireEvent.click(await screen.findByRole("button", { name: "second" }));
  expect(screen.getByLabelText("名称")).toHaveValue("second");
  fireEvent.click(screen.getByRole("button", { name: "添加到 Hub" }));
  await waitFor(() => expect(methods()).toContain("saveEntry"));
  expect(
    calls.find(
      (c) =>
        c.command === "mcp_request" &&
        (c.args as { method: string }).method === "saveEntry",
    )?.args,
  ).toMatchObject({
    params: { entry: { definition: { startup_timeout_sec: 120 } } },
  });
});
it("shows a Pi project override on the managed server instead of a second card", async () => {
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          ...empty,
          entries: [
            {
              id: "managed-id",
              name: "hf-mcp-server",
              mode: "gateway",
              definition: { type: "http", url: "https://huggingface.co/mcp" },
              oauth: false,
              clientId: null,
              scopes: [],
              bindings: [],
            },
          ],
          discovered: [
            {
              id: "found",
              name: "hf-mcp-server",
              server: null,
              managedId: "managed-id",
              issue: null,
              sources: [
                {
                  id: "source-pi",
                  agent: "pi",
                  path: "/Users/me/.pi/agent/mcp.json",
                  scope: "用户全局",
                  project: null,
                  key: "hf-mcp-server",
                  enabled: true,
                  gateway: true,
                  definition: {
                    command: "/app/studio",
                    args: ["--mcp-client", "managed-id", "/studio"],
                  },
                  projectOverrides: [
                    {
                      path: "/work/open_mole/.pi/mcp.json",
                      scope: "项目 · open_mole",
                      key: "hf-mcp-server",
                      enabled: false,
                    },
                  ],
                },
              ],
            },
          ],
        }
      : method === "gatewayCheck"
        ? { issue: null }
        : null,
  );
  renderWithProviders(<McpPage />);
  expect(await screen.findAllByText("hf-mcp-server")).toHaveLength(1);
  fireEvent.click(
    screen.getByRole("button", { name: "查看 hf-mcp-server 的来源" }),
  );
  const override = await screen.findByText(/项目 · open_mole/);
  expect(override).toHaveTextContent("已停用");
  expect(override).toHaveTextContent("项目级覆盖");
  expect(override).toHaveTextContent("open_mole/.pi/mcp.json");
});
it("allows managing discovered client extensions directly, but blocks incompatible gateway mode", async () => {
  const definition = { command: "node", startup_timeout_sec: 120 };
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          ...empty,
          discovered: [
            {
              id: "found",
              name: "Tools",
              server: null,
              managedId: null,
              issue: "暂不支持 startup_timeout_sec",
              sources: [
                {
                  id: "source-found",
                  agent: "codex",
                  path: "/tmp/config.toml",
                  scope: "用户全局",
                  project: null,
                  key: "Tools",
                  enabled: true,
                  gateway: false,
                  definition,
                },
              ],
            },
          ],
        }
      : method === "gatewayCheck"
        ? { issue: "暂不支持 startup_timeout_sec" }
        : null,
  );
  renderWithProviders(<McpPage />);
  await screen.findByText("Tools");
  fireEvent.click(screen.getByRole("button", { name: "查看 Tools 的来源" }));
  fireEvent.click(screen.getByRole("button", { name: "管理此 MCP" }));
  expect(screen.getByRole("button", { name: "保存到 Hub" })).toBeEnabled();
  fireEvent.click(screen.getByRole("radio", { name: /Studio 代理/ }));
  await screen.findByRole("alert");
  expect(screen.getByRole("button", { name: "保存到 Hub" })).toBeDisabled();
  fireEvent.click(screen.getByRole("radio", { name: /^Agent 直连/ }));
  fireEvent.click(screen.getByRole("button", { name: "保存到 Hub" }));
  await waitFor(() => expect(methods()).toContain("saveEntry"));
  expect(
    calls.find(
      (c) =>
        c.command === "mcp_request" &&
        (c.args as { method: string }).method === "saveEntry",
    )?.args,
  ).toMatchObject({
    params: {
      sources: [],
      entry: { mode: "direct", definition: { startup_timeout_sec: 120 } },
    },
  });
  expect(methods()).not.toContain("start");
});
it("saves OAuth gateway configuration without starting or logging in automatically", async () => {
  renderWithProviders(<McpPage />);
  fireEvent.click(screen.getByRole("button", { name: "添加 MCP" }));
  fireEvent.change(screen.getByLabelText("名称"), {
    target: { value: "OAuth" },
  });
  fireEvent.change(screen.getByLabelText("MCP 地址"), {
    target: { value: "https://example.com/mcp" },
  });
  fireEvent.click(screen.getByRole("radio", { name: /Studio 代理/ }));
  expect(screen.queryByLabelText("此服务需要网页登录")).not.toBeInTheDocument();
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "添加到 Hub" })).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole("button", { name: "添加到 Hub" }));
  await waitFor(() => expect(methods()).toContain("saveEntry"));
  expect(methods()).not.toContain("start");
  expect(methods()).not.toContain("login");
});
it("reads only the remote inventory while viewing a remote machine", async () => {
  setTarget({ id: "remote", name: "远程", connected: true });
  handlers.set("remote_request", ({ method }) =>
    method === "scan_mcp" ? { servers: [], warnings: [] } : defaultSettings(),
  );
  renderWithProviders(<McpPage />);
  expect(await screen.findByText("未发现远程 MCP 配置")).toBeVisible();
  expect(methods()).toEqual([]);
});
it("toggles the gateway directly from the list", async () => {
  renderWithProviders(<McpPage />);
  await screen.findByText("网关已关闭");
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "MCP 网关" })).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole("button", { name: "MCP 网关" }));
  await waitFor(() => expect(methods()).toContain("start"));
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("explains that SSE must be tested in the Agent without marking it failed", async () => {
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          ...empty,
          entries: [
            {
              id: "sse",
              name: "SSE service",
              mode: "direct",
              definition: { type: "sse", url: "https://example.com/sse" },
              bindings: [],
              oauth: false,
              clientId: null,
              scopes: [],
            },
          ],
        }
      : null,
  );
  renderWithProviders(<McpPage />);
  fireEvent.click(
    await screen.findByRole("button", { name: "查看 SSE service 的来源" }),
  );
  const test = screen.getByRole("button", { name: "测试连接" });
  expect(test).toBeDisabled();
  expect(screen.getByText(/SSE 服务请在 Agent 中测试连接/)).toBeVisible();
  fireEvent.click(test);
  expect(methods()).not.toContain("testDirect");
  expect(screen.queryByText("最近测试失败")).not.toBeInTheDocument();
});

it("uses the app subpage header and preserves Hub search when returning", async () => {
  localStorage.setItem("skill-studio-resource", "mcp");
  renderWithProviders(<App />);
  const search = await screen.findByRole("textbox", {
    name: "按名称或地址搜索 MCP…",
  });
  fireEvent.change(search, { target: { value: "work" } });
  fireEvent.click(screen.getByRole("button", { name: "添加 MCP" }));
  expect(
    screen.getByRole("heading", { name: "添加 MCP", level: 1 }),
  ).toBeInTheDocument();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.getByRole("navigation", { name: "主导航" })).toBeVisible();
  expect(screen.queryByText("网关已关闭")).not.toBeInTheDocument();
  expect(localStorage.getItem("skill-studio-view")).toBe("hub");
  fireEvent.click(screen.getByRole("button", { name: "返回" }));
  expect(screen.getByRole("heading", { name: "MCP Hub" })).toBeInTheDocument();
  expect(
    screen.getByRole("textbox", { name: "按名称或地址搜索 MCP…" }),
  ).toHaveValue("work");
});

it("adopts discovered MCP sources through the card action without adding new targets", async () => {
  const definition = { command: "tools" };
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          ...empty,
          discovered: [
            {
              id: "source-row",
              name: "Tools",
              managedId: null,
              server: null,
              issue: null,
              sources: [
                {
                  id: "s1",
                  agent: "claude",
                  path: "/tmp/.claude.json",
                  scope: "用户全局",
                  project: null,
                  key: "Tools",
                  enabled: true,
                  gateway: false,
                  definition,
                },
              ],
            },
          ],
        }
      : null,
  );
  renderWithProviders(<McpPage />);
  fireEvent.click(
    await screen.findByRole("button", { name: "托管 Tools 到 Hub" }),
  );
  expect(methods()).not.toContain("saveEntry");
  fireEvent.click(screen.getByRole("button", { name: "托管" }));
  await waitFor(() => expect(methods()).toContain("saveEntry"));
  expect(
    calls.find(
      (c) =>
        c.command === "mcp_request" &&
        (c.args as { method: string }).method === "saveEntry",
    )?.args,
  ).toMatchObject({
    params: {
      sources: [{ id: "s1", definition }],
      agents: [],
      expectedEntry: null,
    },
  });
});

const managedTools = {
  id: "managed",
  name: "Tools",
  mode: "direct",
  definition: { command: "tools" },
  oauth: false,
  clientId: null,
  scopes: [],
  bindings: [
    {
      id: "b1",
      agent: "claude",
      path: "/tmp/.claude.json",
      project: null,
      key: "Tools",
      original: { command: "tools" },
      installed: { command: "tools" },
    },
  ],
};

it.each([true, false])(
  "separates restore=%s from deletion on managed cards",
  async (restore) => {
    handlers.set("mcp_request", ({ method }) =>
      method === "list"
        ? { ...empty, entries: [managedTools] }
        : method === "removeEntry"
          ? { removed: 1, foreign: [] }
          : null,
    );
    renderWithProviders(<McpPage />);
    expect(await screen.findByText("托管中")).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", {
        name: restore ? "还原 Tools 到原位置" : "删除 Tools",
      }),
    );
    expect(methods()).not.toContain("removeEntry");
    fireEvent.click(
      screen.getByRole("button", { name: restore ? "还原" : "删除" }),
    );
    await waitFor(() => expect(methods()).toContain("removeEntry"));
    expect(
      calls.find(
        (c) =>
          c.command === "mcp_request" &&
          (c.args as { method: string }).method === "removeEntry",
      )?.args,
    ).toMatchObject({ params: { id: "managed", restore } });
  },
);

it("deletes a managed MCP another program rewrote, naming the connection it left alone", async () => {
  const warning = vi.spyOn(toast, "warning");
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? { ...empty, entries: [managedTools] }
      : method === "removeEntry"
        ? {
            removed: 0,
            foreign: [
              {
                agent: "claude",
                path: "/tmp/.claude.json",
                project: null,
                key: "Tools",
              },
            ],
          }
        : null,
  );
  renderWithProviders(<McpPage />);
  fireEvent.click(await screen.findByRole("button", { name: "删除 Tools" }));
  // 确认框说清楚：已被其他程序改写的接入不会被删
  expect(
    screen.getByText(/已被其他程序改写的接入原样保留/),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "删除" }));
  await waitFor(() => expect(warning).toHaveBeenCalledTimes(1));
  const message = String(warning.mock.calls[0][0]);
  expect(message).toContain("Claude Code（/tmp/.claude.json）里的同名配置");
  expect(message).toContain("已被其他程序改写");
  // 一处配置都没写：不能说「配置文件已备份」
  expect(message).toMatch(/^MCP 已从 Hub 删除。/);
  expect(message).not.toContain("已备份");
  warning.mockRestore();
});

it("deletes an unmanaged source only after confirmation, retaining its snapshot", async () => {
  const definition = { command: "tools" };
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          ...empty,
          discovered: [
            {
              id: "source-row",
              name: "Tools",
              managedId: null,
              server: null,
              issue: null,
              sources: [
                {
                  id: "s1",
                  agent: "codex",
                  path: "/tmp/config.toml",
                  scope: "用户全局",
                  project: null,
                  key: "Tools",
                  enabled: true,
                  gateway: false,
                  definition,
                },
              ],
            },
          ],
        }
      : null,
  );
  renderWithProviders(<McpPage />);
  fireEvent.click(await screen.findByRole("button", { name: "删除 Tools" }));
  expect(methods()).not.toContain("removeSources");
  fireEvent.click(screen.getByRole("button", { name: "删除" }));
  await waitFor(() => expect(methods()).toContain("removeSources"));
  expect(
    calls.find(
      (c) =>
        c.command === "mcp_request" &&
        (c.args as { method: string }).method === "removeSources",
    )?.args,
  ).toMatchObject({ params: { sources: [{ id: "s1", definition }] } });
});

it("reveals login only after the upstream requests authorization, then shows tools after login", async () => {
  let authenticated = false;
  const entry = {
    id: "auto-auth",
    name: "Auto Auth",
    mode: "gateway",
    oauth: false,
    definition: { type: "http", url: "https://example.com/mcp" },
    clientId: null,
    scopes: [],
    bindings: [],
  };
  handlers.set("mcp_request", ({ method }) => {
    if (method === "list") return { ...empty, running: true, entries: [entry] };
    if (method === "test")
      return authenticated ? [{ name: "echo" }] : { authRequired: true };
    if (method === "loginStatus")
      return { status: authenticated ? "complete" : "pending" };
    if (method === "login") {
      authenticated = true;
      return { url: "https://example.com/authorize", flowId: "flow-1" };
    }
    return null;
  });
  renderWithProviders(<McpPage />);
  fireEvent.click(
    await screen.findByRole("button", { name: "查看 Auto Auth 的来源" }),
  );
  expect(
    screen.queryByRole("button", { name: "登录授权" }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "测试连接" }));
  fireEvent.click(await screen.findByRole("button", { name: "登录授权" }));
  await waitFor(() => expect(methods()).toContain("login"));
  expect(
    await screen.findByText("echo", {}, { timeout: 3000 }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("dialog", { name: "完成浏览器授权" }),
  ).not.toBeInTheDocument();
});

it("shows authorization needed when an Agent connection detected it", async () => {
  const entry = {
    id: "agent-auth",
    name: "Agent Auth",
    mode: "gateway",
    oauth: false,
    definition: { type: "http", url: "https://example.com/mcp" },
    clientId: null,
    scopes: [],
    bindings: [],
  };
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          ...empty,
          running: true,
          entries: [entry],
          servers: [{ id: entry.id, authRequired: true }],
        }
      : null,
  );
  renderWithProviders(<McpPage />);
  expect(await screen.findByText("待授权")).toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", { name: "查看 Agent Auth 的来源" }),
  );
  expect(screen.getByRole("button", { name: "登录授权" })).toBeInTheDocument();
});

it("shows bundled services as inert cards outside MCP counts", async () => {
  handlers.set("get_settings", () => ({
    ...defaultSettings(),
    showCodexBuiltinMcp: true,
  }));
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          ...empty,
          builtins: [
            {
              name: "node_repl",
              agent: "codex",
              path: "/home/.codex/config.toml",
              scope: "用户全局",
            },
          ],
        }
      : null,
  );
  renderWithProviders(<McpPage />);
  const entry = await screen.findByText("node_repl");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "全部 0" })).toBeInTheDocument();
  fireEvent.click(entry);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.getByText("node_repl")).toBeVisible();
  expect(screen.getByText("只读")).toBeVisible();
  expect(
    screen.queryByRole("button", { name: "查看 node_repl" }),
  ).not.toBeInTheDocument();
  expect(methods().every((method) => method === "list")).toBe(true);
});

it("hides builtin cards by default", async () => {
  handlers.set("mcp_request", () => ({
    ...empty,
    builtins: [
      {
        name: "node_repl",
        agent: "codex",
        path: "/home/.codex/config.toml",
        scope: "用户全局",
      },
    ],
  }));
  renderWithProviders(<McpPage />);
  await screen.findByText("网关已关闭");
  expect(screen.queryByText("node_repl")).not.toBeInTheDocument();
  expect(await screen.findByText("MCP Hub 还没有发现服务")).toBeInTheDocument();
});
