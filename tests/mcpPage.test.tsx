import {
  screen,
  fireEvent,
  waitFor,
  cleanup,
  within,
} from "@testing-library/react";
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
it("hides URL credentials on MCP cards and detail headings without changing the editable value", async () => {
  const url =
    "https://user:sk_ui_password@example.com/mcp?token=sk_ui_query&sk_ui_bare";
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          ...empty,
          entries: [
            {
              id: "private-url",
              name: "Private URL",
              mode: "direct",
              definition: { type: "http", url },
              oauth: false,
              clientId: null,
              scopes: [],
              bindings: [],
            },
          ],
        }
      : null,
  );
  renderWithProviders(<McpPage />);
  await screen.findByText("Private URL");
  for (const secret of ["sk_ui_password", "sk_ui_query", "sk_ui_bare"]) {
    expect(screen.queryByText(new RegExp(secret))).not.toBeInTheDocument();
  }
  expect(
    screen.getByText(/https:\/\/<已隐藏>@example.com\/mcp/),
  ).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "编辑 Private URL" }));
  await screen.findByText(/https:\/\/<已隐藏>@example.com\/mcp/);
  expect(screen.getByLabelText("MCP 地址")).toHaveValue(url);
  for (const secret of ["sk_ui_password", "sk_ui_query", "sk_ui_bare"]) {
    expect(screen.queryByText(new RegExp(secret))).not.toBeInTheDocument();
  }
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
  // 点卡片在页内展开整页：接入位置里列出项目级覆盖
  fireEvent.click(screen.getByRole("button", { name: "编辑 hf-mcp-server" }));
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
  // 详情和配置在同一页：原有配置直接可以另存到 Hub，不用再点一次「管理」
  fireEvent.click(screen.getByRole("button", { name: "编辑 Tools" }));
  expect(
    await screen.findByRole("button", { name: "保存到 Hub" }),
  ).toBeEnabled();
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
it("sends MCP management to the server instead of this machine while viewing a server", async () => {
  setTarget({ id: "remote", name: "远程", connected: true });
  handlers.set("remote_request", ({ method, params }) =>
    method === "mcp_request" && params.method === "list"
      ? empty
      : defaultSettings(),
  );
  renderWithProviders(<McpPage />);
  expect(await screen.findByText("MCP Hub 还没有发现服务")).toBeVisible();
  expect(methods()).toEqual([]);
  expect(
    calls.some(
      (c) =>
        c.command === "remote_request" &&
        (c.args as { method: string }).method === "mcp_request",
    ),
  ).toBe(true);
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
    await screen.findByRole("button", { name: "编辑 SSE service" }),
  );
  const test = screen.getByRole("button", { name: "测试连接" });
  expect(test).toBeDisabled();
  expect(screen.getByText(/SSE 服务请在 Agent 中测试连接/)).toBeVisible();
  fireEvent.click(test);
  expect(methods()).not.toContain("testDirect");
  expect(screen.queryByText("最近测试失败")).not.toBeInTheDocument();
});

it("expands 添加 MCP inside the Hub page like a project detail, keeping the Hub search when returning", async () => {
  localStorage.setItem("skill-studio-resource", "mcp");
  renderWithProviders(<App />);
  const search = await screen.findByRole("textbox", {
    name: "按名称或地址搜索 MCP…",
  });
  fireEvent.change(search, { target: { value: "work" } });
  fireEvent.click(screen.getByRole("button", { name: "添加 MCP" }));
  expect(
    await screen.findByLabelText("粘贴安装命令、网址或配置"),
  ).toBeInTheDocument();
  // 不是单独的子页面：窗口标题仍是 MCP Hub
  expect(
    screen.getByRole("heading", { name: "MCP Hub", level: 1 }),
  ).toBeInTheDocument();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.getByRole("navigation", { name: "主导航" })).toBeVisible();
  expect(screen.queryByText("网关已关闭")).not.toBeInTheDocument();
  expect(localStorage.getItem("skill-studio-view")).toBe("hub");
  fireEvent.click(screen.getByRole("button", { name: "返回" }));
  expect(await screen.findByText("网关已关闭")).toBeInTheDocument();
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
    await screen.findByRole("button", { name: "编辑 Auto Auth" }),
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
  fireEvent.click(screen.getByRole("button", { name: "编辑 Agent Auth" }));
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
    screen.queryByRole("button", { name: "编辑 node_repl" }),
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

const work = {
  id: "work",
  name: "Work",
  mode: "direct" as const,
  definition: { type: "http", url: "https://example.com/mcp" },
  oauth: false,
  clientId: null,
  scopes: [],
  bindings: [],
};
const toolsDefinition = { type: "stdio", command: "tools" };
const toolsSource = {
  id: "found",
  name: "Tools",
  server: null,
  managedId: null,
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
      definition: toolsDefinition,
    },
  ],
};
/** Hub 记下保存过的服务，之后的读取能看到它，和真实后端一样 */
function hub(
  initial: { entries?: { id: string }[]; discovered?: unknown[] } = {},
  saveError?: string,
) {
  let entries = [...(initial.entries ?? [])];
  handlers.set("mcp_request", ({ method, params }) => {
    if (method === "list")
      return { ...empty, entries, discovered: initial.discovered ?? [] };
    if (method === "gatewayCheck") return { issue: null };
    if (method === "saveEntry") {
      if (saveError) throw new Error(saveError);
      const entry = (params as { entry: { id: string } }).entry;
      entries = [...entries.filter((e) => e.id !== entry.id), entry];
    }
    return null;
  });
}
const saves = () =>
  calls
    .filter(
      (c) =>
        c.command === "mcp_request" &&
        (c.args as { method: string }).method === "saveEntry",
    )
    .map((c) => (c.args as { params: Record<string, unknown> }).params);

it("点卡片在页内展开整页：改了才能保存，保存后留在这一页；有修改时先不能测试连接", async () => {
  hub({ entries: [work] });
  const success = vi.spyOn(toast, "success");
  renderWithProviders(<McpPage />);
  fireEvent.click(await screen.findByRole("button", { name: "编辑 Work" }));
  // 卡片上不再有「查看来源」和铅笔：详情和配置在同一页
  expect(
    screen.queryByRole("button", { name: "查看 Work 的来源" }),
  ).not.toBeInTheDocument();
  expect(
    await screen.findByRole("heading", { name: "接入位置" }),
  ).toBeInTheDocument();
  expect(screen.getByText(/还没有接入/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
  fireEvent.change(screen.getByLabelText("名称"), {
    target: { value: "Work 2" },
  });
  expect(screen.getByRole("button", { name: "测试连接" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(success).toHaveBeenCalledWith("已保存到 MCP Hub"));
  expect(saves()[0]).toMatchObject({
    entry: { id: "work", name: "Work 2" },
    expectedEntry: { id: "work", name: "Work" },
  });
  expect(screen.getByLabelText("名称")).toHaveValue("Work 2");
  expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "测试连接" })).toBeEnabled();
  success.mockRestore();
});

it("保存失败时原因写在标题栏下方，修改保留；这时返回要先确认", async () => {
  hub({ entries: [work] }, "配置已被其他程序修改");
  renderWithProviders(<McpPage />);
  fireEvent.click(await screen.findByRole("button", { name: "编辑 Work" }));
  fireEvent.change(await screen.findByLabelText("名称"), {
    target: { value: "Work 2" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "保存失败，修改已保留：配置已被其他程序修改",
  );
  expect(screen.getByLabelText("名称")).toHaveValue("Work 2");
  expect(screen.getByRole("button", { name: "保存" })).toBeEnabled();
  fireEvent.click(screen.getByRole("button", { name: "返回" }));
  expect(
    await screen.findByRole("dialog", { name: "放弃未保存的修改？" }),
  ).toBeInTheDocument();
});

it("原有配置另存到 Hub 后转到新服务的整页，原有配置不动", async () => {
  hub({ discovered: [toolsSource] });
  renderWithProviders(<McpPage />);
  fireEvent.click(await screen.findByRole("button", { name: "编辑 Tools" }));
  expect(
    await screen.findByRole("button", { name: "托管到 Hub" }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "保存到 Hub" }));
  await waitFor(() => expect(saves()).toHaveLength(1));
  expect(saves()[0]).toMatchObject({
    expectedEntry: null,
    sources: [],
    entry: { name: "Tools", definition: toolsDefinition },
  });
  expect(await screen.findByText("托管中")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "保存" })).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "托管到 Hub" }),
  ).not.toBeInTheDocument();
});

it("原有配置在整页里托管到 Hub：确认后转到托管后的这一项", async () => {
  hub({ discovered: [toolsSource] });
  renderWithProviders(<McpPage />);
  fireEvent.click(await screen.findByRole("button", { name: "编辑 Tools" }));
  fireEvent.click(await screen.findByRole("button", { name: "托管到 Hub" }));
  const dialog = await screen.findByRole("dialog", { name: "托管到 Hub" });
  fireEvent.click(within(dialog).getByRole("button", { name: "托管" }));
  await waitFor(() => expect(saves()).toHaveLength(1));
  expect(saves()[0]).toMatchObject({
    sources: [{ id: "s1", definition: toolsDefinition }],
  });
  expect(await screen.findByText("托管中")).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "托管到 Hub" }),
  ).not.toBeInTheDocument();
});

it("添加 MCP 后转到新服务的整页，可以接着测试连接", async () => {
  hub();
  const success = vi.spyOn(toast, "success");
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
  await waitFor(() => expect(success).toHaveBeenCalledWith("已添加到 MCP Hub"));
  expect(await screen.findByRole("button", { name: "测试连接" })).toBeEnabled();
  expect(screen.getByLabelText("名称")).toHaveValue("Work");
  success.mockRestore();
});

it("添加 MCP 填好后点返回：先确认放弃，不会顺手提交", async () => {
  hub();
  renderWithProviders(<McpPage />);
  await screen.findByText("网关已关闭");
  fireEvent.click(screen.getByRole("button", { name: "添加 MCP" }));
  fireEvent.change(screen.getByLabelText("名称"), {
    target: { value: "Work" },
  });
  fireEvent.change(screen.getByLabelText("MCP 地址"), {
    target: { value: "https://example.com/mcp" },
  });
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "添加到 Hub" })).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole("button", { name: "返回" }));
  expect(
    await screen.findByRole("dialog", { name: "放弃未保存的修改？" }),
  ).toBeInTheDocument();
  expect(saves()).toHaveLength(0);
});
