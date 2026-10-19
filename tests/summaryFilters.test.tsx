import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { SummaryFilter, agentOption } from "@/components/common/SummaryBar";
import { setTarget } from "@/lib/api/transport";
import type { ManagementStatus } from "@/lib/api/mcpManagement";
import { GroupsPage } from "@/pages/GroupsPage";
import { LibraryPage } from "@/pages/LibraryPage";
import { McpGroupsPage } from "@/pages/McpGroupsPage";
import { McpPage } from "@/pages/McpPage";
import { McpProjectsPage } from "@/pages/McpProjectsPage";
import { ProjectsPage } from "@/pages/ProjectsPage";
import {
  agentState,
  claudeAgent,
  codexAgent,
  defaultSettings,
  handlers,
  makeGroup,
  makeProject,
  makeSkill,
} from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

afterEach(() => setTarget({ id: "local", name: "本机", connected: true }));

it("SummaryFilter：「全部」固定在最前；单选，点已选中的那枚回到「全部」", () => {
  const onSelect = vi.fn();
  const options = [
    agentOption({ id: "codex", label: "Codex", count: 2, title: "筛选 Codex" }),
    { id: "managed", label: "已托管", count: 1 },
  ];
  // 纯展示组件：不套 Provider，rerender 才能保住同一批 DOM 节点
  const { rerender } = render(
    <SummaryFilter
      label="筛选测试"
      total={3}
      options={options}
      selected={null}
      onSelect={onSelect}
    />,
  );
  const group = within(screen.getByRole("group", { name: "筛选测试" }));
  const [all, codex, managed] = group.getAllByRole("button");
  expect(all).toHaveAccessibleName("全部 3");
  expect(all).toHaveAttribute("aria-pressed", "true");
  // 有图标（一眼认得出的 Agent、Skill Studio）只显示图标和数字，名字进读屏名称和悬停提示
  expect(codex).toHaveAccessibleName("Codex 2");
  expect(codex).toHaveAttribute("title", "筛选 Codex");
  expect(codex).toHaveTextContent(/^2$/);
  // 其余显示名称
  expect(managed).toHaveAccessibleName("已托管 1");
  expect(managed).toHaveTextContent(/^已托管 1$/);
  expect(managed.querySelector("svg, img")).toBeNull();
  fireEvent.click(codex);
  expect(onSelect).toHaveBeenLastCalledWith("codex");

  rerender(
    <SummaryFilter
      label="筛选测试"
      total={3}
      options={options}
      selected="codex"
      onSelect={onSelect}
    />,
  );
  expect(codex).toHaveAttribute("aria-pressed", "true");
  expect(all).toHaveAttribute("aria-pressed", "false");
  fireEvent.click(codex);
  expect(onSelect).toHaveBeenLastCalledWith(null);
  fireEvent.click(managed);
  expect(onSelect).toHaveBeenLastCalledWith("managed");
  fireEvent.click(all);
  expect(onSelect).toHaveBeenLastCalledWith(null);
});

it("SummaryFilter：选中项不在选项里（例如 Agent 被关闭管理）时显示为「全部」", () => {
  render(
    <SummaryFilter
      label="筛选测试"
      total={1}
      options={[agentOption({ id: "codex", label: "Codex", count: 1 })]}
      selected="opencode"
      onSelect={() => {}}
    />,
  );
  expect(screen.getByRole("button", { name: "全部 1" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});

it("SummaryFilter：放不下时在被裁掉的一侧渐隐，提示还能横向滚动", () => {
  const scrollWidth = vi
    .spyOn(HTMLElement.prototype, "scrollWidth", "get")
    .mockReturnValue(600);
  const clientWidth = vi
    .spyOn(HTMLElement.prototype, "clientWidth", "get")
    .mockReturnValue(400);
  try {
    render(
      <SummaryFilter
        label="筛选测试"
        total={1}
        options={[]}
        selected={null}
        onSelect={() => {}}
      />,
    );
    const group = screen.getByRole("group", { name: "筛选测试" });
    expect(group).toHaveAttribute("data-fade", "end");
    // 滚到最右：右边没有被裁掉的了，左边有
    Object.defineProperty(group, "scrollLeft", {
      value: 200,
      configurable: true,
    });
    fireEvent.scroll(group);
    expect(group).toHaveAttribute("data-fade", "start");
    // 窗口放大到放得下：浏览器会把滚动位置归零，也就不再渐隐
    scrollWidth.mockReturnValue(300);
    Object.defineProperty(group, "scrollLeft", {
      value: 0,
      configurable: true,
    });
    fireEvent(window, new Event("resize"));
    expect(group).toHaveAttribute("data-fade", "none");
  } finally {
    scrollWidth.mockRestore();
    clientWidth.mockRestore();
  }
});

/**
 * 每个统计栏都是同一套筛选：第一枚是「全部 N」且默认选中；点一个 Agent 只看它，
 * 再点一次回到「全部」。总数只在「全部」上出现，右侧不再重复一枚总数。
 */
async function checkFilter({
  group,
  total,
  option,
  narrowed,
  restored,
}: {
  group: string;
  total: number;
  option: string;
  narrowed: () => void;
  restored: () => Promise<unknown> | void;
}) {
  const bar = within(await screen.findByRole("group", { name: group }));
  const chip = await bar.findByRole("button", { name: option });
  const [all, ...options] = bar.getAllByRole("button");
  expect(all).toHaveAccessibleName(`全部 ${total}`);
  expect(all).toHaveAttribute("aria-pressed", "true");
  expectOptions(options);
  expect(chip).toHaveAttribute("aria-pressed", "false");

  fireEvent.click(chip);
  expect(chip).toHaveAttribute("aria-pressed", "true");
  expect(all).toHaveAttribute("aria-pressed", "false");
  narrowed();

  fireEvent.click(chip);
  expect(chip).toHaveAttribute("aria-pressed", "false");
  expect(all).toHaveAttribute("aria-pressed", "true");
  await restored();
}

/** 带图标的项只显示图标和数字；不带图标的显示「名称 数字」。悬停提示都带名称 */
function expectOptions(options: HTMLElement[]) {
  for (const option of options) {
    const name = option.getAttribute("aria-label")!;
    expect(option).toHaveTextContent(
      option.querySelector("svg, img") ? /^\d+$/ : name,
    );
    expect(option.getAttribute("title")).toContain(name.replace(/ \d+$/, ""));
  }
}
/**
 * 统计栏右侧各项：数量一律写成「名称 数字」，不带「个」「次」「处」这样的单位；
 * 单位和口径写在悬停提示里，提示以名称开头。token 合计是用量，保留「tokens」。
 */
function expectStats(group: string, texts: string[]) {
  const end = screen.getByRole("group", { name: group }).nextElementSibling!;
  const items = Array.from(end.children).filter((el) => el.textContent!.trim());
  expect(items.map((el) => el.textContent!.trim())).toEqual(texts);
  for (const el of items) {
    const name = el.textContent!.trim().match(/^(\D+) \d+$/)?.[1];
    if (name)
      expect(el.getAttribute("title")).toMatch(new RegExp(`^${name}：`));
  }
}
/** 卡片上写成文字的来源标记，读屏读作「来源：名称」 */
const textMark = (label: string) =>
  screen.getByText((_, el) => el?.textContent === `来源：${label}`);
/** 读屏名称为 name 的筛选项有没有图标 */
const hasIcon = (bar: ReturnType<typeof within>, name: string) =>
  !!bar.getByRole("button", { name }).querySelector("svg, img");

const binding = (agent: string, path: string, project: string | null) => ({
  id: `${agent}-binding`,
  agent,
  path,
  project,
  key: "work",
  original: null,
  installed: { type: "stdio", command: "work" },
});
const managed = (
  id: string,
  bindings: ReturnType<typeof binding>[],
  mode: "direct" | "gateway" = "direct",
) => ({
  id,
  name: id,
  mode,
  definition: { type: "stdio", command: id },
  oauth: false,
  clientId: null,
  scopes: [],
  bindings,
});
const mcpStatus = (over: Partial<ManagementStatus>): ManagementStatus => ({
  running: false,
  entries: [],
  servers: [],
  discovered: [],
  scanWarnings: [],
  ...over,
});

it("Skill Hub：同一套「全部」筛选，原来那枚「已安装 N 个」不再重复", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("scan_skills", () => [
    makeSkill(),
    makeSkill({
      id: "review",
      name: "code-review",
      contentHash: "review",
      sourcePath: "/home/u/.codex/skills/code-review",
      root: "/home/u/.codex/skills",
      origin: { kind: "inPlace", ownerAgent: "codex" },
      agents: { codex: agentState("source") },
    }),
    makeSkill({
      id: "hub",
      name: "hub-only",
      contentHash: "hub",
      origin: { kind: "hub" },
      sourceIds: ["studio"],
      agents: {},
    }),
  ]);
  renderWithProviders(<LibraryPage />);
  await checkFilter({
    group: "筛选 skill",
    total: 3,
    option: "Claude Code 1",
    narrowed: () => {
      expect(screen.getByText("pdf-tools")).toBeInTheDocument();
      expect(screen.queryByText("code-review")).toBeNull();
    },
    restored: () => screen.findByText("code-review"),
  });
  expect(screen.queryByText(/^已安装 \d+ 个$/)).toBeNull();
  expectStats("筛选 skill", ["已托管 1", "已备份 0", "合计 ≈ 300 tokens"]);
});

it("Skill Hub：Agent 品牌、Skill Studio 和共享目录的小机器人用图标；待确认、外部来源写出名称，卡片上也一样", async () => {
  handlers.set("list_agents", () => [claudeAgent]);
  handlers.set("scan_skills", () => [
    makeSkill({ id: "installed", name: "installed", sourceIds: ["studio"] }),
    makeSkill({
      id: "shared",
      name: "shared",
      contentHash: "shared",
      sourceIds: ["agent"],
    }),
    makeSkill({
      id: "legacy",
      name: "legacy",
      contentHash: "legacy",
      sourceIds: ["unknown"],
      origin: { kind: "hub" },
      agents: {},
    }),
    makeSkill({
      id: "outside",
      name: "outside",
      contentHash: "outside",
      sourceIds: ["external"],
    }),
    makeSkill({
      id: "claude",
      name: "claude-only",
      contentHash: "claude",
      sourceIds: ["claude-code"],
    }),
  ]);
  renderWithProviders(<LibraryPage />);
  const bar = within(await screen.findByRole("group", { name: "筛选 skill" }));
  await bar.findByRole("button", { name: "外部来源 1" });
  const [, ...options] = bar.getAllByRole("button");
  expectOptions(options);
  expect(hasIcon(bar, "Claude Code 1")).toBe(true);
  expect(hasIcon(bar, "Skill Studio 1")).toBe(true);
  // 共享目录（~/.agents/skills）是一个小机器人
  expect(
    bar.getByRole("button", { name: "Agent 1" }).querySelector(".lucide-bot"),
  ).not.toBeNull();
  for (const name of ["来源待确认 1", "外部来源 1"])
    expect(hasIcon(bar, name)).toBe(false);
  // 卡片上的来源标记同一规则：有图标的用图标，其余写出名称
  expect(
    screen.getByRole("img", { name: "来源：Skill Studio" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("img", { name: "来源：Claude Code" }),
  ).toBeInTheDocument();
  expect(
    screen
      .getByRole("img", { name: "来源：Agent" })
      .querySelector(".lucide-bot"),
  ).not.toBeNull();
  for (const label of ["来源待确认", "外部来源"]) {
    expect(screen.queryByRole("img", { name: `来源：${label}` })).toBeNull();
    const mark = textMark(label);
    expect(mark.querySelector("svg, img")).toBeNull();
    expect(mark).toHaveAttribute("title", expect.stringContaining(label));
  }
});

it("MCP Hub：同一套「全部」筛选，再点一次已选中的 Agent 回到全部", async () => {
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? mcpStatus({
          entries: [
            managed("alpha", [binding("claude", "/home/u/.claude.json", null)]),
            managed("beta", [
              binding("codex", "/home/u/.codex/config.toml", null),
            ]),
          ],
        })
      : null,
  );
  renderWithProviders(<McpPage />);
  const bar = within(await screen.findByRole("group", { name: "筛选 MCP" }));
  await bar.findByRole("button", { name: "Codex 1" });
  expect(hasIcon(bar, "Codex 1")).toBe(true);
  expect(hasIcon(bar, "网关连接 0")).toBe(false);
  await checkFilter({
    group: "筛选 MCP",
    total: 2,
    option: "Codex 1",
    narrowed: () =>
      expect(screen.queryByRole("button", { name: "查看 alpha" })).toBeNull(),
    restored: () => screen.findByRole("button", { name: "查看 alpha" }),
  });
  expectStats("筛选 MCP", ["已托管 2", "已备份 0", "网关已关闭"]);
});

it("Skill Hub：「已托管」在右侧，是能和来源叠加的开关；开着时左侧的数字跟着收窄", async () => {
  handlers.set("list_agents", () => [claudeAgent]);
  handlers.set("scan_skills", () => [
    makeSkill(),
    makeSkill({
      id: "collected",
      name: "collected",
      contentHash: "collected",
      origin: { kind: "hub" },
      sourceIds: ["claude-code"],
      agents: {},
    }),
    makeSkill({
      id: "studio",
      name: "studio-made",
      contentHash: "studio",
      origin: { kind: "hub" },
      sourceIds: ["studio"],
      agents: {},
    }),
  ]);
  renderWithProviders(<LibraryPage />);
  const bar = within(await screen.findByRole("group", { name: "筛选 skill" }));
  const claude = await bar.findByRole("button", { name: "Claude Code 2" });
  expect(bar.queryByRole("button", { name: /^已托管/ })).toBeNull();
  const managed = screen.getByRole("button", { name: "已托管 2" });
  expect(managed).toHaveAttribute("aria-pressed", "false");
  expect(managed.querySelector("svg, img")).toBeNull();

  fireEvent.click(claude);
  fireEvent.click(managed);
  // 两个筛选叠加：Claude Code 来源里已托管的那一个
  expect(managed).toHaveAttribute("aria-pressed", "true");
  expect(claude).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByText("collected")).toBeInTheDocument();
  expect(screen.queryByText("pdf-tools")).toBeNull();
  expect(screen.queryByText("studio-made")).toBeNull();
  // 左侧的数字是开着「已托管」时点下去会剩下的条数；右侧仍是总数
  expect(bar.getByRole("button", { name: "全部 2" })).toBeInTheDocument();
  expect(
    bar.getByRole("button", { name: "Claude Code 1" }),
  ).toBeInTheDocument();
  expect(
    bar.getByRole("button", { name: "Skill Studio 1" }),
  ).toBeInTheDocument();
  expect(managed).toHaveAccessibleName("已托管 2");

  // 「全部」只清左侧这一组，「已托管」照旧开着
  fireEvent.click(bar.getByRole("button", { name: "全部 2" }));
  expect(managed).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByText("studio-made")).toBeInTheDocument();
  expect(screen.queryByText("pdf-tools")).toBeNull();
  fireEvent.click(managed);
  expect(await screen.findByText("pdf-tools")).toBeInTheDocument();
  expect(bar.getByRole("button", { name: "全部 3" })).toBeInTheDocument();
});

it("MCP Hub：「已托管」同样移到右侧，开着时只看托管的，内置服务也不显示", async () => {
  handlers.set("get_settings", () => ({
    ...defaultSettings(),
    showCodexBuiltinMcp: true,
  }));
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? mcpStatus({
          entries: [
            managed("alpha", [binding("claude", "/home/u/.claude.json", null)]),
            managed(
              "beta",
              [binding("codex", "/home/u/.codex/config.toml", null)],
              "gateway",
            ),
          ],
          discovered: [
            {
              id: "gamma",
              name: "gamma",
              server: null,
              issue: null,
              managedId: null,
              sources: [
                {
                  id: "gamma-source",
                  project: null,
                  definition: { command: "gamma" },
                  agent: "codex",
                  path: "/home/u/.codex/config.toml",
                  scope: "用户全局",
                  key: "gamma",
                  enabled: true,
                  gateway: false,
                },
              ],
            },
          ],
          builtins: [
            {
              name: "node_repl",
              agent: "codex",
              path: "/home/u/.codex/config.toml",
              scope: "用户全局",
            },
          ],
        })
      : null,
  );
  renderWithProviders(<McpPage />);
  const bar = within(await screen.findByRole("group", { name: "筛选 MCP" }));
  const codex = await bar.findByRole("button", { name: "Codex 2" });
  expect(bar.queryByRole("button", { name: /^已托管/ })).toBeNull();
  // 「网关连接」是连接方式，留在左侧
  expect(bar.getByRole("button", { name: "网关连接 1" })).toBeInTheDocument();
  const managedOnly = screen.getByRole("button", { name: "已托管 2" });
  expect(screen.getByText("node_repl")).toBeInTheDocument();

  fireEvent.click(codex);
  fireEvent.click(managedOnly);
  expect(managedOnly).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: "查看 beta" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "查看 gamma" })).toBeNull();
  expect(screen.queryByRole("button", { name: "查看 alpha" })).toBeNull();
  expect(screen.queryByText("node_repl")).toBeNull();
  expect(bar.getByRole("button", { name: "全部 2" })).toBeInTheDocument();
  expect(bar.getByRole("button", { name: "Codex 1" })).toBeInTheDocument();
});

it("Skill 分组：「全部」视角与单个 Agent 视角共用这套筛选，总数不再在右侧重复", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("list_groups", () => [
    makeGroup({ id: "fe", name: "前端" }),
    makeGroup({ id: "doc", name: "文档", sortOrder: 1 }),
  ]);
  handlers.set("get_config", () => ({
    activeGroups: { codex: [{ groupId: "fe", skillIds: [], entries: [] }] },
  }));
  renderWithProviders(<GroupsPage />);
  await checkFilter({
    group: "按 Agent 查看",
    total: 2,
    option: "Codex 1",
    // 单个 Agent 视角：卡片上只剩这个 Agent 的启停，右侧是它自己的统计
    narrowed: () => {
      expect(
        screen.queryByRole("button", { name: /为 Claude Code/ }),
      ).toBeNull();
      expectStats("按 Agent 查看", ["已启用 0", "合计 ≈ 0 tokens"]);
    },
    restored: () =>
      expect(
        screen.getAllByRole("button", { name: /为 Claude Code/ }).length,
      ).toBeGreaterThan(0),
  });
  // 「全部」视角右侧给出在用的分组数，和卡片上的「使用中」同一口径
  expectStats("按 Agent 查看", ["已启用 1"]);
  expect(screen.queryByText(/^共 \d+ 个分组$/)).toBeNull();
});

it("MCP 分组：同一套筛选，「全部」视角右侧是在用的分组数", async () => {
  const entry = managed("one", []);
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? mcpStatus({
          entries: [entry],
          groups: [
            { id: "dev", name: "开发", entryIds: ["one"], sortOrder: 0 },
            { id: "doc", name: "文档", entryIds: ["one"], sortOrder: 1 },
          ],
          activeGroups: {
            claude: [{ groupId: "dev", entries: [entry], bindings: [] }],
          },
        })
      : null,
  );
  renderWithProviders(<McpGroupsPage />);
  await checkFilter({
    group: "按 Agent 查看",
    total: 2,
    option: "Claude Code 1",
    narrowed: () => {
      expect(screen.queryByRole("button", { name: /为 Codex/ })).toBeNull();
      expectStats("按 Agent 查看", ["已启用 0"]);
    },
    restored: () =>
      expect(
        screen.getAllByRole("button", { name: /为 Codex/ }).length,
      ).toBeGreaterThan(0),
  });
  expectStats("按 Agent 查看", ["已启用 1"]);
  expect(screen.queryByText(/^共 \d+ 个分组$/)).toBeNull();
});

it("Skill 项目：补上「全部」，右侧不再重复「项目 N 个」", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("list_projects", () => [
    makeProject({
      id: "web",
      name: "webapp",
      agentIds: ["claude-code"],
      enabledAgentIds: ["claude-code"],
    }),
    makeProject({ id: "note", name: "note", root: "/work/note" }),
  ]);
  renderWithProviders(<ProjectsPage />);
  await checkFilter({
    group: "按项目 Agent 筛选",
    total: 2,
    option: "Claude Code 1",
    narrowed: () => expect(screen.queryByText("note")).toBeNull(),
    restored: () => screen.findByText("note"),
  });
  expect(screen.queryByText(/^项目 \d+ 个$/)).toBeNull();
  expectStats("按项目 Agent 筛选", ["已备份 0", "未托管 0", "已启用 1"]);
});

it("MCP 项目：补上「全部」，右侧不再重复「项目 N 个」", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("list_projects", () => [
    makeProject({ id: "web", name: "webapp", root: "/work/webapp" }),
    makeProject({ id: "note", name: "note", root: "/work/note" }),
  ]);
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? mcpStatus({
          entries: [
            managed("work", [
              binding(
                "codex",
                "/work/webapp/.codex/config.toml",
                "/work/webapp",
              ),
            ]),
          ],
        })
      : null,
  );
  renderWithProviders(<McpProjectsPage />);
  await checkFilter({
    group: "按项目 Agent 筛选",
    total: 2,
    option: "Codex 1",
    narrowed: () => expect(screen.queryByText("note")).toBeNull(),
    restored: () => screen.findByText("note"),
  });
  expect(screen.queryByText(/^项目 \d+ 个$/)).toBeNull();
  expectStats("按项目 Agent 筛选", ["托管接入 1", "未托管 0", "已启用 1"]);
});

it("远程 MCP：只读页面也有同一套「全部」筛选", async () => {
  setTarget({ id: "server-a", name: "A", connected: true });
  handlers.set("remote_request", ({ method }) => {
    if (method === "scan_mcp")
      return {
        servers: [
          { id: "a", name: "Alpha MCP", agents: ["codex"] },
          { id: "b", name: "Beta MCP", agents: ["opencode"] },
        ],
        warnings: [],
      };
    if (method === "get_settings") return defaultSettings();
    return [];
  });
  renderWithProviders(<McpPage />);
  await checkFilter({
    group: "筛选 MCP",
    total: 2,
    option: "Codex 1",
    narrowed: () => expect(screen.queryByText("Beta MCP")).toBeNull(),
    restored: () => screen.findByText("Beta MCP"),
  });
  expect(screen.queryByText(/^已配置 \d+ 个 MCP$/)).toBeNull();
  expectStats("筛选 MCP", ["只读"]);
});
