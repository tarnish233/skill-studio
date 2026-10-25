import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { LibraryPage } from "@/pages/LibraryPage";
import {
  agentState,
  claudeAgent,
  codexAgent,
  handlers,
  makeSkill,
} from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

/**
 * 这里没有测 Radix 下拉菜单打开后的交互：过去 Radix 浮层在 jsdom 里单次打开要 3–50 秒。
 * 根因是 nwsapi 匹配 :modal 等状态伪类时递归到栈溢出，已在 tests/setupGlobals.ts 绕过，
 * 现在打开只要几十毫秒，新测试可以直接点开。菜单里那几个动作的正确性另由两处兜住：
 * - 参数形状：tests/apiLayer.test.ts 直接断言 invoke 收到的参数
 * - 命令名与参数名是否对得上后端：tests/ipcContract.test.ts 静态比对
 */

function setup(skills = [makeSkill()]) {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("scan_skills", () => skills);
}

describe("Skill Hub", () => {
  it("uses a single loading message instead of dashed placeholder cards", () => {
    handlers.set("scan_skills", () => new Promise(() => {}));
    const { container } = renderWithProviders(<LibraryPage />);
    expect(screen.getByRole("status")).toHaveTextContent("正在加载 skill");
    expect(container.querySelector(".border-dashed")).toBeNull();
  });

  it("按卡片汇总 tokens，多来源去重、不同内容分别计数且筛选不改变总量", async () => {
    setup([
      makeSkill({
        id: "a",
        contentHash: "same",
        tokens: { skillMd: 1000, extras: 200 },
        sourceIds: ["claude-code"],
      }),
      makeSkill({
        id: "b",
        contentHash: "same",
        tokens: { skillMd: 1000, extras: 200 },
        sourceIds: ["codex"],
      }),
      makeSkill({
        id: "c",
        contentHash: "different",
        tokens: { skillMd: 300, extras: 100 },
        sourceIds: ["studio"],
      }),
      makeSkill({
        id: "broken",
        name: "broken",
        diagnostics: ["链接目标不可用"],
        tokens: { skillMd: 9000, extras: 0 },
      }),
    ]);
    const user = userEvent.setup({ pointerEventsCheck: 0 });
    renderWithProviders(<LibraryPage />);
    expect(await screen.findByText("合计 ≈ 1.6k tokens")).toBeInTheDocument();
    expect(screen.getAllByText("≈ 1k tokens")).toHaveLength(1);
    expect(screen.getByText("≈ 300 tokens")).toHaveAttribute(
      "title",
      expect.stringContaining("SKILL.md ≈ 300"),
    );
    expect(screen.getByText("≈ 300 tokens").getAttribute("title")).toContain(
      "附带文件 ≈ 100",
    );
    expect(screen.getByText("≈ 300 tokens").getAttribute("title")).toContain(
      "整个目录文本合计 ≈ 400 tokens",
    );
    await user.click(screen.getByRole("button", { name: "Codex 1" }));
    expect(screen.queryByText("≈ 300 tokens")).not.toBeInTheDocument();
    expect(screen.getByText("≈ 1k tokens")).toBeInTheDocument();
    expect(screen.getByText("合计 ≈ 1.6k tokens")).toBeInTheDocument();
  });

  it("空状态给出可操作的引导，而不是一句「暂无数据」", async () => {
    handlers.set("list_agents", () => [claudeAgent]);
    handlers.set("scan_skills", () => []);
    renderWithProviders(<LibraryPage />);
    expect(
      await screen.findByText("Skill Hub 还没有发现 skill"),
    ).toBeInTheDocument();
    // 提示里要带上实际扫描的目录，用户才知道该往哪放
    expect(screen.getByText(/\.claude\/skills/)).toBeInTheDocument();
  });

  it("列出 skill 并标注真身所在的 agent", async () => {
    setup();
    renderWithProviders(<LibraryPage />);
    expect(await screen.findByText("pdf-tools")).toBeInTheDocument();
    expect(screen.getByText("处理 PDF")).toBeInTheDocument();
    expect(
      screen.getByRole("img", { name: "来源：Claude Code" }),
    ).toBeInTheDocument();
  });

  it("Hub 托管的 skill 用不同徽标区分", async () => {
    setup([makeSkill({ origin: { kind: "hub" } })]);
    renderWithProviders(<LibraryPage />);
    expect(await screen.findByText("托管中")).toBeInTheDocument();
  });

  it("「已托管」只数真身在 Hub 的 skill，点一下就只看这些", async () => {
    setup([
      makeSkill(),
      makeSkill({
        id: "hub",
        name: "StudioOnly",
        origin: { kind: "hub" },
        sourceIds: ["studio"],
        agents: {},
      }),
    ]);
    const user = userEvent.setup({ pointerEventsCheck: 0 });
    renderWithProviders(<LibraryPage />);
    const hubPill = await screen.findByRole("button", { name: "已托管 1" });
    expect(hubPill).toHaveAttribute("aria-pressed", "false");

    await user.click(hubPill);
    expect(hubPill).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("StudioOnly")).toBeInTheDocument();
    expect(screen.queryByText("pdf-tools")).toBeNull();

    // 是个开关：再点一次关掉
    await user.click(hubPill);
    expect(screen.getByText("pdf-tools")).toBeInTheDocument();
    expect(hubPill).toHaveAttribute("aria-pressed", "false");
  });

  it("筛选胶囊用主题边框，深色模式下不是硬编码的近白色", async () => {
    setup([
      makeSkill(),
      makeSkill({
        id: "hub",
        name: "StudioOnly",
        origin: { kind: "hub" },
        sourceIds: ["studio"],
        agents: {},
      }),
    ]);
    renderWithProviders(<LibraryPage />);
    const filters = within(
      await screen.findByRole("group", { name: "筛选 skill" }),
    );
    await filters.findByRole("button", { name: "全部 2" });
    for (const pill of [
      ...filters.getAllByRole("button"),
      screen.getByRole("button", { name: "已托管 1" }),
    ])
      expect(pill).toHaveClass("border-border-default");
  });

  it("内置 skill 的来源写「Skill Studio 内置」，胶囊和卡片都写名称", async () => {
    setup([
      makeSkill({
        id: "builtin",
        name: "skill-studio",
        origin: { kind: "hub" },
        sourceIds: ["builtin"],
        agents: {},
      }),
    ]);
    renderWithProviders(<LibraryPage />);
    expect(
      await screen.findByRole("button", { name: "Skill Studio 内置 1" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        (_, element) => element?.textContent === "来源：Skill Studio 内置",
      ),
    ).toBeInTheDocument();
  });

  it("开着「已托管」时，来源胶囊与失效来源都按同一口径收窄", async () => {
    setup([
      makeSkill(),
      makeSkill({
        id: "hub",
        name: "StudioOnly",
        origin: { kind: "hub" },
        sourceIds: ["studio"],
        agents: {},
      }),
      makeSkill({ id: "broken", name: "Broken", diagnostics: ["目标不存在"] }),
    ]);
    const user = userEvent.setup({ pointerEventsCheck: 0 });
    renderWithProviders(<LibraryPage />);
    expect(
      await screen.findByRole("button", { name: "Claude Code 1" }),
    ).toBeInTheDocument();
    expect(screen.getByText("失效来源（1）")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "已托管 1" }));
    // 胶囊不能承诺一条点下去只剩 0 条的筛选
    expect(
      screen.getByRole("button", { name: "Claude Code 0" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Skill Studio 1" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("失效来源（1）")).toBeNull();
  });

  it("开着「已托管」筛选还原一个 skill 后，卡片仍留在眼前而不是凭空消失", async () => {
    let collected = true;
    const provenance = {
      sourceIds: ["codex"],
      originalPath: "/home/u/.codex/skills/yuque",
      originalRoot: "/home/u/.codex/skills",
      originalOrigin: { kind: "inPlace" as const, ownerAgent: "codex" },
      backupPath: "/tmp/backup",
      originalHash: "abc",
      collectedAt: 1,
      entryPaths: [],
    };
    handlers.set("list_agents", () => [claudeAgent, codexAgent]);
    handlers.set("scan_skills", () => [
      makeSkill({ id: "plain", name: "Alpha" }),
      makeSkill({
        id: collected ? "hub-id" : "original-id",
        name: "yuque",
        contentHash: "yuque",
        sourceIds: ["codex"],
        provenance,
        origin: collected ? { kind: "hub" } : provenance.originalOrigin,
        sourcePath: collected ? "/hub/yuque" : provenance.originalPath,
        agents: {},
      }),
    ]);
    handlers.set("release_from_hub", () => {
      collected = false;
      return {};
    });
    const user = userEvent.setup({ pointerEventsCheck: 0 });
    renderWithProviders(<LibraryPage />);
    await user.click(await screen.findByRole("button", { name: "已托管 1" }));
    expect(screen.queryByText("Alpha")).toBeNull();

    await user.click(
      screen.getByRole("button", { name: "还原 yuque 到原位置" }),
    );
    await user.click(screen.getByRole("button", { name: "还原到原位置" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    // 文件还在磁盘上，界面必须继续交代它 —— 否则用户会以为被删了
    expect(await screen.findByText("yuque")).toBeInTheDocument();
    expect(screen.queryByText("没有匹配的可用 skill")).toBeNull();
  });

  it("开着来源筛选托管一个 skill 后，卡片同样留在眼前", async () => {
    let collected = false;
    handlers.set("list_agents", () => [claudeAgent, codexAgent]);
    // 旧后端认不出已托管内容的原始来源，托管后它会从「Claude Code」这一维掉出去
    handlers.set("scan_skills", () => [
      makeSkill({
        id: collected ? "hub-id" : "original-id",
        origin: collected
          ? { kind: "hub" }
          : { kind: "inPlace", ownerAgent: "claude-code" },
        agents: collected ? {} : { "claude-code": agentState("source") },
      }),
    ]);
    handlers.set("adopt_to_hub", () => {
      collected = true;
      return {};
    });
    const user = userEvent.setup({ pointerEventsCheck: 0 });
    renderWithProviders(<LibraryPage />);
    await user.click(
      await screen.findByRole("button", { name: "Claude Code 1" }),
    );
    expect(screen.getByText("pdf-tools")).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "托管 pdf-tools 到 Hub" }),
    );
    await user.click(screen.getByRole("button", { name: "托管" }));
    await waitFor(() => expect(collected).toBe(true));
    expect(await screen.findByText("pdf-tools")).toBeInTheDocument();
    expect(screen.queryByText("没有匹配的可用 skill")).toBeNull();
  });

  it("以扩展字段提示代替笼统的跨端警告", async () => {
    setup([makeSkill({ frontmatterExtra: ["context", "agent"] })]);
    renderWithProviders(<LibraryPage />);
    expect(await screen.findByText("扩展字段")).toBeInTheDocument();
    expect(screen.queryByText("跨端")).toBeNull();
  });

  it("keeps one Hub card and explains dedicated variants separately from plugin installation", async () => {
    setup([
      makeSkill({
        name: "demo",
        installation: {
          source: "owner/repo",
          skillId: "demo",
          repositoryPath: ".claude/skills/demo",
          contentHash: "bundle",
          installedAt: 1,
          variants: [
            {
              key: "claude-code",
              repositoryPath: ".claude/skills/demo",
              contentHash: "cc",
            },
            {
              key: "codex",
              repositoryPath: ".codex/skills/demo",
              contentHash: "cx",
            },
          ],
        },
      }),
    ]);
    renderWithProviders(<LibraryPage />);
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "demo 适配版本" }),
    );
    expect(screen.getByRole("dialog")).toHaveTextContent("Claude Code 专用版");
    expect(screen.getByRole("dialog")).toHaveTextContent("Codex 专用版");
    expect(screen.getByRole("dialog")).toHaveTextContent(
      "不包含插件、hooks 或扩展",
    );
    // 弹窗开着时背景不在无障碍树里，按文字找
    expect(screen.getByText("全部 1")).toBeInTheDocument();
  });

  it("搜索按名称与描述过滤", async () => {
    setup([
      makeSkill({ id: "a", name: "pdf-tools", description: "处理 PDF" }),
      makeSkill({ id: "b", name: "code-review", description: "审查代码" }),
    ]);
    const user = userEvent.setup({ pointerEventsCheck: 0 });
    renderWithProviders(<LibraryPage />);
    await screen.findByText("pdf-tools");

    await user.type(screen.getByPlaceholderText("按名称或描述搜索…"), "审查");
    await waitFor(() => {
      expect(screen.queryByText("pdf-tools")).not.toBeInTheDocument();
      expect(screen.getByText("code-review")).toBeInTheDocument();
    });
  });

  it("Hub only exposes collection actions, not registration or removal", async () => {
    setup();
    renderWithProviders(<LibraryPage />);
    await screen.findByText("pdf-tools");
    expect(
      screen.getByRole("button", { name: "打开 pdf-tools 所在目录" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "托管 pdf-tools 到 Hub" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /注册|移除/ })).toBeNull();
    expect(screen.queryByRole("checkbox")).toBeNull();
  });
});

it("merges identical sources into one card and shows both origin logos", async () => {
  setup([
    makeSkill({
      id: "one",
      sourcePath: "/codex/pdf",
      origin: { kind: "inPlace", ownerAgent: "codex" },
      agents: {
        codex: agentState("source"),
        "claude-code": agentState("foreign"),
      },
    }),
    makeSkill({
      id: "two",
      sourcePath: "/claude/pdf",
      origin: { kind: "external" },
      agents: {
        codex: agentState("conflict"),
        "claude-code": agentState("linked"),
      },
    }),
  ]);
  renderWithProviders(<LibraryPage />);
  expect(await screen.findAllByText("pdf-tools")).toHaveLength(1);
  expect(screen.getByRole("img", { name: "来源：Codex" })).toBeInTheDocument();
  expect(
    screen.getByRole("img", { name: "来源：Claude Code" }),
  ).toBeInTheDocument();
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "2 个来源" }));
  expect(screen.getByText("/codex/pdf")).toBeInTheDocument();
  expect(screen.getByText("/claude/pdf")).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: "托管此来源" })).toHaveLength(1);
});

it("keeps differing contents separate and does not label foreign registrations as origins", async () => {
  setup([
    makeSkill({
      id: "a",
      sourcePath: "/a/pdf",
      contentHash: "a",
      agents: {
        "claude-code": agentState("source"),
        codex: agentState("foreign"),
      },
    }),
    makeSkill({
      id: "b",
      sourcePath: "/b/pdf",
      contentHash: "b",
      agents: {
        "claude-code": agentState("source"),
        codex: agentState("conflict"),
      },
    }),
  ]);
  renderWithProviders(<LibraryPage />);
  expect(await screen.findAllByText("pdf-tools")).toHaveLength(2);
  expect(screen.getAllByText("同名 · 内容不同")).toHaveLength(2);
  expect(screen.queryByRole("img", { name: "来源：Codex" })).toBeNull();
});

it("keeps broken links in a separate folded section with actual entry paths", async () => {
  setup([
    makeSkill({
      id: "broken",
      name: "broken",
      origin: { kind: "external" },
      contentHash: "",
      sourcePath: "/missing/target",
      diagnostics: ["目标目录不存在"],
      agents: {
        codex: {
          ...agentState("brokenLink"),
          entryPaths: ["/codex/skills/alias"],
        },
      },
    }),
  ]);
  renderWithProviders(<LibraryPage />);
  const summary = await screen.findByText("失效来源（1）");
  expect(summary.closest("details")).not.toHaveAttribute("open");
  const user = userEvent.setup();
  await user.click(summary);
  expect(screen.getByText("目标目录不存在")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "查看来源" }));
  expect(screen.getByText("/missing/target")).toBeVisible();
  expect(screen.getByText("Codex 入口：/codex/skills/alias")).toBeVisible();
  expect(screen.queryByRole("button", { name: /删除|托管此来源/ })).toBeNull();
});

it("counts unique cards per installation source and combines source filtering with search", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("scan_skills", () => [
    makeSkill({ id: "a", name: "Shared", contentHash: "same" }),
    makeSkill({
      id: "b",
      name: "Shared",
      contentHash: "same",
      sourcePath: "/home/u/.codex/skills/shared",
      root: "/home/u/.codex/skills",
      origin: { kind: "inPlace", ownerAgent: "codex" },
      agents: {
        codex: agentState("source"),
        "claude-code": agentState("foreign"),
      },
    }),
    makeSkill({
      id: "hub",
      name: "StudioOnly",
      sourceIds: ["studio"],
      origin: { kind: "hub" },
      agents: {},
    }),
    makeSkill({
      id: "neutral",
      name: "Neutral",
      root: "/home/u/.agents/skills",
      sourcePath: "/home/u/.agents/skills/neutral",
      origin: { kind: "inPlace", ownerAgent: "codex" },
      agents: {
        codex: {
          ...agentState("source"),
          entryPaths: ["/home/u/.agents/skills/neutral"],
        },
      },
    }),
    makeSkill({ id: "broken", name: "Broken", diagnostics: ["目标不存在"] }),
  ]);
  const user = userEvent.setup();
  renderWithProviders(<LibraryPage />);
  expect(
    await screen.findByRole("button", { name: "全部 3" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Claude Code 1" }),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Codex 1" })).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Skill Studio 1" }),
  ).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Claude Code 1" }));
  expect(screen.getAllByText("Shared")).toHaveLength(1);
  expect(screen.queryByText("StudioOnly")).toBeNull();
  await user.type(screen.getByPlaceholderText("按名称或描述搜索…"), "Neutral");
  expect(screen.getByText("没有匹配的可用 skill")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Agent 1" }));
  expect(screen.getByText("Neutral")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "全部 3" })).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "清除搜索" }));
  await user.click(screen.getByRole("button", { name: "全部 3" }));
  expect(screen.getByText("StudioOnly")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Agent 1" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
});

it("marks a shared skill with a Codex copy by the shared-directory robot and the Codex logo, matching the counters", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("scan_skills", () => [
    makeSkill({
      name: "yuque",
      sourceIds: ["agent", "codex"],
      sourcePath: "/home/u/.agents/skills/yuque",
      root: "/home/u/.agents/skills",
      origin: { kind: "external" },
      agents: {
        codex: {
          ...agentState("copied"),
          entryPaths: [
            "/home/u/.codex/skills/yuque",
            "/home/u/.agents/skills/yuque",
          ],
        },
      },
    }),
  ]);
  renderWithProviders(<LibraryPage />);
  // 共享目录是小机器人，Codex 用品牌图标
  expect(
    await screen.findByRole("img", { name: "来源：Agent" }),
  ).toBeInTheDocument();
  expect(screen.getByRole("img", { name: "来源：Codex" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Agent 1" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Codex 1" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "全部 1" })).toBeInTheDocument();
});

it("keeps original source counts for collected cards and restores via the source record", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  let collected = true;
  const provenance = {
    sourceIds: ["agent", "codex"],
    originalPath: "/home/u/.agents/skills/yuque",
    originalRoot: "/home/u/.agents/skills",
    originalOrigin: { kind: "inPlace" as const, ownerAgent: "codex" },
    backupPath: "/home/u/.skill-studio/skill-backups/snapshot",
    originalHash: "abc",
    collectedAt: 1,
    entryPaths: [],
  };
  handlers.set("scan_skills", () => [
    makeSkill({
      id: collected ? "hub-id" : "original-id",
      name: "yuque",
      sourceIds: ["agent", "codex"],
      provenance,
      origin: collected ? { kind: "hub" } : provenance.originalOrigin,
      sourcePath: collected
        ? "/home/u/.skill-studio/skills/yuque"
        : provenance.originalPath,
      agents: {},
    }),
  ]);
  handlers.set("release_from_hub", (args) => {
    expect(args.skillId).toBe("hub-id");
    collected = false;
    return {};
  });
  const user = userEvent.setup();
  renderWithProviders(<LibraryPage />);
  expect(
    await screen.findByRole("button", { name: "Skill Studio 0" }),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Agent 1" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Codex 1" })).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "还原 yuque 到原位置" }));
  expect(screen.getByRole("dialog")).toHaveTextContent(provenance.originalPath);
  await user.click(screen.getByRole("button", { name: "还原到原位置" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(
    screen.getByRole("button", { name: "Skill Studio 0" }),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Codex 1" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Agent 1" })).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "还原 yuque 到原位置" }),
  ).toBeNull();
});

it("does not invent a source or offer automatic restoration for historical Hub items", async () => {
  handlers.set("scan_skills", () => [
    makeSkill({ name: "legacy", origin: { kind: "hub" }, agents: {} }),
  ]);
  renderWithProviders(<LibraryPage />);
  expect(
    await screen.findByRole("button", { name: "来源待确认 1" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Skill Studio 0" }),
  ).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /还原 .* 到原位置/ })).toBeNull();
});

it("collects and restores one source of a merged card while retaining unique source counters", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  let collected = false;
  const provenance = {
    sourceIds: ["claude-code"],
    originalPath: "/home/u/.claude/skills/demo",
    originalRoot: "/home/u/.claude/skills",
    originalOrigin: { kind: "inPlace" as const, ownerAgent: "claude-code" },
    backupPath: "/tmp/backup",
    originalHash: "same",
    collectedAt: 1,
    entryPaths: [],
  };
  handlers.set("scan_skills", () => [
    makeSkill({
      id: collected ? "hub" : "original",
      name: "demo",
      contentHash: "same",
      sourceIds: ["claude-code"],
      origin: collected ? { kind: "hub" } : provenance.originalOrigin,
      provenance: collected ? provenance : null,
      sourcePath: collected ? "/hub/demo" : provenance.originalPath,
    }),
    makeSkill({
      id: "external",
      name: "demo",
      contentHash: "same",
      sourceIds: ["codex"],
      origin: { kind: "external" },
      sourcePath: "/external/demo",
    }),
  ]);
  handlers.set("adopt_to_hub", (args) => {
    expect(args.skillId).toBe("original");
    collected = true;
    return {};
  });
  handlers.set("release_from_hub", (args) => {
    expect(args.skillId).toBe("hub");
    collected = false;
    return {};
  });
  const user = userEvent.setup();
  renderWithProviders(<LibraryPage />);
  await user.click(await screen.findByRole("button", { name: "2 个来源" }));
  await user.click(screen.getByRole("button", { name: "托管此来源" }));
  await user.click(screen.getByRole("button", { name: "托管" }));
  await waitFor(() => expect(collected).toBe(true));
  await screen.findByText("托管中");
  await user.click(screen.getByRole("button", { name: "2 个来源" }));
  await user.click(screen.getByRole("button", { name: "移出 Hub 并还原" }));
  await user.click(screen.getByRole("button", { name: "还原到原位置" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(screen.getByRole("button", { name: "全部 1" })).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Claude Code 1" }),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Codex 1" })).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Skill Studio 0" }),
  ).toBeInTheDocument();
});
