import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { ProjectsPage } from "@/pages/ProjectsPage";
import { McpProjectsPage } from "@/pages/McpProjectsPage";
import { setTarget } from "@/lib/api/transport";
import type { ManagedMcp, ManagementStatus } from "@/lib/api/mcpManagement";
import type { ProjectBinding } from "@/types";
import {
  calls,
  claudeAgent,
  codexAgent,
  defaultSettings,
  handlers,
  makeGroup,
  makeProject,
  makeSkill,
} from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

/**
 * Skill 项目页和 MCP 项目页跑同一组用例，两边用对等的数据：
 * - webapp：成员是一个单独选择的 + 一个绑定分组里的；Claude Code 已启用，Codex 没有
 * - note：还没有成员
 * 哪一边的行为或文案和另一边（以及分组页）不一样，这里会直接失败。
 */
interface Options {
  list?: "pending" | Error;
  none?: boolean;
  /** Claude Code 的部署比现在的成员少一个 */
  changed?: boolean;
  /** Claude Code 的部署被外部改过 */
  issue?: boolean;
  /** 同一个项目在另一边（Skill / MCP）还有 Agent 启用着 */
  otherSide?: boolean;
  /** 一个项目都没有 */
  empty?: boolean;
}
interface Board {
  name: string;
  noun: string;
  setup(options?: Options): void;
  render(): void;
  writes(): unknown[];
  enableCodex: unknown;
  stopClaude: unknown;
  reapplyClaude: unknown;
  issue: string;
  /** 详情里没选中的那个成员，与勾选它后的保存请求 */
  extra: string;
  saved: unknown;
  reload: string;
  otherSide: string;
}

const listed = <T,>(list: Options["list"], value: T) =>
  list === "pending"
    ? new Promise<T>(() => {})
    : list
      ? Promise.reject(list)
      : value;

const skillBoard: Board = {
  name: "Skill",
  noun: "skill",
  setup(o = {}) {
    setTarget({ id: "local", name: "本机", connected: true });
    handlers.set("list_agents", () => [claudeAgent, codexAgent]);
    handlers.set("get_settings", () => ({
      ...defaultSettings(),
      manageMcp: true,
    }));
    handlers.set("scan_skills", () => [
      makeSkill({ id: "s1", name: "pdf-tools" }),
      makeSkill({ id: "s2", name: "frontend-design" }),
      makeSkill({ id: "s3", name: "yuque" }),
    ]);
    handlers.set("list_groups", () => [
      makeGroup({ id: "g1", name: "前端", skillIds: ["s2"] }),
    ]);
    handlers.set("list_project_skills", () => []);
    let projects: ProjectBinding[] = [
      makeProject({
        id: "p1",
        name: "webapp",
        root: "/work/webapp",
        agentIds: ["claude-code"],
        skillIds: o.none ? [] : ["s1"],
        groupIds: o.none ? [] : ["g1"],
        deployments: [
          {
            agentId: "claude-code",
            skillIds: o.changed ? ["s1"] : ["s1", "s2"],
            stale: false,
            issue: o.issue ? "pdf-tools 的副本已被修改" : null,
          },
        ],
      }),
      makeProject({ id: "p2", name: "note", root: "/work/note" }),
    ];
    if (o.empty) projects = [];
    handlers.set("list_projects", () => listed(o.list, projects));
    // 与后端一致：写入哪些 Agent 按当时的部署算，成员按当时保存的
    const deploy = (p: ProjectBinding, agentIds: string[]) => ({
      ...p,
      deployments: agentIds.map((agentId) => ({
        agentId,
        skillIds: ["s1", "s2"],
        stale: false,
        issue: null,
      })),
    });
    const deployed = (p: ProjectBinding) =>
      (p.deployments ?? []).map((d) => d.agentId);
    const update = (
      id: string,
      change: (p: ProjectBinding) => ProjectBinding,
    ) => {
      projects = projects.map((p) => (p.id === id ? change(p) : p));
      return { success: [], failed: [] };
    };
    handlers.set("set_project_agent", ({ projectId, agentId, enabled }) =>
      update(projectId, (p) =>
        deploy(
          p,
          enabled
            ? [...deployed(p), agentId]
            : deployed(p).filter((a) => a !== agentId),
        ),
      ),
    );
    handlers.set("reapply_project", ({ projectId }) =>
      update(projectId, (p) => deploy(p, deployed(p))),
    );
    handlers.set("save_project", ({ projectId, members }) =>
      update(projectId, (p) => deploy({ ...p, ...members }, deployed(p))),
    );
    handlers.set("delete_project", ({ projectId }) => {
      projects = projects.filter((p) => p.id !== projectId);
      return null;
    });
    handlers.set("mcp_request", ({ method }) =>
      method === "list"
        ? {
            running: false,
            entries: [],
            servers: [],
            discovered: [],
            scanWarnings: [],
            projects: o.otherSide
              ? {
                  p2: {
                    root: "/work/note",
                    entryIds: ["x"],
                    groupIds: [],
                    claudeLocal: false,
                    active: { codex: { entries: [], bindings: [] } },
                  },
                }
              : {},
          }
        : null,
    );
  },
  render: () => renderWithProviders(<ProjectsPage />),
  // 启停只传这个项目和 Agent，应用修改只传项目：其余由后端按最新状态计算
  writes: () =>
    calls
      .filter((c) =>
        [
          "set_project_agent",
          "reapply_project",
          "save_project",
          "apply_project",
        ].includes(c.command),
      )
      .map((c) => c.args),
  enableCodex: { projectId: "p1", agentId: "codex", enabled: true },
  stopClaude: { projectId: "p1", agentId: "claude-code", enabled: false },
  reapplyClaude: { projectId: "p1" },
  issue: "pdf-tools 的副本已被修改",
  extra: "yuque",
  // 写入哪些 Agent 由后端按当前部署确定；带上打开时的成员，别处改过就不覆盖
  saved: {
    projectId: "p1",
    members: { skillIds: ["s1", "s3"], groupIds: ["g1"], linkMode: "copy" },
    expected: { skillIds: ["s1"], groupIds: ["g1"], linkMode: "copy" },
  },
  reload: "请在 Agent 中重新加载 skill 或新开会话",
  otherSide: "请先在 MCP 项目里停用所有 Agent",
};

const mcp = (id: string, name: string): ManagedMcp => ({
  id,
  name,
  mode: "direct",
  definition: { type: "stdio", command: name },
  oauth: false,
  clientId: null,
  scopes: [],
  bindings: [],
});
const mcpCalls = (method: string) =>
  calls
    .filter(
      (c) =>
        c.command === "mcp_request" &&
        (c.args as { method: string }).method === method,
    )
    .map((c) => (c.args as { params: unknown }).params);
let status: ManagementStatus;
const mcpBoard: Board = {
  name: "MCP",
  noun: "MCP",
  setup(o = {}) {
    setTarget({ id: "local", name: "本机", connected: true });
    handlers.set("list_agents", () => [claudeAgent, codexAgent]);
    let projects: ProjectBinding[] = [
      makeProject({ id: "p1", name: "webapp", root: "/work/webapp" }),
      makeProject({
        id: "p2",
        name: "note",
        root: "/work/note",
        deployments: o.otherSide
          ? [{ agentId: "codex", skillIds: ["s"], stale: false, issue: null }]
          : [],
      }),
    ];
    if (o.empty) projects = [];
    handlers.set("list_projects", () => listed(o.list, projects));
    handlers.set("delete_project", ({ projectId }) => {
      projects = projects.filter((p) => p.id !== projectId);
      return null;
    });
    const hf = mcp("hf", "hf-mcp-server");
    const work = mcp("work", "work-tools");
    const ctx = mcp("ctx", "context7");
    status = {
      running: false,
      servers: [],
      discovered: [],
      scanWarnings: [],
      entries: [hf, work, ctx],
      groups: [{ id: "g1", name: "前端", entryIds: ["work"], sortOrder: 0 }],
      activeGroups: {},
      projects: {
        p1: {
          root: "/work/webapp",
          entryIds: o.none ? [] : ["hf"],
          groupIds: o.none ? [] : ["g1"],
          claudeLocal: false,
          active: {
            claude: { entries: o.changed ? [hf] : [hf, work], bindings: [] },
          },
        },
      },
      projectIssues: o.issue
        ? {
            p1: {
              claude:
                "MCP「hf-mcp-server」的配置已在外部修改或移除，请先核对 /work/webapp/.mcp.json",
            },
          }
        : {},
    };
    handlers.set("mcp_request", ({ method, params }) => {
      if (method === "list") return listed(o.list, status);
      const record = status.projects?.[params?.projectId];
      if (method === "activateProject" && record) {
        const active = { ...record.active };
        if (params.enabled)
          active[params.agent] = { entries: [hf, work], bindings: [] };
        else delete active[params.agent];
        status = {
          ...status,
          projects: {
            ...status.projects,
            [params.projectId]: { ...record, active },
          },
        };
        return [];
      }
      if (method === "saveProject") return [];
      return null;
    });
  },
  render: () => renderWithProviders(<McpProjectsPage />),
  writes: () => [...mcpCalls("activateProject"), ...mcpCalls("saveProject")],
  enableCodex: { projectId: "p1", agent: "codex", enabled: true },
  stopClaude: { projectId: "p1", agent: "claude", enabled: false },
  reapplyClaude: { projectId: "p1", agent: "claude", enabled: true },
  issue: "hf-mcp-server",
  extra: "context7",
  // 带上打开时的成员与设置，别处改过就不覆盖
  saved: {
    projectId: "p1",
    entryIds: ["hf", "ctx"],
    groupIds: ["g1"],
    claudeLocal: false,
    imports: [],
    expected: { entryIds: ["hf"], groupIds: ["g1"], claudeLocal: false },
  },
  reload: "请在 Agent 中重新加载 MCP",
  otherSide: "请先在 Skill 项目里停用所有 Agent",
};

const card = (name: string) =>
  screen.getByRole("button", { name: `编辑 ${name}` });

describe.each([skillBoard, mcpBoard])("$name 项目页", (board) => {
  const spy = (method: "success" | "error" | "warning") =>
    vi.spyOn(toast, method);
  let success: ReturnType<typeof spy>;
  let failure: ReturnType<typeof spy>;
  beforeEach(() => {
    success = spy("success");
    failure = spy("error");
  });
  afterEach(() => {
    success.mockRestore();
    failure.mockRestore();
  });
  async function open(options?: Options) {
    board.setup(options);
    board.render();
    await screen.findByRole("button", { name: "编辑 webapp" });
  }

  it("工具栏、卡片和弹窗用同一套文案", async () => {
    await open();
    expect(screen.getByRole("textbox", { name: "搜索项目…" })).toBeVisible();
    expect(
      screen.getByRole("button", { name: "拖拽排序 webapp" }),
    ).toHaveAttribute("title", "拖拽排序（也可按空格后用方向键移动）");
    // 卡片第二行是项目路径；数量徽标是展开分组后的成员数
    expect(within(card("webapp")).getByText("/work/webapp")).toBeVisible();
    expect(
      within(card("webapp")).getByText(`2 个 ${board.noun}`),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "添加项目" }));
    const dialog = await screen.findByRole("dialog", { name: "添加项目" });
    expect(
      within(dialog).getByRole("textbox", { name: "项目目录" }),
    ).toBeVisible();
    expect(
      within(dialog).getByRole("textbox", { name: "项目名" }),
    ).toBeVisible();
  });

  it("读取中显示「正在读取项目…」", async () => {
    board.setup({ list: "pending" });
    board.render();
    expect(await screen.findByText("正在读取项目…")).toBeVisible();
    expect(screen.queryByText("还没有项目")).toBeNull();
  });

  it("没有项目时给出空状态，可以直接添加", async () => {
    board.setup({ empty: true });
    board.render();
    expect(await screen.findByText("还没有项目")).toBeVisible();
    const add = screen.getAllByRole("button", { name: "添加项目" });
    expect(add).toHaveLength(2);
    fireEvent.click(add[1]);
    expect(
      await screen.findByRole("dialog", { name: "添加项目" }),
    ).toBeVisible();
  });

  it("读取失败时说明原因", async () => {
    board.setup({ list: new Error("磁盘不可读") });
    board.render();
    const alert = await screen.findByText(/读取项目失败/);
    expect(alert).toHaveAttribute("role", "alert");
    expect(alert).toHaveTextContent("磁盘不可读");
  });

  it("每个 Agent 一个图标：只为点的那个 Agent 写入或撤回", async () => {
    await open();
    expect(within(card("webapp")).getByText("使用中")).toBeVisible();
    fireEvent.click(
      within(card("webapp")).getByRole("button", {
        name: "为 Codex 启用「webapp」",
      }),
    );
    await waitFor(() => expect(board.writes()).toEqual([board.enableCodex]));
    await waitFor(() => expect(success).toHaveBeenCalledTimes(1));
    expect(String(success.mock.calls[0][0])).toBe(
      `项目已启用，${board.reload}`,
    );
    fireEvent.click(
      within(card("webapp")).getByRole("button", {
        name: "为 Claude Code 停用「webapp」",
      }),
    );
    await waitFor(() =>
      expect(board.writes()).toEqual([board.enableCodex, board.stopClaude]),
    );
  });

  it("没有成员的项目不能启用，并说明原因", async () => {
    await open();
    const icon = within(card("note")).getByRole("button", {
      name: "为 Codex 启用「note」",
    });
    expect(icon).toHaveAttribute("aria-disabled", "true");
    fireEvent.focus(icon);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      `项目还没有选择 ${board.noun}`,
    );
    fireEvent.click(icon);
    expect(board.writes()).toEqual([]);
  });

  it("成员在写入后变了：标出「有待应用修改」，点「应用修改」重新写入", async () => {
    await open({ changed: true });
    expect(within(card("webapp")).getByText("有待应用修改")).toBeVisible();
    fireEvent.click(
      within(card("webapp")).getByRole("button", {
        name: "应用修改「webapp」",
      }),
    );
    await waitFor(() => expect(board.writes()).toEqual([board.reapplyClaude]));
  });

  it("部署被外部改过：「需要检查」写明原因", async () => {
    await open({ issue: true });
    const badge = within(card("webapp")).getByText("需要检查");
    expect(badge.getAttribute("title")).toContain("Claude Code");
    expect(badge.getAttribute("title")).toContain(board.issue);
  });

  it("有 Agent 启用的项目不能移除；空闲的项目确认后移除", async () => {
    await open();
    const busy = within(card("webapp")).getByRole("button", {
      name: "移除 webapp",
    });
    expect(busy).toBeDisabled();
    expect(busy).toHaveAttribute("title", "请先停用所有 Agent");
    fireEvent.click(
      within(card("note")).getByRole("button", { name: "移除 note" }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: "移除项目「note」",
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "移除" }));
    await waitFor(() =>
      expect(calls.find((c) => c.command === "delete_project")?.args).toEqual({
        projectId: "p2",
      }),
    );
    await waitFor(() =>
      expect(success).toHaveBeenCalledWith(
        "项目已移除（项目目录里的文件未变动）",
      ),
    );
  });

  it("同一个项目在另一边还有 Agent 启用时，同样不能移除", async () => {
    await open({ otherSide: true });
    // 另一边的状态单独读取，读到之后才知道
    await waitFor(() =>
      expect(
        within(card("note")).getByRole("button", { name: "移除 note" }),
      ).toBeDisabled(),
    );
    expect(
      within(card("note")).getByRole("button", { name: "移除 note" }),
    ).toHaveAttribute("title", board.otherSide);
  });

  it("按 Agent 查看：卡片只留这个 Agent 的启用 / 停用", async () => {
    await open();
    fireEvent.click(
      within(screen.getByRole("group", { name: "按 Agent 查看" })).getByRole(
        "button",
        { name: "Codex 0" },
      ),
    );
    const button = within(card("webapp")).getByRole("button", {
      name: "为 Codex 启用「webapp」",
    });
    expect(button).toHaveTextContent("启用");
    expect(
      within(card("webapp")).queryByRole("button", {
        name: "为 Claude Code 停用「webapp」",
      }),
    ).toBeNull();
  });

  it("详情与卡片同一套 Agent 图标；改了成员后保存即写入已启用的 Agent", async () => {
    await open();
    fireEvent.click(card("webapp"));
    const toggle = await screen.findByRole("button", {
      name: "为 Codex 启用「webapp」",
    });
    expect(screen.getByRole("button", { name: "打开项目目录" })).toBeVisible();
    fireEvent.click(screen.getByRole("checkbox", { name: board.extra }));
    // 有未保存的修改时先保存，Agent 图标暂时不能点，悬停说明原因
    expect(toggle).toHaveAttribute("aria-disabled", "true");
    fireEvent.focus(toggle);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "先保存修改，再为 Agent 启停",
    );
    fireEvent.click(toggle);
    expect(board.writes()).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(board.writes()).toEqual([board.saved]));
    await waitFor(() =>
      expect(success).toHaveBeenCalledWith(
        `已保存并写入 Claude Code，${board.reload}`,
      ),
    );
  });

  it("还没有 Agent 启用时，详情保存只记录成员", async () => {
    await open();
    fireEvent.click(card("note"));
    fireEvent.click(await screen.findByRole("checkbox", { name: board.extra }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() =>
      expect(success).toHaveBeenCalledWith(
        "已保存，可在卡片上为需要的 Agent 启用",
      ),
    );
    expect(failure).not.toHaveBeenCalled();
  });
});

it("Skill 项目的 Agent 图标说明写到哪个目录，共享目录还会被谁读取", async () => {
  skillBoard.setup();
  skillBoard.render();
  const icon = await screen.findByRole("button", {
    name: "为 Codex 启用「webapp」",
  });
  fireEvent.focus(icon);
  expect(await screen.findByRole("tooltip")).toHaveTextContent(
    "写到 .agents/skills（共享目录，Pi、OpenCode、Grok Build 也会读取）",
  );
});

it("MCP 项目的 Agent 图标说明写到哪个配置文件", async () => {
  mcpBoard.setup();
  mcpBoard.render();
  const icon = await screen.findByRole("button", {
    name: "为 Codex 启用「webapp」",
  });
  fireEvent.focus(icon);
  expect(await screen.findByRole("tooltip")).toHaveTextContent(
    "写到 .codex/config.toml",
  );
});
