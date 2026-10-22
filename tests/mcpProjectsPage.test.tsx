import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { toast } from "sonner";
import { McpProjectsPage } from "@/pages/McpProjectsPage";
import { renderWithProviders } from "./utils/render";
import {
  calls,
  claudeAgent,
  codexAgent,
  handlers,
  makeProject,
  skillOnlyAgents,
} from "./mocks/tauri";
import { setTarget } from "@/lib/api/transport";
import type { ManagementStatus } from "@/lib/api/mcpManagement";
import { rows } from "@/lib/api/mcpManagement";

const p1 = makeProject({ id: "p1", name: "webapp", root: "/work/webapp" });
const p2 = makeProject({ id: "p2", name: "note", root: "/work/note" });

/** 托管接入写在 p1（Codex），另有一条 p2 里已有的、没纳入管理的配置（Claude） */
function status(): ManagementStatus {
  return {
    running: false,
    entries: [
      {
        id: "m1",
        name: "Work",
        mode: "direct",
        definition: { type: "stdio", command: "work" },
        oauth: false,
        clientId: null,
        scopes: [],
        bindings: [
          {
            id: "b1",
            agent: "codex",
            path: "/work/webapp/.mcp.json",
            project: p1.root,
            key: "work",
            original: null,
            installed: { type: "stdio", command: "work" },
          },
        ],
      },
    ],
    discovered: [
      {
        id: "d1",
        name: "Local only",
        server: null,
        issue: null,
        managedId: null,
        sources: [
          {
            id: "s1",
            project: p2.root,
            definition: { type: "stdio", command: "local" },
            agent: "claude",
            path: "/work/note/.mcp.json",
            scope: "项目",
            key: "local-only",
            enabled: true,
            gateway: false,
          },
        ],
      },
    ],
    servers: [],
    scanWarnings: [],
  };
}

function setup(edit: (status: ManagementStatus) => void = () => {}) {
  setTarget({ id: "local", name: "本机", connected: true });
  handlers.set("list_agents", () => [
    claudeAgent,
    codexAgent,
    ...skillOnlyAgents,
  ]);
  handlers.set("list_projects", () => [p1, p2]);
  const current = status();
  edit(current);
  handlers.set("mcp_request", (args) => {
    const method = (args as { method?: string })?.method;
    if (method === "list") return current;
    if (method === "saveProject") return [];
    return null;
  });
}
const mcpCall = (method: string) =>
  (
    calls.find(
      (c) =>
        c.command === "mcp_request" &&
        (c.args as { method: string }).method === method,
    )?.args as { params: unknown } | undefined
  )?.params;
const openDetail = async (name: string) => {
  renderWithProviders(<McpProjectsPage />);
  fireEvent.click(await screen.findByRole("button", { name: `编辑 ${name}` }));
  return screen.findByText("项目已有 MCP");
};

it("汇总统计：托管接入含项目自己的部署和 Hub 写在项目里的，未托管是项目里没纳入管理的", async () => {
  setup((s) => {
    s.projects = {
      p1: {
        root: p1.root,
        entryIds: ["m1"],
        groupIds: [],
        claudeLocal: false,
        active: {
          claude: {
            entries: [s.entries[0]],
            bindings: [
              {
                id: "m1",
                agent: "claude",
                path: "/work/webapp/.mcp.json",
                project: null,
                key: "Work",
                original: null,
                installed: { type: "stdio", command: "work" },
              },
            ],
          },
        },
      },
    };
  });
  renderWithProviders(<McpProjectsPage />);
  await screen.findByText("webapp");
  expect(screen.getByRole("button", { name: "全部 2" })).toBeInTheDocument();
  expect(screen.getByText("托管接入 2")).toBeInTheDocument();
  // 统计栏一枚、note 卡片上一枚
  expect(screen.getAllByText("未托管 1")).toHaveLength(2);
  expect(screen.getByText("已启用 1")).toBeInTheDocument();
  // 胶囊上的数字是启用了这个 Agent 的项目数
  const group = within(screen.getByRole("group", { name: "按 Agent 查看" }));
  expect(group.getByRole("button", { name: "Claude Code 1" })).toBeVisible();
  expect(group.getByRole("button", { name: "Codex 0" })).toBeVisible();
});

it("项目已有 MCP：Hub 写在项目里的可以一起移除，被其他程序改写的那处原样保留并说明", async () => {
  setup();
  const warning = vi.spyOn(toast, "warning");
  handlers.set("mcp_request", (args) => {
    const method = (args as { method?: string })?.method;
    if (method === "list") return status();
    if (method === "removeProjectBindings")
      return {
        removed: 0,
        foreign: [
          {
            agent: "codex",
            path: "/work/webapp/.mcp.json",
            project: p1.root,
            key: "work",
          },
        ],
      };
    return null;
  });
  await openDetail("webapp");
  expect(screen.getByText("Hub 托管")).toBeVisible();
  expect(
    screen.getByRole("button", { name: "移除 Work 的 Codex 接入" }),
  ).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: /移除 Hub 接入 \(1\)/ }));
  const dialog = screen.getByRole("dialog", {
    name: "移除 webapp 的 Hub 接入？",
  });
  expect(dialog).toHaveTextContent("原样保留");
  fireEvent.click(
    within(dialog).getByRole("button", { name: "移除 Hub 接入" }),
  );
  await waitFor(() => expect(warning).toHaveBeenCalledTimes(1));
  const message = String(warning.mock.calls[0][0]);
  expect(message).toContain("Codex（/work/webapp/.mcp.json）里的同名配置");
  expect(message).not.toContain("已移除");
  warning.mockRestore();
});

it("项目已有 MCP：原有配置纳入管理时保留原位置，不写进别的 Agent", async () => {
  setup();
  await openDetail("note");
  expect(screen.getByText("原有配置")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "纳入管理" }));
  await waitFor(() =>
    expect(mcpCall("saveEntry")).toMatchObject({
      expectedEntry: null,
      sources: [{ id: "s1", definition: { type: "stdio", command: "local" } }],
      agents: [],
      projectId: "",
      scope: "user",
    }),
  );
});

it("Claude Code 改成项目本地后保存", async () => {
  setup();
  await openDetail("webapp");
  const trigger = screen.getByRole("combobox", {
    name: "Claude Code 保存位置",
  });
  expect(trigger).toHaveTextContent("项目配置");
  trigger.focus();
  fireEvent.keyDown(trigger, { key: "Enter" });
  fireEvent.click(await screen.findByRole("option", { name: "项目本地" }));
  expect(trigger).toHaveTextContent("项目本地");
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() =>
    expect(mcpCall("saveProject")).toMatchObject({
      projectId: "p1",
      claudeLocal: true,
    }),
  );
});

it("Claude Code 的保存位置随项目保存：读到「项目本地」，改了成员再保存时原样带上", async () => {
  setup((s) => {
    s.projects = {
      p1: {
        root: p1.root,
        entryIds: [],
        groupIds: [],
        claudeLocal: true,
        active: {},
      },
    };
  });
  await openDetail("webapp");
  expect(
    screen.getByRole("combobox", { name: "Claude Code 保存位置" }),
  ).toHaveTextContent("项目本地");
  fireEvent.click(screen.getByRole("checkbox", { name: "Work" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() =>
    expect(mcpCall("saveProject")).toEqual({
      projectId: "p1",
      entryIds: ["m1"],
      groupIds: [],
      claudeLocal: true,
      imports: [],
      // 打开详情时的样子：别处改过时后端拒绝保存
      expected: { entryIds: [], groupIds: [], claudeLocal: true },
    }),
  );
});

it("保存时别处改过：提示冲突，修改留在页面上；保存成功后以刚存的为准", async () => {
  setup();
  let conflict = true;
  const answer = handlers.get("mcp_request")!;
  handlers.set("mcp_request", (args) => {
    if (args.method === "saveProject" && conflict)
      throw new Error(
        "项目已被别处修改（另一个窗口或命令行），请按最新内容重新修改",
      );
    return answer(args);
  });
  await openDetail("webapp");
  fireEvent.click(screen.getByRole("checkbox", { name: "Work" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  expect(
    await screen.findByText(/写入失败，修改已保留：.*项目已被别处修改/),
  ).toBeInTheDocument();
  expect(screen.getByRole("checkbox", { name: "Work" })).toBeChecked();
  conflict = false;
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() =>
    expect(
      calls.filter(
        (c) =>
          c.command === "mcp_request" &&
          (c.args as { method: string }).method === "saveProject",
      ),
    ).toHaveLength(2),
  );
  fireEvent.click(screen.getByRole("checkbox", { name: "Work" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() =>
    expect(
      calls
        .filter(
          (c) =>
            c.command === "mcp_request" &&
            (c.args as { method: string }).method === "saveProject",
        )
        .map((c) => (c.args as { params: unknown }).params)
        .at(-1),
    ).toMatchObject({
      entryIds: [],
      expected: { entryIds: ["m1"], groupIds: [], claudeLocal: false },
    }),
  );
});

it("指向 Studio 网关的原有配置不能选作成员，并写明原因", async () => {
  setup((s) => {
    s.discovered[0].sources[0].gateway = true;
  });
  await openDetail("webapp");
  expect(screen.getByRole("checkbox", { name: "Local only" })).toBeDisabled();
  expect(
    screen.getByText(
      "指向 Studio 网关的配置不能作为成员，请先在 MCP Hub 添加独立配置",
    ),
  ).toBeVisible();
});

it("远程目标不读取本机的项目 MCP", () => {
  setTarget({ id: "remote", name: "远程", connected: true });
  renderWithProviders(<McpProjectsPage />);
  expect(screen.getByText(/项目 MCP 当前仅支持本机/)).toBeVisible();
  expect(calls.some((c) => c.command === "mcp_request")).toBe(false);
});

it("MCP 项目卡片按名字生成图标，点卡片进详情", async () => {
  setup();
  renderWithProviders(<McpProjectsPage />);
  await screen.findByText("webapp");
  // 不再共用同一个文件夹图标：首字来自项目名
  expect(screen.getByText("W")).toBeInTheDocument();
  expect(screen.getByText("N")).toBeInTheDocument();
  const card = screen.getByRole("button", { name: "编辑 webapp" });
  const transition = (el: HTMLElement) =>
    el.closest<HTMLElement>('[data-transition="detail"]');
  // 刚进入页面不叠加列表淡入
  expect(transition(card)).not.toHaveAttribute("data-appear");
  fireEvent.click(card);
  const content = await screen.findByText("项目已有 MCP");
  // 标题栏和内容在同一个过渡容器里一起淡入，与 Skill 项目详情一致
  const back = screen.getByRole("button", { name: "返回" });
  expect(transition(back)).toHaveAttribute("data-appear");
  expect(transition(content)).toBe(transition(back));
});

it("项目写进配置的接入算 Hub 里这个 MCP 的来源，不会另外显示成一条原有配置", () => {
  const current = status();
  const entry = current.entries[0];
  // 写入时 Codex 的 enabled 字段被改过：内容与 Hub 定义不完全一致，只能靠位置认出来
  const binding = {
    id: entry.id,
    agent: "codex",
    path: "/work/webapp/.codex/config.toml",
    project: null,
    key: "work",
    original: null,
    installed: { type: "stdio", command: "work", enabled: true },
  };
  current.projects = {
    p1: {
      root: p1.root,
      entryIds: [entry.id],
      groupIds: [],
      claudeLocal: false,
      active: { codex: { entries: [entry], bindings: [binding] } },
    },
  };
  current.discovered = [
    {
      id: "deployed",
      name: "work",
      sources: [
        {
          ...binding,
          id: "source",
          scope: "项目",
          enabled: true,
          gateway: false,
          definition: { ...binding.installed, command: "work --changed" },
        },
      ],
      server: null,
      issue: null,
      managedId: null,
    },
  ];
  const result = rows(current);
  expect(result.filter((r) => !r.managed)).toHaveLength(0);
  expect(result.find((r) => r.entry.id === entry.id)?.sources).toHaveLength(1);
});
