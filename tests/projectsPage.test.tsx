import { screen, fireEvent, waitFor } from "@testing-library/react";
import { expect, it } from "vitest";
import { ProjectsPage } from "@/pages/ProjectsPage";
import {
  calls,
  claudeAgent,
  codexAgent,
  handlers,
  makeGroup,
  makeProject,
  makeSkill,
} from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

function setupSkills() {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("scan_skills", () => [
    makeSkill({
      id: "a",
      name: "Alpha",
      tokens: { skillMd: 400, extras: 100 },
    }),
    makeSkill({ id: "b", name: "Beta", tokens: { skillMd: 1200, extras: 0 } }),
    // 没被任何项目用到，不该出现在合计里
    makeSkill({ id: "c", name: "Gamma", tokens: { skillMd: 900, extras: 0 } }),
  ]);
}

it("项目卡片上的 token 体量按「直接绑定 + 绑定分组」去重后合计", async () => {
  setupSkills();
  handlers.set("list_groups", () => [
    makeGroup({ id: "g1", name: "Dev", skillIds: ["a", "b"] }),
  ]);
  handlers.set("list_projects", () => [
    // a 既单独绑定又在 g1 里 —— 只占一份文件，只该算一次
    makeProject({
      id: "p1",
      name: "webapp",
      agentIds: ["claude-code"],
      skillIds: ["a"],
      groupIds: ["g1"],
    }),
    makeProject({ id: "p2", name: "docs", skillIds: [], groupIds: [] }),
  ]);
  renderWithProviders(<ProjectsPage />);
  // 400 + 100（附带文件）+ 1200，Alpha 不因为"既直接绑定又在组里"被数两遍
  expect(await screen.findByText("≈ 1.7k tokens")).toBeInTheDocument();
  // 什么都没绑的项目不显示这个徽标
  expect(screen.getAllByText(/tokens$/)).toHaveLength(1);
});

it("绑定的所有分组都计入 —— 分组不区分当年用哪个 Agent 创建", async () => {
  setupSkills();
  handlers.set("list_groups", () => [
    makeGroup({ id: "g1", name: "Codex 组", skillIds: ["b"] }),
    makeGroup({ id: "g2", name: "共享组", skillIds: ["c"] }),
  ]);
  handlers.set("list_projects", () => [
    makeProject({
      id: "p1",
      name: "webapp",
      agentIds: ["claude-code"],
      skillIds: ["a"],
      groupIds: ["g1", "g2"],
    }),
  ]);
  renderWithProviders(<ProjectsPage />);
  // a(500) + b(1200) + c(900)，两个分组都计入
  expect(await screen.findByText("≈ 2.6k tokens")).toBeInTheDocument();
});

it("还没有 Agent 启用的项目也按成员显示 token 体量，与分组卡片一致", async () => {
  setupSkills();
  handlers.set("list_groups", () => [
    makeGroup({ id: "g1", name: "Dev", skillIds: ["a", "b"] }),
  ]);
  handlers.set("list_projects", () => [
    makeProject({
      id: "p1",
      name: "webapp",
      agentIds: [],
      skillIds: ["a"],
      groupIds: ["g1"],
    }),
    makeProject({ id: "p2", name: "docs" }),
  ]);
  renderWithProviders(<ProjectsPage />);
  expect(await screen.findByText("≈ 1.7k tokens")).toBeInTheDocument();
  // 没有成员的项目不显示这个徽标
  expect(screen.getAllByText(/tokens$/)).toHaveLength(1);
});

it("选过 Agent 但还没写入的项目显示为未启用；为另一个 Agent 启用时不会顺带写入它", async () => {
  setupSkills();
  const deployed = (agentId: string) => ({
    agentId,
    skillIds: ["a"],
    stale: false,
    issue: null,
  });
  handlers.set("list_projects", () => [
    makeProject({
      id: "both",
      name: "Both",
      agentIds: ["codex", "claude-code"],
      skillIds: ["a"],
      deployments: [deployed("codex"), deployed("claude-code")],
    }),
    // 真实数据里的 note 就是这样：给 Codex 选了 skill，但停用了（没有部署）
    makeProject({
      id: "draft",
      name: "Draft",
      agentIds: ["codex"],
      skillIds: ["a"],
    }),
  ]);
  handlers.set("set_project_agent", () => ({ success: [], failed: [] }));
  renderWithProviders(<ProjectsPage />);
  // 胶囊上的数字是启用了这个 Agent 的项目数
  expect(await screen.findByRole("button", { name: "Codex 1" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Claude Code 1" })).toBeVisible();
  expect(screen.getByText("已启用 1")).toBeVisible();
  expect(
    screen.getByRole("button", { name: "为 Codex 启用「Draft」" }),
  ).toBeVisible();
  fireEvent.click(
    screen.getByRole("button", { name: "为 Claude Code 启用「Draft」" }),
  );
  // 只告诉后端这一个 Agent：写入哪些由后端按实际部署计算，选过没写入的 Codex 不会顺带写入
  await waitFor(() =>
    expect(calls.find((c) => c.command === "set_project_agent")?.args).toEqual({
      projectId: "draft",
      agentId: "claude-code",
      enabled: true,
    }),
  );
});

it("整张项目卡片可点：点卡片进项目详情，原来的铅笔按钮已去掉", async () => {
  setupSkills();
  handlers.set("list_projects", () => [
    makeProject({ id: "p1", name: "webapp", skillIds: [], groupIds: [] }),
  ]);
  renderWithProviders(<ProjectsPage />);
  const transition = (el: HTMLElement) =>
    el.closest<HTMLElement>('[data-transition="detail"]');
  const card = await screen.findByRole("button", { name: "编辑 webapp" });
  // 刚进入页面：页面级过渡已经在淡入，列表不再叠一层（否则起点成了 0.4 × 0.4）
  expect(transition(card)).not.toHaveAttribute("data-appear");
  // 卡片本身就是入口（与 MCP 项目页、Hub 卡片一致）
  fireEvent.click(card);
  // 详情（含标题栏）在过渡容器里，并且这次挂载会播放线性淡入（见 tests/motion.test.ts）
  const back = await screen.findByRole("button", { name: "返回" });
  expect(transition(back)).toHaveAttribute("data-appear");
  expect(transition(screen.getByText("项目已有 skill"))).toBe(transition(back));
  // 返回列表同样淡入
  fireEvent.click(back);
  const again = await screen.findByRole("button", { name: "编辑 webapp" });
  expect(transition(again)).toHaveAttribute("data-appear");
});

it("项目卡片按名字生成图标：不同项目首字不同，不再是统一的文件夹图标", async () => {
  setupSkills();
  handlers.set("list_projects", () => [
    makeProject({ id: "p1", name: "webapp", skillIds: [], groupIds: [] }),
    makeProject({ id: "p2", name: "open_mole", skillIds: [], groupIds: [] }),
  ]);
  renderWithProviders(<ProjectsPage />);
  await screen.findByText("webapp");
  expect(screen.getByText("W")).toBeInTheDocument();
  expect(screen.getByText("O")).toBeInTheDocument();
  expect(screen.getByText("W").parentElement?.className).toMatch(
    /bg-\S+ text-/,
  );
});
