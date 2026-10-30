import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { toast } from "sonner";
import { LibraryPage } from "@/pages/LibraryPage";
import {
  agentState,
  calls,
  claudeAgent,
  codexAgent,
  handlers,
  makeSkill,
} from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

const sharedPath = "/home/u/.agents/skills/rex";
const claudePath = "/home/u/.claude/skills/rex";

function rexSources() {
  return [
    makeSkill({
      id: "shared-rex",
      name: "rex",
      sourcePath: sharedPath,
      root: "/home/u/.agents/skills",
      sourceIds: ["agent"],
      origin: { kind: "inPlace", ownerAgent: "codex" },
      agents: { codex: agentState("source") },
    }),
    makeSkill({
      id: "claude-rex",
      name: "rex",
      sourcePath: claudePath,
      sourceIds: ["claude-code"],
    }),
  ];
}

function setup(skills = rexSources()) {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  // 与 IPC 一样，每次扫描返回独立快照，不能原地修改 React Query 缓存。
  handlers.set("scan_skills", () => [...skills]);
}

const actionCalls = (command: string) =>
  calls.filter((call) => call.command === command);
const sourceRow = (path: string) =>
  within(screen.getByRole("group", { name: `来源 ${path}` }));

it("多来源卡片保留托管入口，选择并确认后只托管选中的来源", async () => {
  setup();
  const user = userEvent.setup();
  renderWithProviders(<LibraryPage />);

  await user.click(
    await screen.findByRole("button", { name: "托管 rex 到 Hub" }),
  );
  expect(screen.getByRole("dialog", { name: "rex · 来源" })).toHaveTextContent(
    "只处理选中的来源",
  );
  expect(screen.getAllByRole("button", { name: "托管此来源" })).toHaveLength(2);
  expect(actionCalls("adopt_to_hub")).toHaveLength(0);

  // 选择排序靠后的 Claude 来源，不能误用代表卡片的共享来源 ID。
  await user.click(
    sourceRow(claudePath).getByRole("button", { name: "托管此来源" }),
  );
  const confirm = screen.getByRole("dialog", { name: "托管到 Hub" });
  expect(confirm).toHaveTextContent(claudePath);
  expect(confirm).not.toHaveTextContent(sharedPath);
  expect(actionCalls("adopt_to_hub")).toHaveLength(0);
  await user.click(within(confirm).getByRole("button", { name: "托管" }));
  await waitFor(() =>
    expect(actionCalls("adopt_to_hub")).toEqual([
      { command: "adopt_to_hub", args: { skillId: "claude-rex" } },
    ]),
  );
  expect(actionCalls("delete_skill_file")).toHaveLength(0);
});

it("多来源卡片只显示一个删除入口，取消不删除、确认只删除选中的路径", async () => {
  const skills = rexSources();
  setup(skills);
  handlers.set("delete_skill_file", ({ path }) => {
    const index = skills.findIndex((skill) => skill.sourcePath === path);
    skills.splice(index, 1);
  });
  const user = userEvent.setup();
  renderWithProviders(<LibraryPage />);
  await screen.findByText("rex");
  expect(screen.getAllByRole("button", { name: "删除 rex" })).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: "删除 rex" }));
  expect(screen.getByRole("dialog", { name: "rex · 来源" })).toBeVisible();
  expect(actionCalls("delete_skill_file")).toHaveLength(0);

  const selectClaude = () =>
    user.click(
      sourceRow(claudePath).getByRole("button", {
        name: `删除 rex 的来源 ${claudePath}`,
      }),
    );
  await selectClaude();
  let confirm = screen.getByRole("dialog", { name: "删除 rex？" });
  expect(confirm).toHaveTextContent(claudePath);
  expect(confirm).not.toHaveTextContent(sharedPath);
  expect(confirm).toHaveTextContent("非软链接文件会保留备份");
  expect(actionCalls("delete_skill_file")).toHaveLength(0);
  await user.click(within(confirm).getByRole("button", { name: "取消" }));
  expect(actionCalls("delete_skill_file")).toHaveLength(0);

  await selectClaude();
  confirm = screen.getByRole("dialog", { name: "删除 rex？" });
  await user.click(within(confirm).getByRole("button", { name: "确认" }));
  await waitFor(() =>
    expect(actionCalls("delete_skill_file")).toEqual([
      {
        command: "delete_skill_file",
        args: { scope: "agent:claude-code", path: claudePath },
      },
    ]),
  );
  await waitFor(() =>
    expect(screen.queryByRole("dialog", { name: "删除 rex？" })).toBeNull(),
  );
  expect(sourceRow(sharedPath).getByText(sharedPath)).toBeVisible();
  expect(screen.queryByText(claudePath)).toBeNull();
  expect(actionCalls("adopt_to_hub")).toHaveLength(0);
});

it.each([
  { label: "共享目录", source: rexSources()[0], scope: "agent:codex" },
  {
    label: "Hub",
    source: makeSkill({
      id: "hub-rex",
      name: "rex",
      sourcePath: "/home/u/.skill-studio/skills/rex",
      origin: { kind: "hub" },
      sourceIds: ["studio"],
      agents: {},
    }),
    scope: "hub",
  },
])(
  "$label 来源的删除保留正确作用域，不误删其他副本",
  async ({ source, scope }) => {
    setup([source, rexSources()[1]]);
    const user = userEvent.setup();
    renderWithProviders(<LibraryPage />);
    await user.click(await screen.findByRole("button", { name: "删除 rex" }));
    await user.click(
      sourceRow(source.sourcePath).getByRole("button", {
        name: `删除 rex 的来源 ${source.sourcePath}`,
      }),
    );
    await user.click(screen.getByRole("button", { name: "确认" }));
    await waitFor(() =>
      expect(actionCalls("delete_skill_file")).toEqual([
        {
          command: "delete_skill_file",
          args: { scope, path: source.sourcePath },
        },
      ]),
    );
  },
);

it("单来源仍直接打开托管或删除确认，不增加来源选择步骤", async () => {
  setup([rexSources()[1]]);
  const user = userEvent.setup();
  renderWithProviders(<LibraryPage />);
  await user.click(
    await screen.findByRole("button", { name: "托管 rex 到 Hub" }),
  );
  expect(screen.getByRole("dialog", { name: "托管到 Hub" })).toHaveTextContent(
    claudePath,
  );
  expect(screen.queryByRole("dialog", { name: "rex · 来源" })).toBeNull();
  await user.click(screen.getByRole("button", { name: "取消" }));
  expect(actionCalls("adopt_to_hub")).toHaveLength(0);
  await user.click(screen.getByRole("button", { name: "删除 rex" }));
  expect(screen.getByRole("dialog", { name: "删除 rex？" })).toHaveTextContent(
    claudePath,
  );
  expect(screen.queryByRole("dialog", { name: "rex · 来源" })).toBeNull();
});

function protectedSources() {
  return [
    makeSkill({
      id: "builtin",
      name: "rex",
      sourcePath: "/a/builtin/rex",
      origin: { kind: "hub" },
      sourceIds: ["builtin"],
      agents: {},
    }),
    makeSkill({
      id: "external",
      name: "rex",
      sourcePath: "/b/external/rex",
      origin: { kind: "external" },
      sourceIds: ["external"],
      agents: {},
    }),
  ];
}

it("按全部来源决定入口是否显示，内置与外部来源仍不能删除或托管", async () => {
  const protectedSkills = protectedSources();
  setup([...protectedSkills, rexSources()[1]]);
  const user = userEvent.setup();
  renderWithProviders(<LibraryPage />);
  // 卡片的代表来源是内置项，但仍需提供另一个来源的托管与删除入口。
  await screen.findByRole("button", { name: "托管 rex 到 Hub" });
  expect(screen.getAllByRole("button", { name: "删除 rex" })).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: "删除 rex" }));
  for (const source of protectedSkills) {
    expect(
      sourceRow(source.sourcePath).queryByRole("button", {
        name: /删除|托管此来源/,
      }),
    ).toBeNull();
  }
  expect(
    sourceRow(claudePath).getByRole("button", { name: "托管此来源" }),
  ).toBeVisible();
  expect(
    sourceRow(claudePath).getByRole("button", {
      name: `删除 rex 的来源 ${claudePath}`,
    }),
  ).toHaveTextContent("删除此来源");
});

it("没有可操作的来源时，不出现误导性的托管或删除入口", async () => {
  setup(protectedSources());
  const user = userEvent.setup();
  renderWithProviders(<LibraryPage />);
  await screen.findByText("rex");
  expect(screen.queryByRole("button", { name: "托管 rex 到 Hub" })).toBeNull();
  expect(screen.queryByRole("button", { name: "删除 rex" })).toBeNull();
  await user.click(screen.getByRole("button", { name: "查看 rex 的来源" }));
  expect(screen.queryByRole("button", { name: /删除|托管此来源/ })).toBeNull();
});

it("删除被后端拒绝时保留确认，不改为删除其他来源", async () => {
  setup();
  handlers.set("delete_skill_file", () => {
    throw new Error("此来源仍被分组使用");
  });
  const error = vi.spyOn(toast, "error");
  const user = userEvent.setup();
  renderWithProviders(<LibraryPage />);
  await user.click(await screen.findByRole("button", { name: "删除 rex" }));
  await user.click(
    sourceRow(claudePath).getByRole("button", {
      name: `删除 rex 的来源 ${claudePath}`,
    }),
  );
  await user.click(screen.getByRole("button", { name: "确认" }));
  await waitFor(() =>
    expect(error).toHaveBeenCalledWith(
      expect.stringContaining("此来源仍被分组使用"),
    ),
  );
  expect(screen.getByRole("dialog", { name: "删除 rex？" })).toHaveTextContent(
    claudePath,
  );
  expect(actionCalls("delete_skill_file")).toEqual([
    {
      command: "delete_skill_file",
      args: { scope: "agent:claude-code", path: claudePath },
    },
  ]);
  error.mockRestore();
});
