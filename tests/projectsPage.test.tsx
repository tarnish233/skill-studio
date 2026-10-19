import { screen, fireEvent, waitFor } from "@testing-library/react";
import { expect, it } from "vitest";
import { ProjectsPage } from "@/pages/ProjectsPage";
import {
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

it("还没勾 agent 的项目不显示 token 徽标 —— 此时后端一个文件都不写", async () => {
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
  ]);
  renderWithProviders(<ProjectsPage />);
  expect(await screen.findByText("webapp")).toBeInTheDocument();
  expect(screen.queryByText(/tokens$/)).toBeNull();
});

it("counts deployed projects per agent once, filters cards, and toggles through the card action", async () => {
  setupSkills();
  let enabled = true;
  const entry = {
    skillId: "a",
    sourcePath: "/hub/a",
    targetPath: "/project/.agents/skills/a",
  };
  handlers.set("list_projects", () => [
    makeProject({
      id: "p",
      name: "Both",
      agentIds: ["codex", "claude-code"],
      skillIds: ["a"],
      enabledAgentIds: enabled ? ["codex", "claude-code"] : [],
      managedEntries: enabled ? [entry] : [],
    }),
    makeProject({
      id: "draft",
      name: "Draft",
      agentIds: ["codex"],
      skillIds: ["a"],
      enabledAgentIds: [],
    }),
  ]);
  handlers.set("set_project_enabled", (args) => {
    enabled = args.enabled;
    return { succeeded: [], failed: [] };
  });
  renderWithProviders(<ProjectsPage />);
  // 总数在「全部」上，右侧不再重复一枚「项目 N 个」
  expect(
    await screen.findByRole("button", { name: "全部 2" }),
  ).toBeInTheDocument();
  expect(screen.getByText("已启用 1")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Codex 1" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Claude Code 1" }));
  expect(
    screen.queryByRole("button", { name: "Draft" }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Claude Code 1" }));
  fireEvent.click(screen.getByRole("button", { name: "停用" }));
  await waitFor(() => expect(screen.getByText("已启用 0")).toBeInTheDocument());
  expect(
    screen.getByRole("button", { name: "拖拽排序 Both" }),
  ).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: "启用" })).toHaveLength(2);
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
