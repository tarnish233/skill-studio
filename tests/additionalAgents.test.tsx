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
  // 两个分组页文案相同，用空状态说明确认停在 MCP 分组页
  expect(
    await screen.findByText(/从 MCP Hub 选择需要的 MCP/),
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

it.each(skillOnlyAgents)(
  "writes a project to $displayName alone, into the directory the backend provides",
  async (agent) => {
    setup();
    handlers.set("list_projects", () => [makeProject({ skillIds: ["demo"] })]);
    handlers.set("scan_skills", () => [
      makeSkill({ id: "demo", name: "Demo" }),
    ]);
    renderWithProviders(<ProjectsPage />);
    const icon = await screen.findByRole("button", {
      name: `为 ${agent.displayName} 启用「webapp」`,
    });
    // 写到哪个目录由后端提供的 projectSkillDir 决定，写在图标提示里
    fireEvent.focus(icon);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      `写到 ${agent.projectSkillDir}`,
    );
    fireEvent.click(icon);
    await waitFor(() =>
      expect(
        calls.find((c) => c.command === "set_project_agent")?.args,
      ).toEqual({ projectId: "p1", agentId: agent.id, enabled: true }),
    );
  },
);

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
    // 保存过的分组之后能读到，和真实后端一样
    let saved: ReturnType<typeof makeGroup>[] = [];
    handlers.set("list_groups", () => saved);
    handlers.set("save_group", (args) => {
      const group = makeGroup({
        id: "dev",
        name: args.name as string,
        skillIds: args.skillIds as string[],
      });
      saved = [group];
      return group;
    });
    renderWithProviders(<GroupsPage />);
    fireEvent.click(await screen.findByRole("button", { name: "新建分组" }));
    fireEvent.change(screen.getByLabelText("分组名称"), {
      target: { value: "Dev" },
    });
    fireEvent.click(await screen.findByRole("checkbox", { name: "Demo" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() =>
      expect(calls.find((c) => c.command === "save_group")?.args).toEqual({
        groupId: null,
        name: "Dev",
        skillIds: ["demo"],
        expected: null,
      }),
    );
    // 新建的分组保存后留在整页编辑里，保存按钮回到不可点
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "保存" })).toBeDisabled(),
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
