import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { Boxes } from "lucide-react";
import { expect, it } from "vitest";
import { AgentIcon, agentIcon } from "@/components/common/AgentIcon";
import { GroupsPage } from "@/pages/GroupsPage";
import { ProjectsPage } from "@/pages/ProjectsPage";
import { McpGroupsPage } from "@/pages/McpGroupsPage";
import { SettingsPage } from "@/pages/SettingsPage";
import { setTarget } from "@/lib/api/transport";
import { supportsMcp } from "@/lib/mcpAgents";
import { variantLabel } from "@/lib/skillVariants";
import { SOURCE_TITLES } from "@/components/common/SourceMark";
import { renderWithProviders } from "./utils/render";
import {
  agentState,
  calls,
  claudeAgent,
  codexAgent,
  handlers,
  makeAgent,
  makeGroup,
  makeProject,
  makeSkill,
  skillOnlyAgents,
} from "./mocks/tauri";

const harness = makeAgent({
  id: "deepseek-harness",
  displayName: "DeepSeek Harness",
  configDir: "/home/u/.dsh",
  globalSkillDirs: ["/home/u/.dsh/skills", "/home/u/.agents/skills"],
  cliVersion: "0.2.1-rc.5",
  projectSkillDir: ".dsh/skills",
  supportsNativeToggle: false,
});
function setup() {
  setTarget({ id: "local", name: "本机", connected: true });
  handlers.set("list_agents", () => [
    claudeAgent,
    codexAgent,
    ...skillOnlyAgents,
    harness,
  ]);
  handlers.set("get_config", () => ({ activeGroups: {} }));
  handlers.set("scan_skills", () => [
    makeSkill({
      id: "demo",
      name: "Demo",
      agents: { [harness.id]: agentState("notLinked") },
    }),
  ]);
}

it("labels a Harness-specific Skill variant as a known Agent rather than an unknown source", () => {
  expect(variantLabel(harness.id)).toBe("DeepSeek Harness 专用版");
});

it("includes Harness in the shared-directory explanation", () => {
  expect(SOURCE_TITLES.agent).toContain("DeepSeek Harness");
});

it("uses the official Harness whale mark at the existing 20px and 16px sizes", () => {
  expect(agentIcon(harness.id)).not.toBe(Boxes);
  const { container } = renderWithProviders(
    <>
      <AgentIcon agentId={harness.id} />
      <AgentIcon agentId={harness.id} size="sm" />
    </>,
  );
  const marks = container.querySelectorAll("svg");
  expect(marks).toHaveLength(2);
  expect(marks[0]).toHaveClass("h-5", "w-5", "shrink-0");
  expect(marks[1]).toHaveClass("h-4", "w-4", "shrink-0");
  for (const mark of marks) {
    expect(mark).toHaveAttribute("aria-hidden", "true");
    expect(mark.querySelector("path")?.getAttribute("d")).toContain("M168.384");
    expect(mark.querySelector("clipPath")).toBeNull();
  }
});

it("exposes Harness on Skill groups with explicit directory-only limitations", async () => {
  setup();
  handlers.set("list_groups", () => [
    makeGroup({ id: "g", name: "Dev", skillIds: ["demo"] }),
  ]);
  renderWithProviders(<GroupsPage />);
  expect(
    await screen.findByRole("button", {
      name: "为 DeepSeek Harness 启用「Dev」",
    }),
  ).toBeInTheDocument();
  const filters = screen.getByRole("group", { name: "按 Agent 查看" });
  fireEvent.click(
    within(filters).getByRole("button", { name: /^DeepSeek Harness/ }),
  );
  fireEvent.click(
    await screen.findByRole("button", {
      name: "DeepSeek Harness Skill 使用说明",
    }),
  );
  const note = screen.getByRole("dialog", {
    name: "DeepSeek Harness Skill 使用说明",
  });
  expect(note).toHaveTextContent("平铺");
  expect(note).toHaveTextContent("Profile");
  expect(note).toHaveTextContent(".dsh/skills");
});

it("deploys a project only to Harness and uses the backend-provided directory", async () => {
  setup();
  handlers.set("list_projects", () => [makeProject({ skillIds: ["demo"] })]);
  renderWithProviders(<ProjectsPage />);
  const button = await screen.findByRole("button", {
    name: "为 DeepSeek Harness 启用「webapp」",
  });
  fireEvent.focus(button);
  expect(await screen.findByRole("tooltip")).toHaveTextContent(
    "写到 .dsh/skills",
  );
  fireEvent.click(button);
  await waitFor(() =>
    expect(
      calls.find((call) => call.command === "set_project_agent")?.args,
    ).toEqual({ projectId: "p1", agentId: harness.id, enabled: true }),
  );
});

it("allows Harness Skill management in settings without exposing it as an MCP client", async () => {
  setup();
  const view = renderWithProviders(<SettingsPage />);
  fireEvent.click(
    await screen.findByRole("button", {
      name: "DeepSeek Harness",
    }),
  );
  await waitFor(() =>
    expect(
      calls.find((call) => call.command === "update_settings")?.args,
    ).toMatchObject({ patch: { disabledAgents: [harness.id] } }),
  );
  view.unmount();
  expect(supportsMcp(harness.id)).toBe(false);
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          running: false,
          entries: [],
          servers: [],
          discovered: [],
          scanWarnings: [],
          groups: [{ id: "m", name: "MCP", entryIds: [], sortOrder: 0 }],
          activeGroups: {},
        }
      : null,
  );
  renderWithProviders(<McpGroupsPage />);
  await screen.findByRole("group", { name: "按 Agent 查看" });
  expect(
    screen.queryByRole("button", { name: /^DeepSeek Harness/ }),
  ).not.toBeInTheDocument();
});
