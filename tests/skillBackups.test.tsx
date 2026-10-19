import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, it } from "vitest";
import {
  SkillBackups,
  DeleteSkillButton,
} from "@/components/common/SkillBackups";
import { ProjectsPage } from "@/pages/ProjectsPage";
import { handlers, calls, makeProject } from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

it("filters backups and confirms purge while allowing recovery", async () => {
  let records = [
    {
      id: "a",
      name: "agent-demo",
      scope: "agent:codex",
      originalPath: "/codex/demo",
      deletedAt: 1,
    },
    {
      id: "p",
      name: "project-demo",
      scope: "project:p1",
      originalPath: "/project/demo",
      deletedAt: 2,
    },
  ];
  handlers.set("list_skill_backups", () => records);
  handlers.set("restore_skill_file", () => {
    throw Error("原位置已有内容");
  });
  handlers.set("purge_skill_file", ({ id }) => {
    records = records.filter((r) => r.id !== id);
  });
  renderWithProviders(<SkillBackups scope="agent:codex" />);
  fireEvent.click(await screen.findByText("已备份 1"));
  expect(screen.getByText("agent-demo")).toBeVisible();
  expect(screen.queryByText("project-demo")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "恢复" }));
  await waitFor(() =>
    expect(calls.some((c) => c.command === "restore_skill_file")).toBe(true),
  );
  expect(screen.getByText("agent-demo")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "彻底删除" }));
  expect(calls.filter((c) => c.command === "purge_skill_file")).toHaveLength(0);
  const dialog = screen.getByRole("dialog", { name: "彻底删除备份？" });
  fireEvent.click(within(dialog).getByRole("button", { name: "确认" }));
  await waitFor(() => expect(screen.queryByText("agent-demo")).toBeNull());
});
it("deletion waits for confirmation", async () => {
  handlers.set("delete_skill_file", () => undefined);
  renderWithProviders(
    <DeleteSkillButton scope="hub" path="/hub/demo" name="demo" />,
  );
  fireEvent.click(screen.getByRole("button", { name: "删除 demo" }));
  expect(calls.some((c) => c.command === "delete_skill_file")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "确认" }));
  await waitFor(() =>
    expect(calls.find((c) => c.command === "delete_skill_file")?.args).toEqual({
      scope: "hub",
      path: "/hub/demo",
    }),
  );
});
it("project local skill can be disabled and re-enabled without Hub collection", async () => {
  let disabled = false;
  handlers.set("list_projects", () => [makeProject()]);
  handlers.set("list_project_skills", () => [
    {
      name: "demo",
      path: "/project/.agents/skills/demo",
      storagePath: "/stored/demo",
      agentId: "codex",
      disabled,
      collected: false,
      managed: false,
      frontmatter: { malformed: false },
    },
  ]);
  handlers.set("set_project_skill_enabled", (args) => {
    disabled = !disabled;
    expect(args.enabled).toBe(!disabled);
  });
  renderWithProviders(<ProjectsPage />);
  fireEvent.click(await screen.findByText("webapp"));
  fireEvent.click(await screen.findByRole("button", { name: "停用" }));
  fireEvent.click(await screen.findByRole("button", { name: "启用" }));
  await screen.findByRole("button", { name: "停用" });
  expect(
    calls.filter((c) => c.command === "set_project_skill_enabled"),
  ).toHaveLength(2);
  expect(calls.some((c) => c.command === "collect_project_skill")).toBe(false);
});
