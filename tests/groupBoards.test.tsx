import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { GroupsPage } from "@/pages/GroupsPage";
import { McpGroupsPage } from "@/pages/McpGroupsPage";
import { setTarget } from "@/lib/api/transport";
import type { ManagedMcp, ManagementStatus } from "@/lib/api/mcpManagement";
import {
  agentState,
  calls,
  claudeAgent,
  codexAgent,
  handlers,
  makeGroup,
  makeProject,
  makeSkill,
} from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

/**
 * Skill 分组页和 MCP 分组页跑同一组用例，两边用对等的数据：
 * - 前端：Claude 启用后又加了一个成员（有待应用修改），Codex 也在用且正常
 * - LLM：Codex 在用，但部署出了问题（需要检查）
 * - Git 工具：有一个成员已经不在 Hub 里（成员已缺失）
 * - 空分组：还没有成员
 * 哪一边的行为或文案和另一边不一样，这里会直接失败。
 */
interface Options {
  groups?: "none";
  list?: "pending" | Error;
  emptyHub?: boolean;
  saveError?: string;
  /** 排序请求等它放行才返回，用来看松手后、后端返回前的样子 */
  reorderGate?: Promise<void>;
  /** 启停时后端报告原样留下的副本 / 接入；"none" 表示后端什么都没返回 */
  untouched?: boolean | "none";
  /** 写入 Codex 时后端报错 */
  activateError?: string;
}
interface Board {
  name: string;
  setup(options?: Options): void;
  render(): void;
  /** 「前端」的两个成员，按分组里的顺序 */
  members: [string, string];
  /** 「LLM」出问题的原因里会出现的字样 */
  issue: string;
  emptyHub: RegExp;
  reordered(): string[] | undefined;
  activations(): unknown[];
  /** 保存「前端」（Claude 与 Codex 都在用）后依次写入的两次请求 */
  savedWrites: unknown[];
  /** 「Git 工具」里已经不在 Hub 的那个成员，以及去掉它之后剩下的 */
  missing: string;
  gitRemaining: string[];
  /** 最近一次保存的成员 */
  savedMembers(): string[] | undefined;
  /** 停用时原样留下的那一份，在提示里应出现的字样 */
  untouchedText: string;
}

const gated = <T,>(gate: Promise<void> | undefined, value: () => T) =>
  gate ? gate.then(value) : value();
const listed = <T,>(list: Options["list"], value: T) =>
  list === "pending"
    ? new Promise<T>(() => {})
    : list
      ? Promise.reject(list)
      : value;

/** Skill 分组页每次写入后，这个 Agent 叠加的全部分组（后端算出的结果） */
let skillWrites: { agentId: string; groupIds: string[] }[] = [];
const skillBoard: Board = {
  name: "Skill",
  setup(o = {}) {
    handlers.set("list_agents", () => [claudeAgent, codexAgent]);
    const skills = o.emptyHub
      ? []
      : [
          makeSkill({
            id: "s1",
            name: "pdf-tools",
            sourcePath: "/hub/pdf-tools",
            agents: {
              "claude-code": agentState("notLinked"),
              codex: agentState("linked"),
            },
          }),
          makeSkill({
            id: "s2",
            name: "frontend-design",
            sourcePath: "/hub/frontend-design",
            agents: {
              "claude-code": agentState("linked"),
              codex: agentState("linked"),
            },
          }),
          makeSkill({
            id: "s3",
            name: "yuque",
            sourcePath: "/hub/yuque",
            agents: {
              "claude-code": agentState("notLinked"),
              // 在 Codex 上被原生开关停用：LLM 对 Codex 需要检查
              codex: agentState("linked", true),
            },
          }),
          makeSkill({
            id: "s4",
            name: "git-helper",
            sourcePath: "/hub/git-helper",
            agents: {
              "claude-code": agentState("notLinked"),
              codex: agentState("notLinked"),
            },
          }),
        ];
    handlers.set("scan_skills", () => skills);
    let groups =
      o.groups === "none"
        ? []
        : [
            makeGroup({ id: "g1", name: "前端", skillIds: ["s2", "s1"] }),
            makeGroup({
              id: "g2",
              name: "LLM",
              sortOrder: 1,
              skillIds: ["s3"],
            }),
            makeGroup({
              id: "g3",
              name: "Git 工具",
              sortOrder: 2,
              skillIds: ["s4", "s-gone"],
            }),
            makeGroup({ id: "g4", name: "空分组", sortOrder: 3 }),
          ];
    handlers.set("list_groups", () => listed(o.list, groups));
    let active: Record<string, { groupId: string; skillIds: string[] }[]> = {
      "claude-code": [{ groupId: "g1", skillIds: ["s2"] }],
      codex: [
        { groupId: "g1", skillIds: ["s2", "s1"] },
        { groupId: "g2", skillIds: ["s3"] },
      ],
    };
    handlers.set("get_config", () => ({ activeGroups: active }));
    skillWrites = [];
    // 与后端一致：启停与重新应用都按当时的启用记录算出这个 Agent 的全部分组再写入
    const write = (agentId: string, groupIds: string[]) => {
      if (o.activateError && agentId === "codex")
        throw new Error(o.activateError);
      active = {
        ...active,
        [agentId]: groupIds.map((groupId) => ({
          groupId,
          skillIds: groups.find((g) => g.id === groupId)!.skillIds,
        })),
      };
      skillWrites.push({ agentId, groupIds });
      if (o.untouched === "none") return null;
      return o.untouched
        ? [
            {
              skillId: "s2",
              name: "frontend-design",
              path: "/home/u/.claude/skills/frontend-design",
              reason: "副本已被修改",
              tracked: false,
            },
          ]
        : [];
    };
    const enabledIds = (agentId: string) =>
      (active[agentId] ?? []).map((g) => g.groupId);
    handlers.set("set_agent_group", ({ agentId, groupId, enabled }) => {
      const ids = enabledIds(agentId as string);
      if (ids.includes(groupId as string) === enabled) return [];
      return write(
        agentId as string,
        enabled
          ? [...ids, groupId as string]
          : ids.filter((id) => id !== groupId),
      );
    });
    handlers.set("reapply_agent_groups", ({ agentId }) =>
      write(agentId as string, enabledIds(agentId as string)),
    );
    handlers.set("save_group", ({ groupId, name, skillIds }) => {
      if (o.saveError) throw new Error(o.saveError);
      const group = makeGroup({ id: groupId ?? "new", name, skillIds });
      groups = [...groups.filter((g) => g.id !== group.id), group];
      return group;
    });
    handlers.set("delete_group", ({ groupId }) => {
      groups = groups.filter((g) => g.id !== groupId);
      return null;
    });
    handlers.set("reorder_groups", ({ groupIds }) =>
      gated(o.reorderGate, () => {
        groups = (groupIds as string[]).map((id, sortOrder) => ({
          ...groups.find((g) => g.id === id)!,
          sortOrder,
        }));
        return groups;
      }),
    );
    handlers.set("list_projects", () => [
      makeProject({ id: "p1", name: "webapp", groupIds: ["g3"] }),
    ]);
  },
  render: () => renderWithProviders(<GroupsPage />),
  members: ["frontend-design", "pdf-tools"],
  issue: "yuque 已被手动停用",
  emptyHub: /Skill Hub 暂无 skill/,
  untouchedText: "frontend-design（副本已被修改）",
  savedWrites: [
    { agentId: "claude-code", groupIds: ["g1"] },
    { agentId: "codex", groupIds: ["g1", "g2"] },
  ],
  missing: "s-gone",
  gitRemaining: ["s4"],
  savedMembers: () =>
    (
      calls.filter((c) => c.command === "save_group").at(-1)?.args as
        { skillIds: string[] } | undefined
    )?.skillIds,
  reordered: () =>
    (
      calls.find((c) => c.command === "reorder_groups")?.args as
        { groupIds: string[] } | undefined
    )?.groupIds,
  activations: () => skillWrites,
};

const mcp = (
  id: string,
  name: string,
  mode: "gateway" | "direct",
  definition: Record<string, unknown>,
): ManagedMcp => ({
  id,
  name,
  mode,
  definition,
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
    .map((c) => (c.args as { params: Record<string, unknown> }).params);
let status: ManagementStatus;
const mcpBoard: Board = {
  name: "MCP",
  setup(o = {}) {
    setTarget({ id: "local", name: "本机", connected: true });
    handlers.set("list_agents", () => [claudeAgent, codexAgent]);
    const hf = mcp("hf", "hf-mcp-server", "gateway", {
      type: "http",
      url: "https://huggingface.co/mcp",
    });
    const work = mcp("work", "work-tools", "direct", {
      type: "stdio",
      command: "work",
    });
    const ctx = mcp("ctx", "context7", "direct", {
      type: "stdio",
      command: "context7",
    });
    status = {
      running: false,
      servers: [],
      discovered: [],
      scanWarnings: [],
      entries: o.emptyHub ? [] : [hf, work, ctx],
      groups:
        o.groups === "none"
          ? []
          : [
              {
                id: "g1",
                name: "前端",
                entryIds: ["hf", "work"],
                sortOrder: 0,
              },
              { id: "g2", name: "LLM", entryIds: ["ctx"], sortOrder: 1 },
              {
                id: "g3",
                name: "Git 工具",
                entryIds: ["work", "gone"],
                sortOrder: 2,
              },
              { id: "g4", name: "空分组", entryIds: [], sortOrder: 3 },
            ],
      activeGroups: {
        claude: [{ groupId: "g1", entries: [hf], bindings: [] }],
        codex: [
          { groupId: "g1", entries: [hf, work], bindings: [] },
          { groupId: "g2", entries: [ctx], bindings: [] },
        ],
      },
      // 按 Agent、再按分组：只有 LLM 的接入在 Codex 上被外部改过
      groupIssues: {
        codex: {
          g2: "MCP「context7」的配置已在外部修改或移除，请先核对 /home/u/.codex/config.toml",
        },
      },
    };
    handlers.set("mcp_request", ({ method, params }) => {
      if (method === "list") return listed(o.list, status);
      if (method === "activateGroup") {
        if (o.activateError && params.agent === "codex")
          throw new Error(o.activateError);
        const ids = params.ids as string[];
        status = {
          ...status,
          activeGroups: {
            ...status.activeGroups,
            [params.agent as string]: ids.map((groupId) => ({
              groupId,
              entries: [],
              bindings: [],
            })),
          },
        };
        if (o.untouched === "none") return null;
        return o.untouched
          ? [
              {
                agent: "claude",
                path: "/home/u/.claude.json",
                project: null,
                key: "hf-mcp-server",
                tracked: false,
              },
            ]
          : [];
      }
      if (method === "saveGroup") {
        if (o.saveError) throw new Error(o.saveError);
        status = {
          ...status,
          groups: [
            ...(status.groups ?? []).filter((g) => g.id !== params.group.id),
            params.group,
          ],
        };
      }
      if (method === "removeGroup")
        status = {
          ...status,
          groups: (status.groups ?? []).filter((g) => g.id !== params.id),
        };
      if (method === "reorderGroups")
        return gated(o.reorderGate, () => {
          const ids = params.ids as string[];
          status = {
            ...status,
            groups: ids.map((id, sortOrder) => ({
              ...status.groups!.find((g) => g.id === id)!,
              sortOrder,
            })),
          };
          return null;
        });
      return null;
    });
  },
  render: () => renderWithProviders(<McpGroupsPage />),
  members: ["hf-mcp-server", "work-tools"],
  issue: "context7",
  emptyHub: /MCP Hub 暂无服务/,
  untouchedText: "Claude Code（/home/u/.claude.json）里的「hf-mcp-server」",
  savedWrites: [
    { agent: "claude", ids: ["g1"] },
    { agent: "codex", ids: ["g1", "g2"] },
  ],
  missing: "gone",
  gitRemaining: ["work"],
  savedMembers: () =>
    (mcpCalls("saveGroup").at(-1)?.group as { entryIds: string[] } | undefined)
      ?.entryIds,
  reordered: () => mcpCalls("reorderGroups")[0]?.ids as string[] | undefined,
  activations: () => mcpCalls("activateGroup"),
};

const card = (name: string) =>
  screen.getByRole("button", { name: `编辑 ${name}` });

describe.each([skillBoard, mcpBoard])("$name 分组页", (board) => {
  const spy = (method: "success" | "error") => vi.spyOn(toast, method);
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
    await screen.findByRole("button", { name: "编辑 前端" });
  }

  it("工具栏、卡片和整页编辑用同一套文案", async () => {
    await open();
    expect(
      screen.getByRole("textbox", { name: "搜索分组…" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "新建分组" })).toBeVisible();
    expect(
      screen.getByRole("button", { name: "拖拽排序 前端" }),
    ).toHaveAttribute("title", "拖拽排序（也可按空格后用方向键移动）");
    fireEvent.click(card("前端"));
    // 在页内展开整页编辑，不是弹窗
    expect(
      await screen.findByRole("textbox", { name: "分组名称" }),
    ).toHaveAttribute("placeholder", "例如：前端开发");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(
      screen.getByRole("heading", { name: /^选择 \S+ · 已选 2 个$/ }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "返回" })).toBeInTheDocument();
  });

  it("读取中显示「正在读取分组…」，不会先显示没有分组", async () => {
    board.setup({ list: "pending" });
    board.render();
    expect(await screen.findByText("正在读取分组…")).toBeInTheDocument();
    expect(screen.queryByText("还没有分组")).toBeNull();
  });

  it("读取失败时说明原因", async () => {
    board.setup({ list: new Error("磁盘不可读") });
    board.render();
    const alert = await screen.findByText(/读取分组失败/);
    expect(alert).toHaveAttribute("role", "alert");
    expect(alert).toHaveTextContent("磁盘不可读");
    expect(screen.queryByText("还没有分组")).toBeNull();
  });

  it("没有分组时给出空状态，可以直接新建", async () => {
    board.setup({ groups: "none" });
    board.render();
    expect(await screen.findByText("还没有分组")).toBeInTheDocument();
    const create = screen.getAllByRole("button", { name: "新建分组" });
    expect(create).toHaveLength(2);
    fireEvent.click(create[1]);
    expect(
      await screen.findByRole("textbox", { name: "分组名称" }),
    ).toHaveValue("");
  });

  it("成员已缺失时卡片标出，Agent 图标不能启用并说明原因", async () => {
    await open();
    const git = card("Git 工具");
    expect(within(git).getByText("成员已缺失")).toBeInTheDocument();
    const icon = within(git).getByRole("button", {
      name: "为 Claude Code 启用「Git 工具」",
    });
    expect(icon).toHaveAttribute("aria-disabled", "true");
    fireEvent.focus(icon);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "成员已缺失，请先编辑分组",
    );
    fireEvent.click(icon);
    expect(board.activations()).toEqual([]);
  });

  it("「需要检查」只标在出问题的分组上，并写明原因", async () => {
    await open();
    const badge = within(card("LLM")).getByText("需要检查");
    expect(badge.getAttribute("title")).toContain(board.issue);
    // 前端同样被 Codex 使用，但它自己的部署没有问题
    const frontend = card("前端");
    expect(within(frontend).queryByText("需要检查")).toBeNull();
    expect(within(frontend).getByText("使用中")).toBeInTheDocument();
    expect(within(frontend).getByText("有待应用修改")).toBeInTheDocument();
  });

  it("拖拽排序松手即按新顺序显示，保存成功不另外提示", async () => {
    let release!: () => void;
    const reorderGate = new Promise<void>((resolve) => (release = resolve));
    await open({ reorderGate });
    const handle = screen.getByRole("button", { name: "拖拽排序 前端" });
    const handles = screen.getAllByRole("button", { name: /拖拽排序/ });
    // jsdom 没有布局：给可排序的节点一个真实的卡片尺寸
    const nodes = handles.map((h) => h.parentElement!.parentElement!);
    const rect = vi
      .spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockImplementation(function (this: HTMLElement) {
        const top = Math.max(0, nodes.indexOf(this)) * 100;
        return {
          x: 0,
          y: top,
          top,
          bottom: top + 80,
          left: 0,
          right: 800,
          width: 800,
          height: 80,
          toJSON: () => ({}),
        };
      });
    handle.focus();
    fireEvent.keyDown(handle, { code: "Space" });
    await waitFor(() => expect(handle).toHaveAttribute("aria-pressed", "true"));
    fireEvent.keyDown(document, { code: "ArrowDown" });
    fireEvent.keyDown(document, { code: "Space" });
    rect.mockRestore();
    await waitFor(() =>
      expect(board.reordered()).toEqual(["g2", "g1", "g3", "g4"]),
    );
    // 后端还没返回，列表已经是新顺序
    expect(
      screen.getAllByRole("button", { name: /拖拽排序/ })[0],
    ).toHaveAccessibleName("拖拽排序 LLM");
    release();
    await waitFor(() =>
      expect(
        screen.getAllByRole("button", { name: /拖拽排序/ })[1],
      ).toHaveAccessibleName("拖拽排序 前端"),
    );
    expect(success).not.toHaveBeenCalled();
  });

  it("删除后提示「分组已删除」", async () => {
    await open();
    fireEvent.click(
      within(card("空分组")).getByRole("button", { name: "删除 空分组" }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: "删除分组「空分组」",
    });
    expect(dialog).toHaveTextContent(/^.*只删除分组记录/);
    fireEvent.click(within(dialog).getByRole("button", { name: "删除" }));
    await waitFor(() => expect(success).toHaveBeenCalledWith("分组已删除"));
    expect(screen.queryByRole("button", { name: "编辑 空分组" })).toBeNull();
  });

  it("启用后提示到 Agent 里重新加载", async () => {
    await open();
    fireEvent.click(
      within(card("LLM")).getByRole("button", {
        name: "为 Claude Code 启用「LLM」",
      }),
    );
    await waitFor(() => expect(success).toHaveBeenCalledTimes(1));
    expect(String(success.mock.calls[0][0])).toMatch(
      /^分组已启用，请在 Agent 中重新加载/,
    );
  });

  it("停用时被改过的部署原样留下，提示里列出留下了哪几处", async () => {
    const warning = vi.spyOn(toast, "warning");
    await open({ untouched: true });
    fireEvent.click(
      within(card("前端")).getByRole("button", {
        name: "为 Claude Code 停用「前端」",
      }),
    );
    await waitFor(() => expect(warning).toHaveBeenCalledTimes(1));
    const message = String(warning.mock.calls[0][0]);
    expect(message).toMatch(/^分组已停用，请在 Agent 中重新加载/);
    expect(message).toContain(board.untouchedText);
    expect(message).toContain("Studio 没有删除或覆盖");
    expect(success).not.toHaveBeenCalled();
    warning.mockRestore();
  });

  it("后端没有返回留下的列表时，照常提示成功，不当成失败", async () => {
    await open({ untouched: "none" });
    fireEvent.click(
      within(card("LLM")).getByRole("button", {
        name: "为 Claude Code 启用「LLM」",
      }),
    );
    await waitFor(() => expect(success).toHaveBeenCalledTimes(1));
    expect(failure).not.toHaveBeenCalled();
  });

  it("后端没有返回留下的列表时，重新应用也照常提示成功", async () => {
    await open({ untouched: "none" });
    fireEvent.click(
      within(card("LLM")).getByRole("button", { name: "重新应用「LLM」" }),
    );
    await waitFor(() => expect(success).toHaveBeenCalledTimes(1));
    expect(failure).not.toHaveBeenCalled();
  });

  it("编辑时已选的成员排在最前；保存出错写在标题栏下方，修改保留", async () => {
    await open({ saveError: "已存在同名分组" });
    fireEvent.click(card("前端"));
    const boxes = await screen.findAllByRole("checkbox");
    expect(
      boxes.slice(0, 2).map((box) => box.getAttribute("aria-label")),
    ).toEqual(board.members);
    expect(boxes[0]).toBeChecked();
    expect(boxes[1]).toBeChecked();
    expect(boxes[2]).not.toBeChecked();
    fireEvent.click(boxes[2]);
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "保存失败，修改已保留：已存在同名分组",
    );
    expect(failure).not.toHaveBeenCalled();
    expect(screen.getAllByRole("checkbox")[2]).toBeChecked();
    expect(screen.getByRole("button", { name: "保存" })).toBeEnabled();
  });

  it("Hub 里还没有内容时，新建时说明要先添加", async () => {
    await open({ emptyHub: true });
    fireEvent.click(screen.getByRole("button", { name: "新建分组" }));
    expect(await screen.findByText(board.emptyHub)).toBeInTheDocument();
  });

  it("保存即写入正在使用这个分组的 Agent，并提示写入了哪些", async () => {
    await open();
    fireEvent.click(card("前端"));
    // 加一个原本不在组里的成员
    fireEvent.click((await screen.findAllByRole("checkbox"))[2]);
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(success).toHaveBeenCalledTimes(1));
    expect(String(success.mock.calls[0][0])).toMatch(
      /^已保存并写入 Claude Code、Codex，请在 Agent 中重新加载/,
    );
    expect(board.activations()).toEqual(board.savedWrites);
    // 留在这一页，修改已经存下
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
  });

  it("没有 Agent 在用的分组，保存时只保存", async () => {
    await open();
    fireEvent.click(card("空分组"));
    fireEvent.click((await screen.findAllByRole("checkbox"))[0]);
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() =>
      expect(success).toHaveBeenCalledWith(
        "分组已保存，可在卡片上为需要的 Agent 启用",
      ),
    );
    expect(board.activations()).toEqual([]);
  });

  it("新建的分组保存后留在这一页，标题栏的 Agent 图标随即可用", async () => {
    await open();
    fireEvent.click(screen.getByRole("button", { name: "新建分组" }));
    const name = await screen.findByRole("textbox", { name: "分组名称" });
    // 还没保存：没有可以启用的分组，悬停说明要先保存
    const pending = screen.getByRole("button", {
      name: "为 Claude Code 启用「新分组」",
    });
    expect(pending).toHaveAttribute("aria-disabled", "true");
    fireEvent.focus(pending);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "先保存分组，再为 Agent 启用",
    );
    fireEvent.change(name, { target: { value: "新组" } });
    fireEvent.click(screen.getAllByRole("checkbox")[0]);
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() =>
      expect(success).toHaveBeenCalledWith(
        "分组已保存，可在卡片上为需要的 Agent 启用",
      ),
    );
    const icon = await screen.findByRole("button", {
      name: "为 Claude Code 启用「新组」",
    });
    expect(icon).toBeEnabled();
    expect(icon).not.toHaveAttribute("aria-disabled");
    fireEvent.click(icon);
    await waitFor(() => expect(board.activations()).toHaveLength(1));
    expect(screen.getByRole("textbox", { name: "分组名称" })).toHaveValue(
      "新组",
    );
  });

  it("有未保存的修改时，标题栏的 Agent 图标先停用；返回要确认放弃", async () => {
    await open();
    fireEvent.click(card("前端"));
    const name = await screen.findByRole("textbox", { name: "分组名称" });
    const stop = () =>
      screen.getByRole("button", { name: "为 Claude Code 停用「前端」" });
    expect(stop()).not.toHaveAttribute("aria-disabled");
    fireEvent.change(name, { target: { value: "前端开发" } });
    expect(stop()).toHaveAttribute("aria-disabled", "true");
    fireEvent.focus(stop());
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "先保存修改，再为 Agent 启停",
    );
    fireEvent.click(stop());
    expect(board.activations()).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "放弃修改并离开" }),
    );
    expect(
      await screen.findByRole("button", { name: "编辑 前端" }),
    ).toBeInTheDocument();
    expect(board.savedMembers()).toBeUndefined();
  });

  it("写入某个 Agent 失败时分组已经保存：原因写在标题栏下方，不算未保存", async () => {
    await open({ activateError: "配置被锁定" });
    fireEvent.click(card("前端"));
    fireEvent.click((await screen.findAllByRole("checkbox"))[2]);
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "分组已保存，写入 Codex 时失败：配置被锁定",
    );
    expect(failure).not.toHaveBeenCalled();
    expect(board.savedMembers()).toHaveLength(3);
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    expect(screen.getAllByRole("checkbox")[2]).toBeChecked();
  });

  it("已缺失的成员仍列出来，取消勾选即可移除", async () => {
    await open();
    fireEvent.click(card("Git 工具"));
    const gone = await screen.findByRole("checkbox", {
      name: `已缺失：${board.missing}`,
    });
    expect(gone).toBeChecked();
    fireEvent.click(gone);
    expect(
      screen.queryByRole("checkbox", { name: `已缺失：${board.missing}` }),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() =>
      expect(board.savedMembers()).toEqual(board.gitRemaining),
    );
  });
});

it("Skill 分组里被本地改过的副本标出需要检查：Studio 不会覆盖它", async () => {
  skillBoard.setup();
  handlers.set("scan_skills", () => [
    makeSkill({
      id: "s3",
      name: "yuque",
      agents: {
        "claude-code": agentState("notLinked"),
        codex: agentState("copyModified"),
      },
    }),
  ]);
  skillBoard.render();
  await screen.findByRole("button", { name: "编辑 LLM" });
  expect(
    within(card("LLM")).getByText("需要检查").getAttribute("title"),
  ).toContain("yuque 的副本已被修改");
});

it("删除被项目绑定的 Skill 分组时，确认框列出这些项目", async () => {
  skillBoard.setup();
  skillBoard.render();
  await screen.findByRole("button", { name: "编辑 Git 工具" });
  fireEvent.click(
    within(card("Git 工具")).getByRole("button", { name: "删除 Git 工具" }),
  );
  const dialog = await screen.findByRole("dialog", {
    name: "删除分组「Git 工具」",
  });
  await waitFor(() => expect(dialog).toHaveTextContent("webapp"));
});

it("MCP 分组页顶部不再单独列出问题，原因写在出问题的卡片上", async () => {
  mcpBoard.setup();
  mcpBoard.render();
  await screen.findByRole("button", { name: "编辑 LLM" });
  expect(screen.queryByText(/^Codex：MCP「context7」/)).toBeNull();
  expect(
    within(card("LLM")).getByText("需要检查").getAttribute("title"),
  ).toContain("Codex");
});

it("MCP 整页编辑里不能选的原有配置写明原因", async () => {
  mcpBoard.setup();
  status.discovered = [
    {
      id: "orphan",
      name: "orphan",
      server: null,
      issue: null,
      managedId: null,
      sources: [
        {
          id: "src-orphan",
          project: null,
          definition: {
            command: "/Applications/Skill Studio.app/skill-studio",
          },
          agent: "claude",
          path: "/home/u/.claude.json",
          scope: "用户全局",
          key: "orphan",
          enabled: true,
          gateway: true,
        },
      ],
    },
  ];
  mcpBoard.render();
  await screen.findByRole("button", { name: "编辑 前端" });
  fireEvent.click(screen.getByRole("button", { name: "新建分组" }));
  expect(
    await screen.findByRole("checkbox", { name: "orphan" }),
  ).toBeDisabled();
  expect(
    screen.getByText(
      "指向 Studio 网关的配置不能作为成员，请先在 MCP Hub 添加独立配置",
    ),
  ).toBeInTheDocument();
});
