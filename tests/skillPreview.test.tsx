import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { LibraryPage } from "@/pages/LibraryPage";
import { AgentSkills } from "@/pages/AgentPage";
import { ProjectsPage } from "@/pages/ProjectsPage";
import { setTarget } from "@/lib/api/transport";
import { renderWithProviders } from "./utils/render";
import {
  handlers,
  calls,
  makeSkill,
  makeProject,
  claudeAgent,
} from "./mocks/tauri";

afterEach(() => setTarget({ id: "local", name: "本机", connected: true }));

it("Hub card previews the document body with its frontmatter summary and action buttons do not open preview", async () => {
  handlers.set("scan_skills", () => [makeSkill()]);
  handlers.set("read_skill_document", () => ({
    raw: "---\nname: pdf-tools\ndescription: 预览描述\ncontext: fork\n---\n# Demo\n<script>unsafe</script>",
    frontmatter: {
      name: "pdf-tools",
      description: "预览描述",
      extraKeys: ["context"],
      malformed: false,
    },
    body: "# Demo\n<script>unsafe</script>",
  }));
  renderWithProviders(<LibraryPage />);
  const card = await screen.findByRole("button", { name: "预览 pdf-tools" });
  const buttons = within(card).getAllByRole("button");
  expect(buttons.at(-1)).toHaveAccessibleName("删除 pdf-tools");
  fireEvent.click(
    screen.getByRole("button", { name: "打开 pdf-tools 所在目录" }),
  );
  expect(calls.some((c) => c.command === "read_skill_document")).toBe(false);
  fireEvent.keyDown(card, { key: "Enter" });
  expect(await screen.findByText(/<script>unsafe<\/script>/)).toBeVisible();
  expect(screen.getByText("预览描述")).toBeVisible();
  expect(screen.getByText(/其他 frontmatter 字段：context/)).toBeVisible();
  expect(document.querySelector("script")).toBeNull();
});
it("preview opens SKILL.md with the default application on the local target", async () => {
  handlers.set("scan_skills", () => [makeSkill()]);
  handlers.set("read_skill_document", () => ({
    raw: "body",
    frontmatter: {},
    body: "body",
  }));
  renderWithProviders(<LibraryPage />);
  fireEvent.keyDown(
    await screen.findByRole("button", { name: "预览 pdf-tools" }),
    { key: "Enter" },
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "用默认应用打开" }),
  );
  await waitFor(() =>
    expect(
      calls.find((c) => c.command === "open_skill_document")?.args,
    ).toEqual({ path: "/home/u/.claude/skills/pdf-tools" }),
  );
});
it("preview disables opening with a local app for remote targets", async () => {
  handlers.set("remote_request", ({ method }: { method: string }) => {
    if (method === "scan_skills") return [makeSkill()];
    if (method === "read_skill_document")
      return { raw: "body", frontmatter: {}, body: "body" };
    // 页面其他查询（备份、Agent、分组）走远程时只当空列表
    return [];
  });
  setTarget({ id: "node2", name: "node2", connected: true });
  renderWithProviders(<LibraryPage />);
  fireEvent.keyDown(
    await screen.findByRole("button", { name: "预览 pdf-tools" }),
    { key: "Enter" },
  );
  expect(
    await screen.findByRole("button", { name: "用默认应用打开" }),
  ).toBeDisabled();
  expect(screen.getByText("文件在 node2 上，无法用本机应用打开")).toBeVisible();
  expect(calls.some((c) => c.command === "open_skill_document")).toBe(false);
});
it("preview falls back to plain text from an older remote helper", async () => {
  handlers.set("scan_skills", () => [makeSkill()]);
  handlers.set("read_skill_document", () => "plain text from helper");
  renderWithProviders(<LibraryPage />);
  fireEvent.keyDown(
    await screen.findByRole("button", { name: "预览 pdf-tools" }),
    { key: "Enter" },
  );
  expect(await screen.findByText("plain text from helper")).toBeVisible();
});
it("preview warns about malformed frontmatter and an empty body", async () => {
  handlers.set("scan_skills", () => [makeSkill()]);
  handlers.set("read_skill_document", () => ({
    raw: "---\nname: [unclosed\n---\n",
    frontmatter: {
      malformed: true,
      error: "did not find expected node content",
    },
    body: "",
  }));
  renderWithProviders(<LibraryPage />);
  fireEvent.keyDown(
    await screen.findByRole("button", { name: "预览 pdf-tools" }),
    { key: "Enter" },
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "frontmatter 解析失败：did not find expected node content",
  );
  expect(screen.getByText("此文档只有 frontmatter，没有正文")).toBeVisible();
});
it("Agent preview reads installed path and displays missing-file errors", async () => {
  handlers.set("list_agents", () => [claudeAgent]);
  const skill = makeSkill();
  skill.agents["claude-code"].targetPath = "/installed/pdf-tools";
  handlers.set("scan_skills", () => [skill]);
  handlers.set("read_skill_document", (args) => {
    expect(args.path).toBe("/installed/pdf-tools");
    throw Error("文件不存在");
  });
  renderWithProviders(<AgentSkills agentId="claude-code" />);
  fireEvent.click(
    await screen.findByRole("button", { name: "预览 pdf-tools" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent("文件不存在");
});
it("project preview uses suspended storage and delete remains last without triggering preview", async () => {
  handlers.set("list_projects", () => [makeProject()]);
  handlers.set("list_project_skills", () => [
    {
      name: "local",
      path: "/project/local",
      storagePath: "/backup/payload",
      agentId: "codex",
      disabled: true,
      collected: false,
      managed: false,
      frontmatter: { malformed: false },
    },
  ]);
  handlers.set("read_skill_document", () => ({
    raw: "local document",
    frontmatter: {},
    body: "local document",
  }));
  renderWithProviders(<ProjectsPage />);
  fireEvent.click(await screen.findByText("webapp"));
  const card = await screen.findByRole("button", { name: "预览 local" });
  expect(within(card).getAllByRole("button").at(-1)).toHaveAccessibleName(
    "删除 local",
  );
  fireEvent.click(screen.getByRole("button", { name: "删除 local" }));
  expect(calls.some((c) => c.command === "read_skill_document")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "取消" }));
  fireEvent.click(card);
  await waitFor(() =>
    expect(
      calls.find((c) => c.command === "read_skill_document")?.args,
    ).toEqual({ path: "/backup/payload" }),
  );
  expect(await screen.findByText("local document")).toBeVisible();
});
