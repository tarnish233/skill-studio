import {
  act,
  cleanup,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";
import { useQueryClient } from "@tanstack/react-query";
import App from "@/App";
import { McpPage } from "@/pages/McpPage";
import { setTarget } from "@/lib/api/transport";
import { queryKeys } from "@/lib/queryKeys";
import { renderWithProviders } from "./utils/render";
import {
  calls,
  handlers,
  defaultSettings,
  codexAgent,
  skillOnlyAgents,
} from "./mocks/tauri";

const inventory = {
  servers: [
    { id: "shared", name: "Shared MCP", agents: ["codex", "opencode"] },
    { id: "pi", name: "Pi tools", agents: ["pi"] },
    { id: "private", name: "Open tools", agents: ["opencode"] },
  ],
  warnings: [],
};
beforeEach(() => {
  setTarget({ id: "server-a", name: "A", connected: true });
  handlers.set("remote_request", ({ method }) => {
    if (method === "scan_mcp") return inventory;
    if (method === "get_settings") return defaultSettings();
    if (method === "list_agents") return [codexAgent, ...skillOnlyAgents];
    if (method === "get_config")
      return { activeGroups: {}, settings: defaultSettings() };
    return [];
  });
});
afterEach(() => {
  cleanup();
  setTarget({ id: "local", name: "本机", connected: true });
});

it("renders remote MCP names and Agent owners as inert cards with no management actions", async () => {
  renderWithProviders(<McpPage />);
  const name = await screen.findByText("Shared MCP");
  const card = name.parentElement!;
  expect(within(card).getByText("Codex")).toBeVisible();
  expect(within(card).getByText("OpenCode")).toBeVisible();
  expect(screen.getByText("已配置 3 个 MCP")).toBeVisible();
  expect(screen.getByText("只读")).toBeVisible();
  const add = screen.getByRole("button", { name: "添加 MCP" });
  expect(add).toBeVisible();
  expect(add).toHaveAttribute("aria-disabled", "true");
  expect(screen.queryAllByRole("button")).toHaveLength(1);
  expect(screen.queryAllByRole("link")).toHaveLength(0);
  fireEvent.click(add);
  fireEvent.click(name);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  fireEvent.change(screen.getByPlaceholderText("搜索 MCP 或 Agent…"), {
    target: { value: "Pi" },
  });
  expect(screen.getByText("Pi tools")).toBeVisible();
  expect(screen.queryByText("Shared MCP")).not.toBeInTheDocument();
  expect(calls.filter((call) => call.command === "mcp_request")).toHaveLength(
    0,
  );
  expect(
    calls
      .filter((call) => call.command === "remote_request")
      .map((call) => (call.args as { method: string }).method),
  ).toEqual(expect.arrayContaining(["scan_mcp", "get_settings"]));
  expect(
    calls
      .filter((call) => call.command === "remote_request")
      .every((call) =>
        ["scan_mcp", "get_settings"].includes(
          (call.args as { method: string }).method,
        ),
      ),
  ).toBe(true);
});

function HideOpenCode() {
  const client = useQueryClient();
  return (
    <button
      onClick={() =>
        client.setQueryData(queryKeys.settings, {
          ...defaultSettings(),
          disabledAgents: ["opencode"],
        })
      }
    >
      隐藏 OpenCode
    </button>
  );
}
it("hides disabled Agent badges and private cards while retaining shared MCP", async () => {
  renderWithProviders(
    <>
      <HideOpenCode />
      <McpPage />
    </>,
  );
  await screen.findByText("Open tools");
  fireEvent.click(screen.getByRole("button", { name: "隐藏 OpenCode" }));
  await waitFor(() =>
    expect(screen.queryByText("Open tools")).not.toBeInTheDocument(),
  );
  expect(screen.queryByText("OpenCode")).not.toBeInTheDocument();
  expect(screen.getByText("Shared MCP")).toBeVisible();
  expect(screen.getByText("已配置 2 个 MCP")).toBeVisible();
});

it("keeps late remote responses scoped to their original server and restores local controls", async () => {
  let resolveA!: (value: unknown) => void;
  handlers.set("remote_request", ({ serverId, method }) => {
    if (method === "get_settings") return defaultSettings();
    if (method === "scan_mcp")
      return serverId === "server-a"
        ? new Promise((resolve) => {
            resolveA = resolve;
          })
        : {
            servers: [{ id: "b", name: "B MCP", agents: ["codex"] }],
            warnings: [],
          };
    return [];
  });
  renderWithProviders(<McpPage />);
  await waitFor(() => expect(resolveA).toBeDefined());
  act(() => setTarget({ id: "server-b", name: "B", connected: true }));
  expect(await screen.findByText("B MCP")).toBeVisible();
  await act(async () => resolveA(inventory));
  expect(screen.queryByText("Shared MCP")).not.toBeInTheDocument();
  expect(calls.filter((call) => call.command === "mcp_request")).toHaveLength(
    0,
  );
  act(() => setTarget({ id: "local", name: "本机", connected: true }));
  expect(await screen.findByRole("button", { name: "添加 MCP" })).toBeVisible();
  expect(screen.getByRole("button", { name: "添加 MCP" })).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
  expect(screen.queryByText("B MCP")).not.toBeInTheDocument();
});

it("shows an upgrade hint for old helpers instead of an empty successful inventory", async () => {
  handlers.set("remote_request", ({ method }) => {
    if (method === "get_settings") return defaultSettings();
    throw new Error("不支持的管理命令: scan_mcp");
  });
  renderWithProviders(<McpPage />);
  expect(await screen.findByRole("alert")).toHaveTextContent("更新组件");
  expect(screen.queryByText("未发现远程 MCP 配置")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "添加 MCP" })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
});

it("does not scan or fall back locally while disconnected", () => {
  setTarget({ id: "server-a", name: "A", connected: false });
  renderWithProviders(<McpPage />);
  expect(screen.getByRole("alert")).toHaveTextContent("服务器已断开");
  expect(screen.queryByText("未发现远程 MCP 配置")).not.toBeInTheDocument();
  expect(
    calls.some(
      (call) =>
        call.command === "mcp_request" ||
        (call.args as { method?: string })?.method === "scan_mcp",
    ),
  ).toBe(false);
});

it("keeps remote MCP navigation in place without allowing unsupported actions", async () => {
  localStorage.setItem("skill-studio-view", "groups");
  localStorage.setItem("skill-studio-resource", "mcp");
  renderWithProviders(<App />);
  await waitFor(() => expect(screen.getByText("Shared MCP")).toBeVisible());
  const nav = within(screen.getByRole("navigation", { name: "主导航" }));
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
    "MCP Hub",
  );
  const groups = nav.getByRole("button", { name: "分组" });
  const projects = nav.getByRole("button", { name: "项目" });
  for (const button of [groups, projects]) {
    expect(button).toBeVisible();
    expect(button).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(button);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
      "MCP Hub",
    );
    expect(screen.getByText("Shared MCP")).toBeVisible();
  }
  const add = screen.getByRole("button", { name: "添加 MCP" });
  expect(add).toBeVisible();
  expect(add).toHaveAttribute("aria-disabled", "true");
  fireEvent.click(add);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(
    calls.some(
      (call) =>
        call.command === "mcp_request" ||
        (call.args as { method?: string })?.method === "mcp_request",
    ),
  ).toBe(false);
  fireEvent.click(
    within(screen.getByRole("banner")).getByRole("button", { name: "Skill" }),
  );
  expect(await nav.findByRole("button", { name: "分组" })).toBe(groups);
  expect(nav.getByRole("button", { name: "项目" })).toBe(projects);
  expect(groups).not.toHaveAttribute("aria-disabled", "true");
  expect(projects).not.toHaveAttribute("aria-disabled", "true");
});
