import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { toast } from "sonner";
import { McpProjectsPage } from "@/pages/McpProjectsPage";
import { renderWithProviders } from "./utils/render";
import {
  claudeAgent,
  codexAgent,
  handlers,
  makeProject,
  skillOnlyAgents,
} from "./mocks/tauri";
import { setTarget } from "@/lib/api/transport";
import type { ManagementStatus } from "@/lib/api/mcpManagement";

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

function setup() {
  setTarget({ id: "local", name: "本机", connected: true });
  handlers.set("list_agents", () => [
    claudeAgent,
    codexAgent,
    ...skillOnlyAgents,
  ]);
  handlers.set("list_projects", () => [p1, p2]);
  handlers.set("mcp_request", (args) =>
    (args as { method?: string })?.method === "list" ? status() : null,
  );
}

it("汇总统计与 Agent 筛选：托管 / 未托管分开算，筛选能只留下该 Agent 的项目", async () => {
  setup();
  renderWithProviders(<McpProjectsPage />);
  await screen.findByText("webapp");
  // 总数在「全部」上；右侧统计：1 处托管接入（webapp 的 Codex）、1 处未托管（note 原有的 Claude 配置）
  expect(screen.getByRole("button", { name: "全部 2" })).toBeInTheDocument();
  expect(screen.getByText("托管接入 1")).toBeInTheDocument();
  expect(screen.getByText("未托管 1")).toBeInTheDocument();
  expect(screen.getByText("已启用 1")).toBeInTheDocument();
  // 左侧 Agent 胶囊带各自的项目数
  const group = within(
    screen.getByRole("group", { name: "按项目 Agent 筛选" }),
  );
  expect(
    group.getByRole("button", { name: "Claude Code 1" }),
  ).toBeInTheDocument();
  expect(group.getByRole("button", { name: "Codex 1" })).toBeInTheDocument();
  expect(group.getByRole("button", { name: "OpenCode 0" })).toBeInTheDocument();
  // 点胶囊只留该 Agent 配了 MCP 的项目，再点一次恢复
  fireEvent.click(group.getByRole("button", { name: "Codex 1" }));
  expect(screen.queryByText("note")).not.toBeInTheDocument();
  expect(screen.getByText("webapp")).toBeInTheDocument();
  fireEvent.click(group.getByRole("button", { name: "Codex 1" }));
  expect(await screen.findByText("note")).toBeInTheDocument();
});

it("移除项目接入时，已被其他程序改写的那处原样保留，并告诉用户是哪一处", async () => {
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
  renderWithProviders(<McpProjectsPage />);
  fireEvent.click(
    await screen.findByRole("button", { name: "查看 webapp 的 MCP" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /移除托管接入 \(1\)/ }),
  );
  const dialog = screen.getByRole("dialog", {
    name: "移除 webapp 的托管 MCP？",
  });
  expect(dialog).toHaveTextContent("原样保留");
  fireEvent.click(within(dialog).getByRole("button", { name: "移除托管接入" }));
  await waitFor(() => expect(warning).toHaveBeenCalledTimes(1));
  const message = String(warning.mock.calls[0][0]);
  expect(message).toContain("Codex（/work/webapp/.mcp.json）里的同名配置");
  // 一处都没移除时不说「已移除 0 处」
  expect(message).not.toContain("已移除");
  warning.mockRestore();
});

it("MCP 项目卡片按名字生成图标，点卡片进详情", async () => {
  setup();
  renderWithProviders(<McpProjectsPage />);
  await screen.findByText("webapp");
  // 不再共用同一个文件夹图标：首字来自项目名
  expect(screen.getByText("W")).toBeInTheDocument();
  expect(screen.getByText("N")).toBeInTheDocument();
  const card = screen.getByRole("button", { name: "查看 webapp 的 MCP" });
  const transition = (el: HTMLElement) =>
    el.closest<HTMLElement>('[data-transition="detail"]');
  // 刚进入页面不叠加列表淡入
  expect(transition(card)).not.toHaveAttribute("data-appear");
  fireEvent.click(card);
  const content = await screen.findByText("项目 MCP");
  // 标题栏和内容在同一个过渡容器里一起淡入，与 Skill 项目详情一致
  const back = screen.getByRole("button", { name: "返回" });
  expect(transition(back)).toHaveAttribute("data-appear");
  expect(transition(content)).toBe(transition(back));
});
