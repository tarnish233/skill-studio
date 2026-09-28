import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, it } from "vitest";
import App from "@/App";
import { renderWithProviders } from "./utils/render";
import { claudeAgent, defaultSettings, handlers } from "./mocks/tauri";

it("keeps sidebar navigation mounted across Agent, settings and Hub pages", async () => {
  handlers.set("list_agents", () => [claudeAgent]);
  handlers.set("get_settings", () => ({
    ...defaultSettings(),
    manageMcp: true,
  }));
  renderWithProviders(<App />);
  const sidebar = screen.getByRole("complementary", { name: "侧边栏" });
  const nav = within(sidebar).getByRole("navigation", { name: "主导航" });
  const agent = await within(nav).findByRole("button", { name: "Claude Code" });
  fireEvent.click(within(nav).getByRole("button", { name: "MCP Hub" }));
  fireEvent.click(agent);
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
    "Claude Code",
  );
  expect(screen.getByPlaceholderText("搜索 MCP 分组…")).toBeVisible();
  fireEvent.click(within(sidebar).getByRole("button", { name: "设置" }));
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("设置");
  expect(within(nav).getByRole("button", { name: "Claude Code" })).toBe(agent);
  fireEvent.click(screen.getByRole("button", { name: "返回" }));
  expect(screen.getByPlaceholderText("搜索 MCP 分组…")).toBeVisible();
  fireEvent.click(within(nav).getByRole("button", { name: "Skill Hub" }));
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
    "Skill Hub",
  );
  expect(within(nav).getByRole("button", { name: "Claude Code" })).toBe(agent);
});

it("leaves the MCP editor when the currently selected Hub is clicked", async () => {
  localStorage.setItem("skill-studio-view", "mcp");
  handlers.set("get_settings", () => ({
    ...defaultSettings(),
    manageMcp: true,
  }));
  renderWithProviders(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "添加 MCP" }));
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
    "添加 MCP",
  );
  fireEvent.click(
    within(screen.getByRole("navigation")).getByRole("button", {
      name: "MCP Hub",
    }),
  );
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
    "MCP Hub",
  );
  expect(screen.getByPlaceholderText("按名称或地址搜索 MCP…")).toBeVisible();
});

it("exposes transparency only after native material setup succeeds", async () => {
  handlers.set("get_window_material", () => "liquid-glass");
  const { unmount } = renderWithProviders(<App />);
  await waitFor(() =>
    expect(document.documentElement.dataset.windowMaterial).toBe(
      "liquid-glass",
    ),
  );
  unmount();
  handlers.set("get_window_material", () => "solid");
  renderWithProviders(<App />);
  expect(document.documentElement.dataset.windowMaterial).toBeUndefined();
});

it("protects an MCP draft when navigating from the sidebar", async () => {
  localStorage.setItem("skill-studio-view", "mcp");
  handlers.set("get_settings", () => ({
    ...defaultSettings(),
    manageMcp: true,
  }));
  renderWithProviders(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "添加 MCP" }));
  fireEvent.change(screen.getByLabelText("名称"), {
    target: { value: "draft-service" },
  });
  const projects = within(screen.getByRole("navigation")).getByRole("button", {
    name: "项目",
  });
  fireEvent.click(projects);
  expect(await screen.findByRole("dialog")).toHaveTextContent(
    "放弃未保存的修改？",
  );
  fireEvent.click(screen.getByRole("button", { name: "继续编辑" }));
  expect(screen.getByLabelText("名称")).toHaveValue("draft-service");
  fireEvent.click(projects);
  fireEvent.click(
    await screen.findByRole("button", { name: "放弃修改并离开" }),
  );
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("项目");
});
