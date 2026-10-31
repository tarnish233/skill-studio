import { screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { McpPage } from "@/pages/McpPage";
import {
  entrySnapshot,
  mcpSourceIds,
  type ManagedMcp,
  type McpRow,
} from "@/lib/api/mcpManagement";
import type { McpSource } from "@/lib/api/mcp";
import { handlers } from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

const entry = (over: Partial<ManagedMcp> = {}): ManagedMcp => ({
  id: "sample",
  name: "sample",
  mode: "direct",
  definition: { type: "stdio", command: "npx" },
  oauth: false,
  clientId: null,
  scopes: [],
  bindings: [],
  provenance: { sourceIds: ["studio"], originalSources: [] },
  ...over,
});
const source = (agent: string): McpSource => ({
  id: agent,
  agent,
  path: `/config/${agent}.json`,
  scope: "用户全局",
  project: null,
  key: "sample",
  definition: { type: "stdio", command: "npx" },
  enabled: true,
  gateway: false,
});
function renderMcp(value: ManagedMcp, sources: McpSource[]) {
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          running: false,
          entries: [value],
          servers: [],
          discovered: [
            {
              id: "found",
              name: "sample",
              server: null,
              managedId: value.id,
              issue: null,
              sources,
            },
          ],
          scanWarnings: [],
        }
      : null,
  );
  renderWithProviders(<McpPage />);
}

it("Studio origin is not replaced by a Pi connection in cards or source statistics", async () => {
  renderMcp(entry(), [source("pi")]);
  expect(
    await screen.findByRole("img", { name: "来源：Skill Studio" }),
  ).toBeInTheDocument();
  expect(screen.queryByRole("img", { name: "来源：Pi" })).toBeNull();
  expect(
    screen.getByRole("button", { name: "Skill Studio 1" }),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Pi 0" })).toBeInTheDocument();
  expect(screen.queryByRole("combobox", { name: "筛选接入 Agent" })).toBeNull();
});
it("adopted MCPs preserve their original Agent source after deployment elsewhere", async () => {
  renderMcp(
    entry({
      provenance: {
        sourceIds: ["claude"],
        originalSources: [
          {
            agent: "claude",
            path: "/original/.claude.json",
            project: null,
            key: "sample",
          },
        ],
      },
    }),
    [source("pi")],
  );
  const mark = await screen.findByRole("img", { name: "来源：Claude Code" });
  expect(mark).toHaveAttribute(
    "title",
    expect.stringContaining("/original/.claude.json"),
  );
  expect(screen.queryByRole("img", { name: "来源：Pi" })).toBeNull();
  expect(
    screen.getByRole("button", { name: "Claude Code 1" }),
  ).toBeInTheDocument();
});
it("legacy managed MCPs remain unknown instead of guessing from management or connections", async () => {
  renderMcp(entry({ provenance: undefined }), [source("pi")]);
  expect(
    await screen.findByRole("button", { name: "来源待确认 1" }),
  ).toBeInTheDocument();
  expect(screen.queryByRole("img", { name: "来源：Pi" })).toBeNull();
  expect(screen.queryByRole("img", { name: "来源：Skill Studio" })).toBeNull();
  expect(
    screen.getByText(
      (_, element) => element?.textContent === "来源：来源待确认",
    ),
  ).toBeInTheDocument();
});
it("unmanaged MCPs use their discovered Agent configuration sources", () => {
  const value: McpRow = {
    entry: entry(),
    managed: false,
    sources: [source("pi"), source("codex"), source("pi")],
  };
  expect(mcpSourceIds(value)).toEqual(["pi", "codex"]);
});
it("deployment binding changes never change stored source IDs", () => {
  const value: McpRow = {
    entry: entry(),
    managed: true,
    sources: [source("pi")],
  };
  value.entry.bindings = [
    {
      id: "sample",
      agent: "codex",
      path: "/new/config.toml",
      project: null,
      key: "sample",
      original: null,
      installed: value.entry.definition,
    },
  ];
  expect(mcpSourceIds(value)).toEqual(["studio"]);
});
it("source-only metadata does not mark group deployments as pending", () => {
  const value = entry({ provenance: undefined });
  const before = entrySnapshot(value);
  value.provenance = { sourceIds: ["studio"], originalSources: [] };
  expect(entrySnapshot(value)).toBe(before);
});
