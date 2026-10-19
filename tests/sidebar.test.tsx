import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, it } from "vitest";
import App from "@/App";
import { renderWithProviders } from "./utils/render";
import {
  claudeAgent,
  defaultSettings,
  handlers,
  makeGroup,
} from "./mocks/tauri";

it("keeps sidebar navigation mounted across Agent, settings and Hub pages", async () => {
  handlers.set("list_agents", () => [claudeAgent]);
  handlers.set("get_settings", () => ({
    ...defaultSettings(),
    manageMcp: true,
  }));
  renderWithProviders(<App />);
  const sidebar = screen.getByRole("complementary", { name: "侧边栏" });
  const nav = within(sidebar).getByRole("navigation", { name: "主导航" });
  const header = within(screen.getByRole("banner"));
  const groups = await within(nav).findByRole("button", { name: "分组" });
  fireEvent.click(await header.findByRole("button", { name: "MCP" }));
  fireEvent.click(groups);
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("分组");
  // 两个分组页文案相同，用空状态说明确认停在 MCP 分组页
  expect(await screen.findByText(/从 MCP Hub 选择需要的 MCP/)).toBeVisible();
  fireEvent.click(within(sidebar).getByRole("button", { name: "设置" }));
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("设置");
  expect(within(nav).getByRole("button", { name: "分组" })).toBe(groups);
  fireEvent.click(screen.getByRole("button", { name: "返回" }));
  expect(await screen.findByText(/从 MCP Hub 选择需要的 MCP/)).toBeVisible();
  fireEvent.click(header.getByRole("button", { name: "Skill" }));
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("分组");
  fireEvent.click(within(nav).getByRole("button", { name: "Hub" }));
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
    "Skill Hub",
  );
  expect(within(nav).getByRole("button", { name: "分组" })).toBe(groups);
});

it("侧栏「分组」用四宫格图标，设置页「分组与安装」用同一个，上方卡片不用长得像的图标", async () => {
  handlers.set("list_agents", () => [claudeAgent]);
  renderWithProviders(<App />);
  // svg 上 lucide-<图标名> 的 class，例如 lucide-layout-grid
  const iconOf = (el: Element) =>
    (el.querySelector("svg")?.getAttribute("class") ?? "")
      .split(" ")
      .filter((c) => c.startsWith("lucide-"));
  const sidebar = screen.getByRole("complementary", { name: "侧边栏" });
  const groups = await within(sidebar).findByRole("button", { name: "分组" });
  expect(iconOf(groups)).toEqual(["lucide-layout-grid"]);
  fireEvent.click(within(sidebar).getByRole("button", { name: "设置" }));
  const section = await screen.findByRole("heading", { name: "分组与安装" });
  expect(iconOf(section)).toEqual(iconOf(groups));
  // 紧挨在上方的「显示 Codex App 内置 MCP」原先用积木，也是几个方块，和四宫格看着像；
  // 改用眼睛，对应标题里的「显示」（与「启用 MCP 管理」用电源键同理）
  const builtin = screen
    .getByText("显示 Codex App 内置 MCP")
    .closest("div.flex");
  expect(iconOf(builtin!)).toEqual(["lucide-eye"]);
});

it("returns to the MCP Hub list when the currently selected Hub is clicked", async () => {
  localStorage.setItem("skill-studio-resource", "mcp");
  handlers.set("get_settings", () => ({
    ...defaultSettings(),
    manageMcp: true,
  }));
  renderWithProviders(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "添加 MCP" }));
  expect(
    await screen.findByLabelText("粘贴安装命令、网址或配置"),
  ).toBeInTheDocument();
  fireEvent.click(
    within(screen.getByRole("navigation")).getByRole("button", {
      name: "Hub",
    }),
  );
  expect(
    screen.queryByLabelText("粘贴安装命令、网址或配置"),
  ).not.toBeInTheDocument();
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
    "MCP Hub",
  );
  expect(screen.getByPlaceholderText("按名称或地址搜索 MCP…")).toBeVisible();
});

it("clicking the current nav item closes an open group detail, after confirming unsaved changes", async () => {
  localStorage.setItem("skill-studio-view", "groups");
  handlers.set("list_groups", () => [makeGroup({ id: "g", name: "Dev" })]);
  renderWithProviders(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "编辑 Dev" }));
  fireEvent.change(await screen.findByLabelText("分组名称"), {
    target: { value: "Dev 2" },
  });
  const groups = within(screen.getByRole("navigation")).getByRole("button", {
    name: "分组",
  });
  fireEvent.click(groups);
  fireEvent.click(
    await screen.findByRole("button", { name: "放弃修改并离开" }),
  );
  expect(
    await screen.findByRole("button", { name: "编辑 Dev" }),
  ).toBeInTheDocument();
  expect(screen.queryByLabelText("分组名称")).not.toBeInTheDocument();
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
  localStorage.setItem("skill-studio-resource", "mcp");
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
