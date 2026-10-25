import { fireEvent, screen, waitFor } from "@testing-library/react";
import { expect, it } from "vitest";
import { InstallSkillsPage } from "@/pages/InstallSkillsPage";
import { LibraryPage } from "@/pages/LibraryPage";
import { handlers, calls, makeSkill } from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";
import type { SkillView } from "@/types";

const hit = {
  skillId: "demo",
  name: "Demo",
  source: "owner/repo",
  installs: 1234,
};
function search() {
  fireEvent.change(screen.getByRole("textbox", { name: "搜索 skills.sh" }), {
    target: { value: "demo" },
  });
  fireEvent.click(screen.getByRole("button", { name: "搜索技能" }));
}

it("requires two characters, installs the selected result and refreshes Studio origin", async () => {
  let skills: SkillView[] = [];
  handlers.set("scan_skills", () => skills);
  handlers.set("search_catalog_skills", () => [hit]);
  handlers.set("install_catalog_skill", () => {
    skills = [
      makeSkill({
        name: "demo",
        origin: { kind: "hub" },
        sourceIds: ["studio"],
        installation: {
          source: hit.source,
          skillId: hit.skillId,
          repositoryPath: "skills/demo",
          installedAt: 123,
          contentHash: "hash",
        },
      }),
    ];
    return { status: "installed", skill: skills[0] };
  });
  const view = renderWithProviders(<InstallSkillsPage />);
  expect(screen.getByRole("button", { name: "搜索技能" })).toBeDisabled();
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "a" } });
  expect(screen.getByRole("button", { name: "搜索技能" })).toBeDisabled();
  search();
  expect(await screen.findByText("Demo")).toBeInTheDocument();
  expect(screen.getByText("owner/repo")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "安装" }));
  expect(await screen.findByRole("button", { name: "已安装" })).toBeDisabled();
  expect(calls).toContainEqual({
    command: "install_catalog_skill",
    args: {
      source: "owner/repo",
      skillId: "demo",
      repositoryPath: null,
      repositoryPaths: null,
    },
  });
  view.unmount();
  renderWithProviders(<LibraryPage />);
  expect(
    await screen.findByRole("img", { name: "来源：Skill Studio" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: /Skill Studio.*1/ }),
  ).toBeInTheDocument();
});

it("shows search errors, retries and reports empty results", async () => {
  handlers.set("search_catalog_skills", () => {
    throw new Error("offline");
  });
  renderWithProviders(<InstallSkillsPage />);
  search();
  expect(await screen.findByRole("alert")).toHaveTextContent("offline");
  handlers.set("search_catalog_skills", () => []);
  fireEvent.click(screen.getByRole("button", { name: "重试" }));
  expect(await screen.findByText(/没有找到匹配/)).toBeInTheDocument();
});

it("failed installs remain retryable and do not show installed", async () => {
  handlers.set("search_catalog_skills", () => [hit]);
  handlers.set("install_catalog_skill", () => {
    throw new Error("Hub 已有同名 skill");
  });
  renderWithProviders(<InstallSkillsPage />);
  search();
  fireEvent.click(await screen.findByRole("button", { name: "安装" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Hub 已有同名 skill",
  );
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "安装" })).toBeEnabled(),
  );
  expect(screen.queryByRole("button", { name: "已安装" })).toBeNull();
});

it("chooses one source per ambiguous variant scope while retaining unique Agent variants", async () => {
  handlers.set("search_catalog_skills", () => [hit]);
  handlers.set("install_catalog_skill", (args) =>
    args.repositoryPaths
      ? { status: "installed", skill: makeSkill() }
      : {
          status: "selectionRequired",
          candidates: [
            {
              repositoryPath: "skills/demo",
              variantKey: "generic",
              description: "通用 A",
              contentHash: "a",
            },
            {
              repositoryPath: ".agents/skills/demo",
              variantKey: "generic",
              description: "通用 B",
              contentHash: "b",
            },
            {
              repositoryPath: ".codex/skills/demo",
              variantKey: "codex",
              description: "Codex",
              contentHash: "c",
            },
          ],
        },
  );
  renderWithProviders(<InstallSkillsPage />);
  search();
  fireEvent.click(await screen.findByRole("button", { name: "安装" }));
  const radios = await screen.findAllByRole("radio");
  expect(radios[2]).toBeChecked();
  expect(screen.getByRole("button", { name: "安装所选版本" })).toBeDisabled();
  fireEvent.click(radios[0]);
  fireEvent.click(screen.getByRole("button", { name: "安装所选版本" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(calls).toContainEqual({
    command: "install_catalog_skill",
    args: {
      source: hit.source,
      skillId: hit.skillId,
      repositoryPath: null,
      repositoryPaths: [".codex/skills/demo", "skills/demo"],
    },
  });
});

it("disables invalid candidates and requires confirmation before installing healthy scopes", async () => {
  handlers.set("search_catalog_skills", () => [hit]);
  handlers.set("install_catalog_skill", (args) =>
    args.repositoryPaths
      ? { status: "installed", skill: makeSkill() }
      : {
          status: "selectionRequired",
          candidates: [
            {
              repositoryPath: ".codex/skills/demo",
              variantKey: "codex",
              contentHash: "",
              error: ".codex/skills/demo：YAML 无效",
            },
            {
              repositoryPath: "skills/demo",
              variantKey: "generic",
              contentHash: "ok",
            },
          ],
        },
  );
  renderWithProviders(<InstallSkillsPage />);
  search();
  fireEvent.click(await screen.findByRole("button", { name: "安装" }));
  const radios = await screen.findAllByRole("radio");
  expect(radios[0]).toBeDisabled();
  expect(radios[0]).not.toBeChecked();
  expect(radios[1]).toBeChecked();
  expect(screen.getByRole("alert")).toHaveTextContent("部分目录不可用");
  expect(screen.getByRole("dialog")).toHaveTextContent(
    ".codex/skills/demo：YAML 无效",
  );
  expect(
    calls.filter((c) => c.command === "install_catalog_skill"),
  ).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "安装所选版本" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(calls).toContainEqual({
    command: "install_catalog_skill",
    args: {
      source: hit.source,
      skillId: hit.skillId,
      repositoryPath: null,
      repositoryPaths: ["skills/demo"],
    },
  });
});

const candidates = [
  {
    repositoryPath: ".claude/skills/demo",
    description: "Claude Code 版本",
    contentHash: "cc",
  },
  {
    repositoryPath: ".codex/skills/demo",
    description: "Codex 版本",
    contentHash: "codex",
  },
];

it("asks for an explicit candidate, sends its path and retries failures in the dialog", async () => {
  handlers.set("search_catalog_skills", () => [hit]);
  handlers.set("install_catalog_skill", () => ({
    status: "selectionRequired",
    candidates,
  }));
  renderWithProviders(<InstallSkillsPage />);
  search();
  fireEvent.click(await screen.findByRole("button", { name: "安装" }));
  expect(await screen.findByRole("dialog")).toHaveTextContent("文件内容不同");
  expect(screen.getByRole("button", { name: "安装所选版本" })).toBeDisabled();
  expect(screen.queryByRole("alert")).toBeNull();
  fireEvent.click(screen.getByRole("radio", { name: /\.codex\/skills\/demo/ }));
  handlers.set("install_catalog_skill", () => {
    throw new Error("下载失败");
  });
  fireEvent.click(screen.getByRole("button", { name: "安装所选版本" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("下载失败");
  expect(
    screen.getByRole("radio", { name: /\.codex\/skills\/demo/ }),
  ).toBeChecked();
  expect(calls).toContainEqual({
    command: "install_catalog_skill",
    args: {
      source: hit.source,
      skillId: hit.skillId,
      repositoryPath: ".codex/skills/demo",
      repositoryPaths: null,
    },
  });
  handlers.set("install_catalog_skill", () => ({
    status: "installed",
    skill: makeSkill(),
  }));
  fireEvent.click(screen.getByRole("button", { name: "安装所选版本" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("allows cancelling candidate selection without installing anything", async () => {
  handlers.set("search_catalog_skills", () => [hit]);
  handlers.set("install_catalog_skill", () => ({
    status: "selectionRequired",
    candidates: candidates.map((c) => ({ ...c, contentHash: "same" })),
  }));
  renderWithProviders(<InstallSkillsPage />);
  search();
  fireEvent.click(await screen.findByRole("button", { name: "安装" }));
  expect(await screen.findByRole("dialog")).toHaveTextContent("文件内容相同");
  fireEvent.click(screen.getByRole("button", { name: "取消" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(
    calls.filter((c) => c.command === "install_catalog_skill"),
  ).toHaveLength(1);
  expect(screen.getByRole("button", { name: "安装" })).toBeEnabled();
});

it("local tab scans a collection, imports a selected skill and refreshes its state", async () => {
  let skills: SkillView[] = [];
  const path = "/local/collection/demo";
  handlers.set("scan_skills", () => skills);
  handlers.set("discover_local_skills", () => [
    { path, name: "Local demo", description: "resource", error: null },
  ]);
  handlers.set("import_local_skill", () => {
    skills = [
      makeSkill({
        name: "demo",
        sourceIds: ["studio"],
        installation: {
          source: `local:${path}`,
          skillId: "demo",
          repositoryPath: "",
          installedAt: 123,
          contentHash: "hash",
        },
      }),
    ];
    return { skill: skills[0], changed: true };
  });
  renderWithProviders(<InstallSkillsPage />);
  fireEvent.click(screen.getByRole("tab", { name: "本地" }));
  fireEvent.change(screen.getByRole("textbox", { name: "本地目录" }), {
    target: { value: "/local/collection" },
  });
  fireEvent.click(screen.getByRole("button", { name: "扫描" }));
  expect(await screen.findByText("Local demo")).toBeInTheDocument();
  expect(calls).toContainEqual({
    command: "discover_local_skills",
    args: { path: "/local/collection" },
  });
  fireEvent.click(screen.getByRole("button", { name: "导入" }));
  expect(await screen.findByRole("button", { name: "重新导入" })).toBeEnabled();
  expect(calls).toContainEqual({
    command: "import_local_skill",
    args: { path },
  });
});

it("local scan handles an empty folder and scan errors", async () => {
  handlers.set("discover_local_skills", () => []);
  renderWithProviders(<InstallSkillsPage />);
  fireEvent.click(screen.getByRole("tab", { name: "本地" }));
  fireEvent.change(screen.getByRole("textbox", { name: "本地目录" }), {
    target: { value: "/empty" },
  });
  fireEvent.click(screen.getByRole("button", { name: "扫描" }));
  expect(await screen.findByText(/该目录未发现/)).toBeInTheDocument();
  handlers.set("discover_local_skills", () => {
    throw new Error("目录不存在");
  });
  fireEvent.click(screen.getByRole("button", { name: "扫描" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("目录不存在");
});
