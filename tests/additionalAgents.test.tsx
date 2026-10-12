import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, it } from "vitest";
import App from "@/App";
import { AgentPage } from "@/pages/AgentGroupsPage";
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

it("opens all three Agents in both Skill and MCP navigation", async () => {
  setup();
  renderWithProviders(<App />);
  const nav = within(screen.getByRole("navigation", { name: "主导航" }));
  for (const agent of skillOnlyAgents) {
    fireEvent.click(
      await nav.findByRole("button", { name: agent.displayName }),
    );
    expect(
      await screen.findByRole("heading", { name: agent.displayName }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/手动安装的 Skill 始终保留/),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: `${agent.displayName} Skill 使用说明`,
      }),
    ).toBeEnabled();
    expect(screen.getByRole("button", { name: "新建分组" })).toBeEnabled();
  }
  fireEvent.click(nav.getByRole("button", { name: "MCP Hub" }));
  for (const agent of skillOnlyAgents) {
    fireEvent.click(nav.getByRole("button", { name: agent.displayName }));
    expect(
      await screen.findByRole("heading", { name: agent.displayName }),
    ).toBeVisible();
    await waitFor(() =>
      expect(screen.getByRole("tab", { name: /^分组/ })).toBeVisible(),
    );
  }
  expect(nav.getByRole("button", { name: "Claude Code" })).toBeVisible();
  expect(nav.getByRole("button", { name: "Codex" })).toBeVisible();
  fireEvent.click(nav.getByRole("button", { name: "Skill Hub" }));
  for (const agent of skillOnlyAgents) {
    expect(nav.getByRole("button", { name: agent.displayName })).toBeVisible();
  }
});

it("restores Pi MCP view and provides native MCP usage help", async () => {
  setup();
  localStorage.setItem("skill-studio-view", "agent:pi");
  localStorage.setItem("skill-studio-resource", "mcp");
  renderWithProviders(<App />);
  expect(await screen.findByRole("heading", { name: "Pi" })).toBeVisible();
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
  "creates a group for $displayName and hides unsupported native skill toggles",
  async (agent) => {
    setup();
    handlers.set("scan_skills", () => [
      makeSkill({
        id: "demo",
        name: "Demo",
        agents: { [agent.id]: agentState("source") },
      }),
    ]);
    handlers.set("save_agent_group", (args) => makeGroup(args));
    renderWithProviders(<AgentPage agentId={agent.id} />);
    fireEvent.click(await screen.findByRole("button", { name: "新建分组" }));
    fireEvent.change(screen.getByLabelText("分组名称"), {
      target: { value: "Dev" },
    });
    fireEvent.click(await screen.findByRole("checkbox", { name: "选择 Demo" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() =>
      expect(calls.find((c) => c.command === "save_agent_group")?.args).toEqual(
        {
          groupId: null,
          agentId: agent.id,
          name: "Dev",
          skillIds: ["demo"],
        },
      ),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    fireEvent.mouseDown(screen.getByRole("tab", { name: /已安装 skill/ }), {
      button: 0,
      ctrlKey: false,
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
