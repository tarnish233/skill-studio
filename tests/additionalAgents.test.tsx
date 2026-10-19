import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, it } from "vitest";
import App from "@/App";
import { GroupsPage } from "@/pages/GroupsPage";
import { McpGroupsPage } from "@/pages/McpGroupsPage";
import { setTarget } from "@/lib/api/transport";
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

/** 分组页顶部的 Agent 视角胶囊 */
const viewChip = (name: RegExp) =>
  within(screen.getByRole("group", { name: "按 Agent 查看" })).getByRole(
    "button",
    { name },
  );

it("offers every additional Agent on shared group cards and as an Agent view, for Skills and MCP", async () => {
  setup();
  setTarget({ id: "local", name: "本机", connected: true });
  handlers.set("scan_skills", () => [makeSkill({ id: "demo", name: "Demo" })]);
  handlers.set("list_groups", () => [
    makeGroup({ id: "g", name: "Dev", skillIds: ["demo"] }),
  ]);
  const view = renderWithProviders(<GroupsPage />);
  for (const agent of skillOnlyAgents) {
    expect(
      await screen.findByRole("button", {
        name: `为 ${agent.displayName} 启用「Dev」`,
      }),
    ).toBeInTheDocument();
    expect(viewChip(new RegExp(`^${agent.displayName}`))).toBeInTheDocument();
  }
  view.unmount();

  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          running: false,
          entries: [
            {
              id: "one",
              name: "Work",
              mode: "direct",
              definition: { type: "stdio", command: "work" },
              oauth: false,
              clientId: null,
              scopes: [],
              bindings: [],
            },
          ],
          servers: [],
          discovered: [],
          scanWarnings: [],
          groups: [{ id: "m", name: "开发", entryIds: ["one"], sortOrder: 0 }],
          activeGroups: {},
        }
      : null,
  );
  renderWithProviders(<McpGroupsPage />);
  for (const agent of skillOnlyAgents)
    expect(
      await screen.findByRole("button", {
        name: `为 ${agent.displayName} 启用「开发」`,
      }),
    ).toBeInTheDocument();
});

it("offers Pi's native MCP help in Pi's MCP group view", async () => {
  setup();
  localStorage.setItem("skill-studio-view", "groups");
  localStorage.setItem("skill-studio-resource", "mcp");
  renderWithProviders(<App />);
  expect(
    await screen.findByRole("button", { name: "新建 MCP 分组" }),
  ).toBeInTheDocument();
  fireEvent.click(viewChip(/^Pi/));
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
  "lets a shared group include a skill that lives in $displayName",
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
  },
);

it.each(skillOnlyAgents.filter((agent) => !agent.supportsNativeToggle))(
  "explains how $displayName loads skills in its Agent view, since it has no native toggle",
  async (agent) => {
    setup();
    handlers.set("scan_skills", () => [
      makeSkill({
        id: "demo",
        name: "Demo",
        agents: { [agent.id]: agentState("source") },
      }),
    ]);
    renderWithProviders(<GroupsPage />);
    fireEvent.click(
      await waitFor(() => viewChip(new RegExp(`^${agent.displayName}`))),
    );
    expect(screen.getByText("已安装 1")).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: `${agent.displayName} Skill 使用说明`,
      }),
    ).toBeEnabled();
  },
);

it("Pi 已接入原生开关：Agent 视角按已启用统计，不再显示「暂不读取原生配置」的说明", async () => {
  setup();
  const pi = skillOnlyAgents.find((agent) => agent.id === "pi")!;
  expect(pi.supportsNativeToggle).toBe(true);
  handlers.set("scan_skills", () => [
    makeSkill({
      id: "demo",
      name: "Demo",
      agents: { pi: agentState("source") },
    }),
  ]);
  renderWithProviders(<GroupsPage />);
  fireEvent.click(await waitFor(() => viewChip(/^Pi/)));
  expect(screen.getByText("已启用 1")).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Pi Skill 使用说明" }),
  ).not.toBeInTheDocument();
});

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
