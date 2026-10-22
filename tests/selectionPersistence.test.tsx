import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { expect, it } from "vitest";
import { ProjectsPage } from "@/pages/ProjectsPage";
import {
  handlers,
  makeProject,
  makeSkill,
  makeGroup,
  claudeAgent,
  codexAgent,
  calls,
} from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const updates = () => calls.filter((c) => c.command === "save_project");
type Members = { skillIds: string[]; groupIds: string[]; linkMode: string };

it("shows repository skills without adding them to Hub bindings", async () => {
  handlers.set("list_projects", () => [makeProject()]);
  handlers.set("list_project_skills", () => [
    {
      name: "skill-studio-release",
      tokens: { skillMd: 208, extras: 100 },
      path: "/project/.agents/skills/skill-studio-release",
      agentId: "codex",
      frontmatter: { description: "发布流程", malformed: false },
    },
  ]);
  renderWithProviders(<ProjectsPage />);
  fireEvent.click(await screen.findByText("webapp"));
  expect(await screen.findByText("skill-studio-release")).toBeVisible();
  expect(screen.getByText("≈ 208 tokens")).toHaveAttribute(
    "title",
    expect.stringContaining("SKILL.md ≈ 208"),
  );
  expect(
    screen.getByText("/project/.agents/skills/skill-studio-release"),
  ).toBeVisible();
  expect(
    screen.queryByRole("checkbox", { name: "skill-studio-release" }),
  ).toBeNull();
  expect(updates()).toHaveLength(0);
});

it("counts uncollected project skills and refreshes collection and confirmed deletion", async () => {
  let collected = false;
  let deleted = false;
  const path = "/project/.agents/skills/release";
  handlers.set("list_projects", () => [
    makeProject({ uncollectedSkillCount: collected || deleted ? 0 : 1 }),
  ]);
  handlers.set("list_project_skills", () =>
    deleted
      ? []
      : [
          {
            name: "release",
            path,
            agentId: "codex",
            collected,
            managed: false,
            frontmatter: { description: "Release", malformed: false },
          },
        ],
  );
  handlers.set("collect_project_skill", (args) => {
    expect(args.path).toBe(path);
    collected = true;
    return makeSkill();
  });
  handlers.set("delete_project_local_skill", (args) => {
    expect(args).toEqual({ projectId: "p1", path });
    deleted = true;
    return "/backup/release";
  });
  renderWithProviders(<ProjectsPage />);
  // 卡片上一枚、统计栏一枚
  expect(await screen.findAllByText("未托管 1")).toHaveLength(2);
  fireEvent.click(screen.getByText("webapp"));
  fireEvent.click(
    await screen.findByRole("button", { name: "托管 release 到 Hub" }),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "托管 release 到 Hub" }),
    ).toBeDisabled(),
  );
  fireEvent.click(screen.getByRole("button", { name: "删除 release" }));
  expect(
    calls.filter((c) => c.command === "delete_project_local_skill"),
  ).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "取消" }));
  expect(
    calls.filter((c) => c.command === "delete_project_local_skill"),
  ).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "删除 release" }));
  fireEvent.click(screen.getByRole("button", { name: "删除" }));
  expect(await screen.findByText("项目目录中暂无 skill")).toBeVisible();
  expect(updates()).toHaveLength(0);
});

it("keeps selections local and saves once, writing the Agents already enabled", async () => {
  const deployed = (agentId: string) => ({
    agentId,
    skillIds: [],
    stale: false,
    issue: null,
  });
  let project = makeProject({
    deployments: [deployed("claude-code"), deployed("codex")],
  });
  handlers.set("list_projects", () => [project]);
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("list_groups", () => [
    makeGroup({ id: "g1", name: "Group A" }),
    makeGroup({ id: "g2", name: "Group B" }),
  ]);
  handlers.set("scan_skills", () => [
    makeSkill({ id: "a", name: "Alpha" }),
    makeSkill({ id: "b", name: "Beta" }),
  ]);
  const first = deferred<unknown>();
  let count = 0;
  handlers.set("save_project", async (args) => {
    count++;
    if (count === 1) await first.promise;
    project = { ...project, ...args.members };
    return { success: [], failed: [] };
  });
  renderWithProviders(<ProjectsPage />);
  fireEvent.click(await screen.findByText("webapp"));
  fireEvent.click(await screen.findByRole("checkbox", { name: "Alpha" }));
  expect(updates()).toHaveLength(0);
  fireEvent.click(screen.getByRole("checkbox", { name: "Beta" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Group A" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Group B" }));
  expect(screen.getByRole("checkbox", { name: "Alpha" })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Beta" })).toBeChecked();
  fireEvent.click(screen.getByRole("button", { name: "保存" }));

  await waitFor(() => expect(updates()).toHaveLength(1));
  await act(async () => first.resolve({}));
  await waitFor(() => expect(updates()).toHaveLength(1));
  // 写入哪些 Agent 由后端按当前部署确定；带上打开时的成员，别处改过就不覆盖
  expect(updates().at(-1)!.args).toEqual({
    projectId: "p1",
    members: { skillIds: ["a", "b"], groupIds: ["g1", "g2"], linkMode: "copy" },
    expected: { skillIds: [], groupIds: [], linkMode: "copy" },
  });
});

it("preserves the draft after a failed save, blocks apply, and retries the latest selection", async () => {
  let project = makeProject({ agentIds: ["codex"] });
  handlers.set("list_projects", () => [project]);
  handlers.set("scan_skills", () => [
    makeSkill({ id: "a", name: "Alpha" }),
    makeSkill({ id: "b", name: "Beta" }),
  ]);
  const first = deferred<unknown>();
  let count = 0;
  handlers.set("save_project", async (args) => {
    count++;
    if (count === 1) await first.promise;
    project = { ...project, ...args.members };
    return { success: [], failed: [] };
  });
  renderWithProviders(<ProjectsPage />);
  fireEvent.click(await screen.findByText("webapp"));
  fireEvent.click(await screen.findByRole("checkbox", { name: "Alpha" }));
  expect(updates()).toHaveLength(0);
  fireEvent.click(screen.getByRole("checkbox", { name: "Beta" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await act(async () => first.reject(new Error("disk unavailable")));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "disk unavailable",
  );
  expect(screen.getByRole("checkbox", { name: "Alpha" })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Beta" })).toBeChecked();

  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(updates()).toHaveLength(2));
  expect((updates()[1].args as { members: Members }).members.skillIds).toEqual([
    "a",
    "b",
  ]);
});

it("confirms unsaved navigation, supports cancel and discard without saving", async () => {
  handlers.set("list_projects", () => [makeProject({ agentIds: ["codex"] })]);
  handlers.set("scan_skills", () => [makeSkill({ id: "a", name: "Alpha" })]);
  renderWithProviders(<ProjectsPage />);
  fireEvent.click(await screen.findByText("webapp"));
  fireEvent.click(await screen.findByRole("checkbox", { name: "Alpha" }));
  expect(updates()).toHaveLength(0);
  expect(screen.queryByText("选择会自动保存")).toBeNull();
  const unload = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(unload);
  expect(unload.defaultPrevented).toBe(true);
  fireEvent.click(screen.getByTitle("返回"));
  expect(await screen.findByRole("dialog")).toHaveTextContent(
    "放弃未保存的修改",
  );
  fireEvent.click(screen.getByRole("button", { name: "继续编辑" }));
  expect(screen.getByRole("checkbox", { name: "Alpha" })).toBeChecked();
  fireEvent.click(screen.getByTitle("返回"));
  fireEvent.click(screen.getByRole("button", { name: "放弃修改并离开" }));
  expect(updates()).toHaveLength(0);
  fireEvent.click(await screen.findByText("webapp"));
  expect(
    await screen.findByRole("checkbox", { name: "Alpha" }),
  ).not.toBeChecked();
});

it("does not prompt after successful write", async () => {
  let project = makeProject({ agentIds: ["codex"] });
  handlers.set("list_projects", () => [project]);
  handlers.set("scan_skills", () => [makeSkill({ id: "a", name: "Alpha" })]);
  handlers.set("save_project", (args) => {
    project = { ...project, ...args.members };
    return { success: [], failed: [] };
  });
  renderWithProviders(<ProjectsPage />);
  fireEvent.click(await screen.findByText("webapp"));
  fireEvent.click(await screen.findByRole("checkbox", { name: "Alpha" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  // 保存完成：没有未保存的修改，按钮回到「保存」并禁用
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled(),
  );
  fireEvent.click(screen.getByTitle("返回"));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(updates()).toHaveLength(1);
});
