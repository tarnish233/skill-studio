import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import App from "@/App";
import { PluginsPage } from "@/pages/PluginsPage";
import { LibraryPage } from "@/pages/LibraryPage";
import { McpPage } from "@/pages/McpPage";
import {
  pluginsApi,
  type PluginPackage,
  type PluginView,
} from "@/lib/api/plugins";
import { setTarget } from "@/lib/api/transport";
import { calls, defaultSettings, handlers, makeAgent } from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

function makePlugin(over: Partial<PluginView> = {}): PluginView {
  return {
    id: "plugin-1",
    agentId: "pi",
    nativeId: "npm:pi-tools",
    source: "npm:pi-tools",
    scope: "user",
    profile: null,
    configPath: "/home/u/.pi/agent/settings.json",
    available: true,
    configured: true,
    enabled: true,
    revision: "r1",
    actions: ["enable", "disable", "update", "uninstall"],
    readOnlyReason: null,
    package: {
      name: "pi-tools",
      version: "1.0.0",
      description: "Review code",
      format: "pi-package",
      root: "/home/u/.pi/agent/npm/node_modules/pi-tools",
      components: [
        {
          kind: "skill",
          name: "review",
          path: "/package/skills/review",
          issue: null,
        },
        { kind: "mcp", name: "code-index", path: null, issue: null },
      ],
      extensions: [],
      warnings: [],
      error: null,
    },
    ...over,
  };
}
function setup(managed = false, plugins = [makePlugin()]) {
  let settings = { ...defaultSettings(), managePlugins: managed };
  handlers.set("get_settings", () => settings);
  handlers.set("update_settings", ({ patch }) => {
    settings = { ...settings, ...patch };
    return settings;
  });
  handlers.set("list_agents", () => [
    makeAgent({ id: "pi", displayName: "Pi" }),
    makeAgent({ id: "codex", displayName: "Codex" }),
    makeAgent({ id: "claude-code", displayName: "Claude Code" }),
    makeAgent({ id: "opencode", displayName: "OpenCode" }),
  ]);
  handlers.set("scan_plugins", () => ({
    plugins,
    warnings: [],
    pending: false,
  }));
  handlers.set("probe_plugin_cli", () =>
    ["pi", "claude-code", "opencode"].map((agentId) => ({
      agentId,
      cliVersion: "1.6.0",
      install: true,
      reason: null,
    })),
  );
  return { settings: () => settings };
}

describe("Plugin Hub", () => {
  it("starts read-only and never invokes native management automatically", async () => {
    setup();
    renderWithProviders(<PluginsPage />);
    await screen.findByText("pi-tools");
    expect(
      screen.getByRole("button", { name: "停用 pi-tools" }),
    ).toBeDisabled();
    expect(screen.getByText("配置已启用")).toBeInTheDocument();
    expect(screen.queryByText("已生效")).not.toBeInTheDocument();
    expect(
      calls.some(
        (call) =>
          call.command === "plugin_operation" ||
          call.command === "probe_plugin_cli",
      ),
    ).toBe(false);
  });

  it("shows a single toggle button when the enabled state is unconfirmed", async () => {
    // Claude 插件未登记在 enabledPlugins 时视为默认启用，不能同时出现启用+停用两个电源图标。
    setup(false, [
      makePlugin({
        id: "cc-1",
        agentId: "claude-code",
        enabled: null,
        package: {
          ...makePlugin().package,
          name: "cc-review",
          format: "claude",
        },
      }),
    ]);
    renderWithProviders(<PluginsPage />);
    await screen.findByText("cc-review");
    expect(
      screen.getByRole("button", { name: "停用 cc-review" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "启用 cc-review" }),
    ).not.toBeInTheDocument();
  });

  it("management opt-in only changes the setting, not existing plugins", async () => {
    const state = setup();
    const user = userEvent.setup();
    renderWithProviders(<PluginsPage />);
    await user.click(
      await screen.findByRole("button", { name: "开启插件管理" }),
    );
    await waitFor(() => expect(state.settings().managePlugins).toBe(true));
    expect(
      calls.find((call) => call.command === "update_settings")?.args,
    ).toEqual({ patch: { managePlugins: true } });
    expect(
      calls.filter((call) => call.command === "plugin_operation"),
    ).toHaveLength(0);
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "停用 pi-tools" }),
      ).toBeEnabled(),
    );
  });

  it("confirms the exact native plugin and revision before changing personal configuration", async () => {
    setup(true);
    const user = userEvent.setup();
    handlers.set("plugin_operation", () => ({
      changed: true,
      pending: false,
      message: "已更新个人配置",
      warnings: [],
      backupPath: "/backups/1",
    }));
    renderWithProviders(<PluginsPage />);
    const button = await screen.findByRole("button", { name: "停用 pi-tools" });
    await waitFor(() => expect(button).toBeEnabled());
    await user.click(button);
    expect(
      calls.filter((call) => call.command === "plugin_operation"),
    ).toHaveLength(0);
    expect(await screen.findByText(/本机个人配置/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "确认执行" }));
    await waitFor(() =>
      expect(
        calls.find((call) => call.command === "plugin_operation")?.args,
      ).toEqual({
        input: {
          action: "disable",
          agentId: "pi",
          pluginId: "plugin-1",
          expectedRevision: "r1",
          confirmed: true,
        },
      }),
    );
  });

  it("cancelling confirmation never changes plugins", async () => {
    setup(true);
    const user = userEvent.setup();
    renderWithProviders(<PluginsPage />);
    const button = await screen.findByRole("button", { name: "移除 pi-tools" });
    await waitFor(() => expect(button).toBeEnabled());
    await user.click(button);
    await user.click(screen.getByRole("button", { name: "取消" }));
    expect(
      calls.filter((call) => call.command === "plugin_operation"),
    ).toHaveLength(0);
  });

  it("read-only agents have no native write controls", async () => {
    setup(true, [
      makePlugin({ agentId: "codex", actions: [], readOnlyReason: "首版只读" }),
    ]);
    renderWithProviders(<PluginsPage />);
    await screen.findByText("pi-tools");
    expect(
      screen.queryByRole("button", { name: "停用 pi-tools" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("只读")).toBeInTheDocument();
  });

  it("same-named plugins have distinct Agent and scope labels", async () => {
    setup(false, [
      makePlugin(),
      makePlugin({ id: "plugin-2", agentId: "codex", actions: [] }),
    ]);
    renderWithProviders(<PluginsPage />);
    expect(
      await screen.findByRole("button", {
        name: "查看插件 pi-tools · Pi · 个人配置",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "查看插件 pi-tools · Codex · 个人配置",
      }),
    ).toBeInTheDocument();
  });

  it("changing an inspected path discards an old asynchronous preview", async () => {
    setup(false);
    const user = userEvent.setup();
    let resolve!: (value: PluginPackage) => void;
    handlers.set(
      "inspect_plugin",
      () => new Promise<PluginPackage>((done) => (resolve = done)),
    );
    renderWithProviders(<PluginsPage />);
    await user.click(await screen.findByRole("button", { name: "添加插件" }));
    await user.click(screen.getByRole("tab", { name: "检查本地目录" }));
    const path = screen.getByLabelText("插件目录或 JS/TS 文件路径");
    await user.type(path, "/old");
    await user.click(screen.getByRole("button", { name: "只读检查" }));
    await user.clear(path);
    await user.type(path, "/new");
    await act(async () =>
      resolve({
        ...makePlugin().package,
        root: "/old",
        description: "OLD PREVIEW",
      }),
    );
    expect(screen.queryByText("OLD PREVIEW")).not.toBeInTheDocument();
  });

  it("search includes declared components without turning them into independent deployments", async () => {
    setup();
    renderWithProviders(<PluginsPage />);
    await screen.findByText("pi-tools");
    fireEvent.change(
      screen.getByRole("textbox", { name: "搜索插件名称、来源或组件" }),
      { target: { value: "code-index" } },
    );
    expect(screen.getByText("pi-tools")).toBeInTheDocument();
    fireEvent.change(
      screen.getByRole("textbox", { name: "搜索插件名称、来源或组件" }),
      { target: { value: "missing" } },
    );
    expect(screen.queryByText("pi-tools")).not.toBeInTheDocument();
  });

  it("a command receipt without observed state is shown as pending, not active", async () => {
    setup(true);
    const user = userEvent.setup();
    handlers.set("plugin_operation", () => ({
      changed: false,
      pending: true,
      message: "命令已退出，但状态未确认",
      warnings: [],
      backupPath: null,
    }));
    renderWithProviders(<PluginsPage />);
    const button = await screen.findByRole("button", { name: "更新 pi-tools" });
    await waitFor(() => expect(button).toBeEnabled());
    await user.click(button);
    await user.click(screen.getByRole("button", { name: "确认执行" }));
    expect(
      await screen.findByText(/命令已退出，但状态未确认/),
    ).toBeInTheDocument();
    expect(screen.queryByText("插件已生效")).not.toBeInTheDocument();
  });

  it("recovery only asks to finish metadata and never invokes an install or enable operation", async () => {
    setup(true);
    handlers.set("scan_plugins", () => ({
      plugins: [],
      warnings: [],
      pending: true,
    }));
    handlers.set("recover_plugin_write", () => ({
      changed: true,
      pending: false,
      message: "管理记录已完成，Agent 未改动",
      warnings: [],
      backupPath: null,
    }));
    const user = userEvent.setup();
    renderWithProviders(<PluginsPage />);
    await user.click(
      await screen.findByRole("button", {
        name: "核对待处理记录（不改 Agent）",
      }),
    );
    await waitFor(() =>
      expect(
        calls.find((call) => call.command === "recover_plugin_write")?.args,
      ).toEqual({ confirmed: true }),
    );
    expect(
      calls.filter((call) => call.command === "plugin_operation"),
    ).toHaveLength(0);
  });
});

it("Plugin navigation exposes only the delivered Hub and restores Skill navigation", async () => {
  setup();
  const user = userEvent.setup();
  renderWithProviders(<App />);
  const header = within(screen.getByRole("banner"));
  await user.click(await header.findByRole("button", { name: "Plugin" }));
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
    "Plugin Hub",
  );
  const nav = within(screen.getByRole("navigation", { name: "主导航" }));
  expect(nav.queryByRole("button", { name: "分组" })).not.toBeInTheDocument();
  expect(nav.queryByRole("button", { name: "项目" })).not.toBeInTheDocument();
  expect(localStorage.getItem("skill-studio-resource")).toBe("plugins");
  await user.click(header.getByRole("button", { name: "Skill" }));
  expect(nav.getByRole("button", { name: "分组" })).toBeInTheDocument();
  expect(nav.getByRole("button", { name: "项目" })).toBeInTheDocument();
});

it("plugin-owned skills are associated read-only even when independent Skill Hub is empty", async () => {
  setup();
  renderWithProviders(<LibraryPage onOpenPlugin={() => {}} />);
  const summary = await screen.findByText("插件提供的 Skills · 1");
  fireEvent.click(summary);
  expect(screen.getByText("review")).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "查看所属插件 pi-tools" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: /托管 review/ }),
  ).not.toBeInTheDocument();
  expect(
    calls.filter(
      (call) =>
        call.command === "register_skills" || call.command === "adopt_to_hub",
    ),
  ).toHaveLength(0);
});

it("disabled plugins do not contribute read-only component rows", async () => {
  const paused = makePlugin({
    id: "paused",
    agentId: "codex",
    enabled: false,
    package: {
      ...makePlugin().package,
      name: "paused-kit",
      components: [
        { kind: "skill", name: "paused-skill", path: null, issue: null },
      ],
    },
  });
  setup(false, [makePlugin(), paused]);
  renderWithProviders(<LibraryPage onOpenPlugin={() => {}} />);
  const summary = await screen.findByText("插件提供的 Skills · 1");
  fireEvent.click(summary);
  expect(screen.getByText("review")).toBeInTheDocument();
  expect(screen.queryByText("paused-skill")).not.toBeInTheDocument();
});

it("plugin-owned MCP entries are not copied into the MCP catalog or gateway", async () => {
  setup();
  renderWithProviders(<McpPage onOpenPlugin={() => {}} />);
  const summary = await screen.findByText("插件提供的 MCP · 1");
  fireEvent.click(summary);
  expect(screen.getByText("code-index")).toBeInTheDocument();
  expect(screen.getByText("插件组件 · 只读")).toBeInTheDocument();
  expect(
    calls.filter(
      (call) =>
        call.command === "mcp_request" &&
        (call.args as { method?: string })?.method !== "list",
    ),
  ).toHaveLength(0);
});

it("local plugin operations never fall through to a remote or silently use the local desktop", async () => {
  setup();
  setTarget({ id: "server", name: "Server", connected: true });
  try {
    await expect(pluginsApi.scan()).rejects.toThrow("仅支持本机");
    await expect(pluginsApi.recover(true)).rejects.toThrow("仅支持本机");
    expect(
      calls.filter((call) =>
        ["scan_plugins", "recover_plugin_write", "remote_request"].includes(
          call.command,
        ),
      ),
    ).toHaveLength(0);
  } finally {
    setTarget({ id: "local", name: "本机", connected: true });
  }
});
