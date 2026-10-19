import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useQueryClient } from "@tanstack/react-query";
import { expect, it } from "vitest";
import { LibraryPage } from "@/pages/LibraryPage";
import { McpPage } from "@/pages/McpPage";
import { hubCatalog, isSkillVisible } from "@/lib/hubCatalog";
import { queryKeys } from "@/lib/queryKeys";
import { renderWithProviders } from "./utils/render";
import {
  handlers,
  agentState,
  defaultSettings,
  makeSkill,
  claudeAgent,
  codexAgent,
  skillOnlyAgents,
} from "./mocks/tauri";

function HideAgents() {
  const client = useQueryClient();
  return (
    <>
      <button
        onClick={() =>
          client.setQueryData(queryKeys.settings, {
            ...defaultSettings(),
            disabledAgents: ["claude-code", "opencode", "pi", "grok"],
          })
        }
      >
        隐藏测试应用
      </button>
      <button
        onClick={() =>
          client.setQueryData(queryKeys.settings, defaultSettings())
        }
      >
        恢复测试应用
      </button>
    </>
  );
}

it("hides disabled Agent-only Skills and updates totals while retaining Hub content", async () => {
  handlers.set("list_agents", () => [
    claudeAgent,
    codexAgent,
    ...skillOnlyAgents,
  ]);
  handlers.set("scan_skills", () => [
    makeSkill({
      id: "open",
      name: "Open skill",
      sourceIds: ["opencode"],
      contentHash: "open",
      origin: { kind: "inPlace", ownerAgent: "opencode" },
      agents: { opencode: agentState("source") },
      tokens: { skillMd: 900, extras: 0 },
    }),
    makeSkill({
      id: "hub",
      name: "Hub skill",
      sourceIds: ["studio"],
      origin: { kind: "hub" },
      contentHash: "hub",
    }),
  ]);
  renderWithProviders(
    <>
      <HideAgents />
      <LibraryPage />
    </>,
  );
  const sources = within(
    await screen.findByRole("group", { name: "筛选 skill" }),
  );
  fireEvent.click(await sources.findByRole("button", { name: "OpenCode 1" }));
  expect(
    screen.queryByRole("button", { name: "预览 Hub skill" }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "隐藏测试应用" }));
  await waitFor(() =>
    expect(
      sources.queryByRole("button", { name: /OpenCode/ }),
    ).not.toBeInTheDocument(),
  );
  for (const name of [/Claude Code/, /^Pi /, /Grok Build/])
    expect(sources.queryByRole("button", { name })).not.toBeInTheDocument();
  expect(sources.getByRole("button", { name: "Codex 0" })).toBeVisible();
  expect(sources.getByRole("button", { name: "Skill Studio 1" })).toBeVisible();
  expect(
    screen.queryByRole("button", { name: "预览 Open skill" }),
  ).not.toBeInTheDocument();
  expect(screen.getByText("合计 ≈ 100 tokens")).toBeVisible();
  expect(sources.getByRole("button", { name: "全部 1" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(screen.getByRole("button", { name: "预览 Hub skill" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "恢复测试应用" }));
  expect(
    await screen.findByRole("button", { name: "预览 Open skill" }),
  ).toBeVisible();
  expect(sources.getByRole("button", { name: "全部 2" })).toBeVisible();
});

it("maps disabled Claude and new Agents to MCP statistics and clears hidden filters", async () => {
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          running: false,
          servers: [],
          discovered: [],
          scanWarnings: [],
          entries: ["opencode", "codex"].map((agent) => ({
            id: agent,
            name: agent,
            mode: "direct",
            definition: { type: "stdio", command: agent },
            oauth: false,
            clientId: null,
            scopes: [],
            bindings: [
              {
                id: agent,
                agent,
                path: `/global/${agent}`,
                project: null,
                key: agent,
                original: null,
                installed: { command: agent },
              },
            ],
          })),
        }
      : null,
  );
  renderWithProviders(
    <>
      <HideAgents />
      <McpPage />
    </>,
  );
  const sources = within(
    await screen.findByRole("group", { name: "筛选 MCP" }),
  );
  fireEvent.click(await sources.findByRole("button", { name: "OpenCode 1" }));
  expect(
    screen.queryByRole("button", { name: "查看 codex" }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "隐藏测试应用" }));
  await waitFor(() =>
    expect(
      sources.queryByRole("button", { name: /OpenCode/ }),
    ).not.toBeInTheDocument(),
  );
  for (const name of [/Claude Code/, /Pi /, /Grok Build/])
    expect(sources.queryByRole("button", { name })).not.toBeInTheDocument();
  expect(sources.getByRole("button", { name: "Codex 1" })).toBeVisible();
  expect(sources.getByRole("button", { name: "全部 2" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(screen.getByRole("button", { name: "查看 codex" })).toBeVisible();
});

it("hides private unmanaged MCPs but keeps shared connections and independent Hub entries", async () => {
  const source = (agent: string, command: string) => ({
    id: `${agent}-${command}`,
    agent,
    path: `/global/${agent}`,
    key: command,
    scope: "用户全局",
    enabled: true,
    gateway: false,
    definition: { command },
  });
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          running: false,
          servers: [],
          scanWarnings: [],
          entries: [
            {
              id: "hub",
              name: "Hub tools",
              mode: "direct",
              definition: { type: "stdio", command: "hub" },
              oauth: false,
              clientId: null,
              scopes: [],
              bindings: [],
            },
          ],
          discovered: [
            ...["opencode", "grok", "claude"].map((agent) => ({
              id: agent,
              name: `Only ${agent}`,
              server: null,
              issue: null,
              managedId: null,
              sources: [source(agent, agent)],
            })),
            {
              id: "shared",
              name: "Shared tools",
              server: null,
              issue: null,
              managedId: null,
              sources: [
                source("opencode", "shared"),
                source("codex", "shared"),
              ],
            },
          ],
        }
      : null,
  );
  renderWithProviders(
    <>
      <HideAgents />
      <McpPage />
    </>,
  );
  expect(
    await screen.findByRole("button", { name: "查看 Only opencode" }),
  ).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "隐藏测试应用" }));
  await waitFor(() =>
    expect(
      screen.queryByRole("button", { name: "查看 Only opencode" }),
    ).not.toBeInTheDocument(),
  );
  for (const agent of ["grok", "claude"])
    expect(
      screen.queryByRole("button", { name: `查看 Only ${agent}` }),
    ).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "查看 Shared tools" }),
  ).toBeVisible();
  expect(screen.getByRole("button", { name: "查看 Hub tools" })).toBeVisible();
  expect(screen.getByRole("button", { name: "全部 2" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "恢复测试应用" }));
  expect(
    await screen.findByRole("button", { name: "查看 Only opencode" }),
  ).toBeVisible();
  expect(screen.getByRole("button", { name: "全部 5" })).toBeVisible();
});

it("filters Skill sources before merging so previews do not target a hidden Agent's copy", () => {
  const hidden = makeSkill({
    id: "hidden",
    name: "Same",
    sourcePath: "/a/opencode/skills/Same",
    root: "/a/opencode/skills",
    sourceIds: ["opencode"],
    origin: { kind: "inPlace", ownerAgent: "opencode" },
    agents: { opencode: agentState("source") },
  });
  const visible = makeSkill({
    id: "visible",
    name: "Same",
    sourcePath: "/z/codex/skills/Same",
    root: "/z/codex/skills",
    sourceIds: ["codex"],
    origin: { kind: "inPlace", ownerAgent: "codex" },
    agents: { codex: agentState("source") },
  });
  const cards = hubCatalog(
    [hidden, visible].filter((skill) => isSkillVisible(skill, ["opencode"])),
  );
  expect(cards).toHaveLength(1);
  expect(cards[0].skill.id).toBe("visible");
  expect(cards[0].sources.map((s) => s.id)).toEqual(["visible"]);
  expect(
    isSkillVisible({ ...hidden, diagnostics: ["missing target"] }, [
      "opencode",
    ]),
  ).toBe(false);
  expect(
    isSkillVisible(
      { ...hidden, agents: { ...hidden.agents, codex: agentState("linked") } },
      ["opencode"],
    ),
  ).toBe(true);
  expect(
    isSkillVisible(
      { ...hidden, agents: { ...hidden.agents, codex: agentState("copied") } },
      ["opencode"],
    ),
  ).toBe(true);
  expect(
    isSkillVisible(
      { ...hidden, agents: { ...hidden.agents, codex: agentState("foreign") } },
      ["opencode"],
    ),
  ).toBe(false);
  expect(
    isSkillVisible({ ...hidden, origin: { kind: "hub" } }, ["opencode"]),
  ).toBe(true);
  expect(
    isSkillVisible(
      {
        ...hidden,
        sourceIds: ["agent"],
        sourcePath: "/u/.agents/skills/Same",
        root: "/u/.agents/skills",
      },
      ["opencode", "codex"],
    ),
  ).toBe(true);
  expect(
    isSkillVisible(
      makeSkill({
        origin: { kind: "external" },
        agents: {},
        sourceIds: ["external"],
      }),
      ["opencode", "claude-code"],
    ),
  ).toBe(true);
});
