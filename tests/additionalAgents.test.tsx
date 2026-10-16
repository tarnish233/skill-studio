import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, it } from "vitest";
import App from "@/App";
import { GroupsPage } from "@/pages/GroupsPage";
import { ProjectsPage } from "@/pages/ProjectsPage";
import { SettingsPage } from "@/pages/SettingsPage";
import { renderWithProviders } from "./utils/render";
import {
  agentState,
  calls,
  claudeAgent,
  codexAgent,
  defaultSettings,
  handlers,
  makeGroup,
  makeProject,
  makeSkill,
  skillOnlyAgents,
} from "./mocks/tauri";

function setup() {
  handlers.set("list_agents", () => [
    claudeAgent,
    codexAgent,
    ...skillOnlyAgents,
  ]);
  handlers.set("get_config", () => ({ activeGroups: {} }));
}

it("exposes all three additional Agents through the Agent selector in both Skill and MCP groups", async () => {
  setup();
  renderWithProviders(<App />);
  const nav = within(screen.getByRole("navigation", { name: "主导航" }));
  fireEvent.click(await nav.findByRole("button", { name: "分组" }));
  expect(await screen.findByRole("button", { name: "新建分组" })).toBeEnabled();
  fireEvent.mouseDown(screen.getByRole("tab", { name: /已安装 skill/ }), {
    button: 0,
    ctrlKey: false,
  });
  const picker = screen.getByLabelText("选择 Agent");
  for (const agent of skillOnlyAgents) {
    fireEvent.change(picker, { target: { value: agent.id } });
    expect(
      screen.getByRole("button", {
        name: `${agent.displayName} Skill 使用说明`,
      }),
    ).toBeEnabled();
  }

  fireEvent.click(
    within(screen.getByRole("banner")).getByRole("button", { name: "MCP" }),
  );
  await waitFor(() =>
    expect(screen.getByRole("tab", { name: /^分组/ })).toBeVisible(),
  );
  fireEvent.mouseDown(screen.getByRole("tab", { name: /已配置 MCP/ }), {
    button: 0,
    ctrlKey: false,
  });
  const mcpPicker = screen.getByLabelText("选择 Agent");
  for (const agent of skillOnlyAgents) {
    fireEvent.change(mcpPicker, { target: { value: agent.id } });
    expect(screen.getByLabelText("选择 Agent")).toHaveValue(agent.id);
  }
});

it("offers Pi's native MCP help from the MCP Groups page", async () => {
  setup();
  localStorage.setItem("skill-studio-view", "groups");
  localStorage.setItem("skill-studio-resource", "mcp");
  renderWithProviders(<App />);
  expect(
    await screen.findByRole("button", { name: "新建 MCP 分组" }),
  ).toBeInTheDocument();
  fireEvent.mouseDown(screen.getByRole("tab", { name: /已配置 MCP/ }), {
    button: 0,
    ctrlKey: false,
  });
  fireEvent.change(screen.getByLabelText("选择 Agent"), {
    target: { value: "pi" },
  });
  expect(
    screen.queryByText("pi install npm:pi-mcp-adapter"),
  ).not.toBeInTheDocument();
  // Radix/floating-ui opening is checked in the native GUI; jsdom layout is unstable.
  expect(
    screen.getByRole("button", { name: "Pi MCP 使用说明" }),
  ).toHaveAttribute("aria-expanded", "false");
});

it("writes all selected Skill Agents to a project using each backend-provided directory", async () => {
  setup();
  handlers.set("list_projects", () => [makeProject()]);
  handlers.set("scan_skills", () => [makeSkill({ id: "demo", name: "Demo" })]);
  renderWithProviders(<ProjectsPage />);
  fireEvent.click(await screen.findByText("webapp"));
  for (const agent of skillOnlyAgents) {
    expect(await screen.findByText(agent.projectSkillDir!)).toBeVisible();
    fireEvent.click(screen.getByRole("checkbox", { name: agent.displayName }));
  }
  fireEvent.click(screen.getByRole("checkbox", { name: "Demo" }));
  fireEvent.click(screen.getByRole("button", { name: "写入项目" }));
  await waitFor(() =>
    expect(calls.find((c) => c.command === "apply_project")?.args).toEqual({
      projectId: "p1",
      selection: {
        agentIds: ["opencode", "pi", "grok"],
        skillIds: ["demo"],
        groupIds: [],
        linkMode: "copy",
      },
    }),
  );
});

it.each(skillOnlyAgents)(
  "lets a shared group apply to $displayName and hides its unsupported native skill toggle",
  async (agent) => {
    setup();
    handlers.set("scan_skills", () => [
      makeSkill({
        id: "demo",
        name: "Demo",
        agents: { [agent.id]: agentState("source") },
      }),
    ]);
    handlers.set("save_group", (args) => makeGroup(args));
    renderWithProviders(<GroupsPage />);
    fireEvent.click(await screen.findByRole("button", { name: "新建分组" }));
    fireEvent.change(screen.getByLabelText("分组名称"), {
      target: { value: "Dev" },
    });
    fireEvent.click(await screen.findByRole("checkbox", { name: "选择 Demo" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() =>
      expect(calls.find((c) => c.command === "save_group")?.args).toEqual({
        groupId: null,
        name: "Dev",
        skillIds: ["demo"],
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    fireEvent.mouseDown(screen.getByRole("tab", { name: /已安装 skill/ }), {
      button: 0,
      ctrlKey: false,
    });
    // 分组不归属任何一个 Agent：切到这个 Agent 才能看到它上面的"已安装 skill"视图
    fireEvent.change(screen.getByLabelText("选择 Agent"), {
      target: { value: agent.id },
    });
    expect(await screen.findByText("Demo")).toBeVisible();
    expect(
      screen.queryByRole("switch", { name: "启用 Demo" }),
    ).not.toBeInTheDocument();
  },
);

it("persists management visibility for new Agents through settings", async () => {
  setup();
  let settings = defaultSettings();
  handlers.set("get_settings", () => settings);
  handlers.set(
    "update_settings",
    ({ patch }) => (settings = { ...settings, ...patch }),
  );
  renderWithProviders(<SettingsPage />);
  for (const agent of skillOnlyAgents) {
    const button = await screen.findByRole("button", {
      name: agent.displayName,
    });
    await waitFor(() =>
      expect(button).toHaveAttribute("aria-disabled", "false"),
    );
    fireEvent.click(button);
    await waitFor(() =>
      expect(button).toHaveAttribute("aria-pressed", "false"),
    );
    await waitFor(() =>
      expect(button).toHaveAttribute("aria-disabled", "false"),
    );
  }
  expect(settings.disabledAgents).toEqual(["opencode", "pi", "grok"]);
});
