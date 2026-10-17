import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { timeoutManager } from "@tanstack/react-query";
import { afterEach, expect, it } from "vitest";
import { LibraryPage } from "@/pages/LibraryPage";
import { ProjectsPage } from "@/pages/ProjectsPage";
import { setTarget } from "@/lib/api/transport";
import { renderWithProviders } from "./utils/render";
import { handlers, calls, makeSkill, makeProject } from "./mocks/tauri";

const systemTimers = {
  setTimeout: (cb: () => void, delay: number) =>
    setTimeout(cb, delay) as unknown as number,
  clearTimeout: (id: number | undefined) => clearTimeout(id),
  setInterval: (cb: () => void, delay: number) =>
    setInterval(cb, delay) as unknown as number,
  clearInterval: (id: number | undefined) => clearInterval(id),
};
afterEach(() => {
  setTarget({ id: "local", name: "本机", connected: true });
  timeoutManager.setTimeoutProvider(systemTimers);
});

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const doc = (body: string) => ({
  raw: `---\nname: x\n---\n${body}`,
  frontmatter: {
    name: "x",
    description: null,
    extraKeys: [],
    malformed: false,
  },
  body,
});
const readCount = () =>
  calls.filter((c) => c.command === "read_skill_document").length;

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
it("先读完文档再开预览弹窗：不会先弹出小窗、内容到达后再长高", async () => {
  handlers.set("scan_skills", () => [makeSkill()]);
  handlers.set("read_skill_document", () => ({
    raw: "---\nname: pdf-tools\n---\n# Demo",
    frontmatter: {
      name: "pdf-tools",
      description: null,
      extraKeys: [],
      malformed: false,
    },
    body: "# Demo",
  }));
  renderWithProviders(<LibraryPage />);
  fireEvent.keyDown(
    await screen.findByRole("button", { name: "预览 pdf-tools" }),
    { key: "Enter" },
  );
  // 点击这一帧弹窗还不存在（文档是在开窗之前读完的），因此不会出现
  // 「读取中…」的小窗随后被正文撑高的那一下闪烁
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(await screen.findByText("# Demo")).toBeVisible();
  expect(screen.queryByText("读取中…")).not.toBeInTheDocument();
  // 预取 + 弹窗共用同一份缓存：只读一次，避免正文被重新渲染
  await waitFor(() => expect(readCount()).toBe(1));
  // 关掉再打开要重新读文件（缓存只在弹窗打开期间复用）
  fireEvent.click(screen.getByRole("button", { name: "关闭" }));
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  fireEvent.keyDown(screen.getByRole("button", { name: "预览 pdf-tools" }), {
    key: "Enter",
  });
  expect(await screen.findByText("# Demo")).toBeVisible();
  await waitFor(() => expect(readCount()).toBe(2));
});
it("缓存回收抢在弹窗订阅之前也只读一次：开窗后不会再闪出「读取中」", async () => {
  // 浏览器里 setTimeout(0) 和 React 的调度谁先跑没有保证；这里让 gcTime: 0 的回收
  // 一律抢先，模拟最坏顺序
  timeoutManager.setTimeoutProvider({
    ...systemTimers,
    setTimeout: (cb, delay) => {
      if (delay !== 0) return systemTimers.setTimeout(cb, delay);
      queueMicrotask(cb);
      return -1;
    },
  });
  handlers.set("scan_skills", () => [makeSkill()]);
  handlers.set("read_skill_document", () => doc("# Demo"));
  renderWithProviders(<LibraryPage />);
  fireEvent.keyDown(
    await screen.findByRole("button", { name: "预览 pdf-tools" }),
    { key: "Enter" },
  );
  let loadingInDialog = false;
  const watch = new MutationObserver(() => {
    if (screen.queryByRole("dialog") && screen.queryByText("读取中…"))
      loadingInDialog = true;
  });
  watch.observe(document.body, { subtree: true, childList: true });
  expect(await screen.findByText("# Demo")).toBeVisible();
  await sleep(30);
  watch.disconnect();
  expect({ reads: readCount(), loadingInDialog }).toEqual({
    reads: 1,
    loadingInDialog: false,
  });
});

it("读得慢时先开窗显示读取中；这时关掉，文档读完也不会自己再弹出来", async () => {
  handlers.set("scan_skills", () => [makeSkill()]);
  let finish!: () => void;
  handlers.set(
    "read_skill_document",
    () => new Promise((resolve) => (finish = () => resolve(doc("# Slow")))),
  );
  renderWithProviders(<LibraryPage />);
  fireEvent.keyDown(
    await screen.findByRole("button", { name: "预览 pdf-tools" }),
    { key: "Enter" },
  );
  expect(await screen.findByText("读取中…")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "关闭" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  finish();
  await sleep(30);
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("只认最后一次点击：先点的 skill 读得慢，也不会盖掉后点的那个", async () => {
  handlers.set("scan_skills", () => [
    makeSkill({ id: "a", name: "alpha", sourcePath: "/s/alpha" }),
    makeSkill({ id: "b", name: "beta", sourcePath: "/s/beta" }),
  ]);
  handlers.set("read_skill_document", async ({ path }: { path: string }) => {
    if (path.endsWith("alpha")) await sleep(200);
    return doc(path.endsWith("alpha") ? "# A" : "# B");
  });
  renderWithProviders(<LibraryPage />);
  fireEvent.keyDown(await screen.findByRole("button", { name: "预览 alpha" }), {
    key: "Enter",
  });
  fireEvent.keyDown(screen.getByRole("button", { name: "预览 beta" }), {
    key: "Enter",
  });
  expect(await screen.findByText("# B")).toBeVisible();
  await sleep(250);
  expect(screen.queryByText("# A")).toBeNull();
  expect(screen.getByRole("dialog")).toHaveTextContent("beta");
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
