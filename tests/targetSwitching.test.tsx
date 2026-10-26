import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import App from "@/App";
import { NavigationGuard } from "@/components/common/NavigationGuard";
import { TargetProvider } from "@/components/targets/TargetProvider";
import { getTarget, setTarget, ServerProfile } from "@/lib/api/transport";
import { queryClient } from "@/lib/query/queryClient";
import { renderWithProviders } from "./utils/render";
import { calls, handlers, makeProject, defaultSettings } from "./mocks/tauri";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onCloseRequested: async () => () => {},
    destroy: async () => {},
  }),
}));
// Exercise the real target provider and pages without jsdom's slow floating-ui layout.
vi.mock("@/components/ui/dropdown-menu", async () => {
  const React = await import("react");
  const Menu = React.createContext({ open: false, toggle: () => {} });
  return {
    DropdownMenu: ({ children }: { children: React.ReactNode }) => {
      const [open, setOpen] = React.useState(false);
      return (
        <Menu.Provider value={{ open, toggle: () => setOpen((v) => !v) }}>
          {children}
        </Menu.Provider>
      );
    },
    DropdownMenuTrigger: ({ children }: { children: React.ReactElement }) => {
      const menu = React.useContext(Menu);
      return React.cloneElement(children, { onClick: menu.toggle });
    },
    DropdownMenuContent: ({ children }: { children: React.ReactNode }) =>
      React.useContext(Menu).open ? <div role="menu">{children}</div> : null,
    DropdownMenuItem: ({
      children,
      onSelect,
    }: {
      children: React.ReactNode;
      onSelect: () => void;
    }) => (
      <button role="menuitem" onClick={onSelect}>
        {children}
      </button>
    ),
    DropdownMenuSeparator: () => <hr />,
  };
});
const profile: ServerProfile = {
  id: "node2",
  name: "node2",
  host: "192.168.1.2",
  user: null,
  port: null,
  identityFile: null,
  jumpHost: null,
  passwordAuth: false,
  helperBinary: null,
};

beforeEach(() => {
  queryClient.clear();
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    value: {},
    configurable: true,
  });
  setTarget({ id: "local", name: "本机", connected: true });
  localStorage.setItem("skill-studio-view", "projects");
  handlers.set("list_servers", () => [profile]);
  handlers.set("connect_server", () => ({ protocolVersion: 1 }));
  handlers.set("list_projects", () => [
    makeProject({ id: "local-project", name: "本机项目", root: "/local/work" }),
  ]);
  handlers.set("get_config", () => ({
    activeGroups: {},
    settings: defaultSettings(),
  }));
  handlers.set("remote_request", ({ method }) => {
    if (method === "list_projects")
      return [
        makeProject({
          id: "remote-project",
          name: "远程项目",
          root: "/remote/work",
        }),
      ];
    if (method === "get_settings") return defaultSettings();
    if (method === "get_config")
      return { activeGroups: {}, settings: defaultSettings() };
    return [];
  });
});
afterEach(() => {
  cleanup();
  queryClient.clear();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  setTarget({ id: "local", name: "本机", connected: true });
});

it("switches the complete project workspace without leaking local cache and returns to local", async () => {
  const user = userEvent.setup();
  renderWithProviders(
    <NavigationGuard>
      <TargetProvider>
        <App />
      </TargetProvider>
    </NavigationGuard>,
  );
  expect(await screen.findByText("本机项目")).toBeInTheDocument();
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "本机" })).not.toBeDisabled(),
  );
  await user.click(screen.getByRole("button", { name: "本机" }));
  await user.click(await screen.findByRole("menuitem", { name: /node2/ }));
  expect(await screen.findByText("远程项目")).toBeInTheDocument();
  expect(screen.queryByText("本机项目")).not.toBeInTheDocument();
  expect(getTarget().id).toBe("node2");
  expect(
    calls.some(
      (c) =>
        c.command === "remote_request" &&
        (c.args as { method: string }).method === "list_projects",
    ),
  ).toBe(true);
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "node2" })).not.toBeDisabled(),
  );
  await user.click(screen.getByRole("button", { name: "node2" }));
  await user.click(await screen.findByRole("menuitem", { name: "本机" }));
  expect(await screen.findByText("本机项目")).toBeInTheDocument();
  expect(screen.queryByText("远程项目")).not.toBeInTheDocument();
});

it("keeps the existing target if connecting fails", async () => {
  handlers.set("connect_server", () => {
    throw "无法连接";
  });
  const user = userEvent.setup();
  renderWithProviders(
    <NavigationGuard>
      <TargetProvider>
        <App />
      </TargetProvider>
    </NavigationGuard>,
  );
  await screen.findByText("本机项目");
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "本机" })).not.toBeDisabled(),
  );
  await user.click(screen.getByRole("button", { name: "本机" }));
  await user.click(await screen.findByRole("menuitem", { name: /node2/ }));
  expect(await screen.findByRole("alert")).toHaveTextContent("无法连接");
  expect(getTarget().id).toBe("local");
  expect(screen.getByText("本机项目")).toBeInTheDocument();
});

it("adds a server by selecting its SSH config alias without copying connection options", async () => {
  handlers.set("list_ssh_hosts", () => ({
    hosts: ["build-node", "prod-node"],
    configPath: "/home/test/.ssh/config",
  }));
  const user = userEvent.setup();
  renderWithProviders(
    <NavigationGuard>
      <TargetProvider>
        <App />
      </TargetProvider>
    </NavigationGuard>,
  );
  await screen.findByText("本机项目");
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "本机" })).not.toBeDisabled(),
  );
  await user.click(screen.getByRole("button", { name: "本机" }));
  await user.click(screen.getByRole("menuitem", { name: "添加服务器…" }));
  expect(
    await screen.findByRole("radio", { name: "build-node" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByLabelText("用户名（留空沿用 SSH 配置）"),
  ).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "添加并连接" })).toBeDisabled();
  await user.type(
    screen.getByRole("textbox", { name: "搜索 SSH Host" }),
    "build",
  );
  expect(
    screen.queryByRole("radio", { name: "prod-node" }),
  ).not.toBeInTheDocument();
  await user.click(screen.getByRole("radio", { name: "build-node" }));
  await user.click(screen.getByRole("button", { name: "添加并连接" }));
  await waitFor(() => expect(getTarget().name).toBe("build-node"));
  const saved = calls.find((c) => c.command === "save_servers")?.args as {
    servers: ServerProfile[];
  };
  expect(saved.servers.find((s) => s.host === "build-node")).toMatchObject({
    name: "build-node",
    user: null,
    port: null,
    identityFile: null,
    jumpHost: null,
    passwordAuth: false,
  });
});

it("switches the MCP Hub between this machine and a server without mixing their services", async () => {
  localStorage.setItem("skill-studio-view", "hub");
  localStorage.setItem("skill-studio-resource", "mcp");
  const status = (name: string) => ({
    running: false,
    servers: [],
    discovered: [],
    scanWarnings: [],
    entries: [
      {
        id: name,
        name,
        mode: "direct",
        definition: { type: "http", url: `https://${name}.example/mcp` },
        oauth: false,
        clientId: null,
        scopes: [],
        bindings: [],
      },
    ],
  });
  handlers.set("mcp_request", ({ method }) =>
    method === "list" ? status("local-mcp") : null,
  );
  const remote = handlers.get("remote_request")!;
  handlers.set("remote_request", (args) =>
    args.method === "mcp_request"
      ? args.params.method === "list"
        ? status("server-mcp")
        : null
      : remote(args),
  );
  const user = userEvent.setup();
  renderWithProviders(
    <NavigationGuard>
      <TargetProvider>
        <App />
      </TargetProvider>
    </NavigationGuard>,
  );
  expect(await screen.findByText("local-mcp")).toBeInTheDocument();
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "本机" })).not.toBeDisabled(),
  );
  await user.click(screen.getByRole("button", { name: "本机" }));
  await user.click(await screen.findByRole("menuitem", { name: /node2/ }));
  expect(await screen.findByText("server-mcp")).toBeInTheDocument();
  expect(screen.queryByText("local-mcp")).not.toBeInTheDocument();
  // 网关只在本机：服务器上留在原位但不可用
  expect(screen.getByRole("button", { name: "MCP 网关" })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "node2" })).not.toBeDisabled(),
  );
  await user.click(screen.getByRole("button", { name: "node2" }));
  await user.click(await screen.findByRole("menuitem", { name: "本机" }));
  expect(await screen.findByText("local-mcp")).toBeInTheDocument();
  expect(screen.queryByText("server-mcp")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "MCP 网关" })).not.toHaveAttribute(
    "aria-disabled",
  );
});
