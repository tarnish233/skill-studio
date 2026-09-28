import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import App from "@/App";
import {
  claudeAgent,
  codexAgent,
  handlers,
  calls,
  defaultSettings,
  makeGroup,
  makeSkill,
  makeProject,
} from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

function withAgents() {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("get_config", () => ({ activeGroups: {} }));
}

describe("窗口壳与侧栏导航", () => {
  it("hides disabled application navigation while keeping Hub available", async () => {
    withAgents();
    handlers.set("get_settings", () => ({
      ...defaultSettings(),
      disabledAgents: ["codex"],
    }));
    renderWithProviders(<App />);
    let nav = within(screen.getByRole("navigation", { name: "主导航" }));
    await waitFor(() => {
      expect(
        nav.getByRole("button", { name: /Claude Code/ }),
      ).toBeInTheDocument();
      expect(
        nav.queryByRole("button", { name: /Codex/ }),
      ).not.toBeInTheDocument();
    });
    expect(nav.getByRole("button", { name: "Skill Hub" })).toBeInTheDocument();
  });

  it("首次启动默认停在「Skill Hub」并高亮该项", async () => {
    withAgents();
    renderWithProviders(<App />);
    const active = await screen.findByRole("button", { current: "page" });
    expect(active.textContent).toBe("Skill Hub");
  });

  it("侧栏按 agent 列表动态生成", async () => {
    withAgents();
    renderWithProviders(<App />);
    await waitFor(() => {
      expect(
        within(screen.getByRole("navigation", { name: "主导航" })).getByRole(
          "button",
          { name: /Claude Code/ },
        ),
      ).toBeInTheDocument();
      expect(
        within(screen.getByRole("navigation", { name: "主导航" })).getByRole(
          "button",
          { name: /Codex/ },
        ),
      ).toBeInTheDocument();
    });
  });

  it("未安装的 agent 在侧栏标注出来", async () => {
    handlers.set("list_agents", () => [
      claudeAgent,
      { ...codexAgent, detected: false },
    ]);
    renderWithProviders(<App />);
    const codexNav = await within(
      screen.getByRole("navigation", { name: "主导航" }),
    ).findByRole("button", { name: /Codex/ });
    expect(codexNav.textContent).toContain("未装");
  });

  it("点击侧栏切换视图，并把选择持久化到 localStorage", async () => {
    withAgents();
    const user = userEvent.setup();
    renderWithProviders(<App />);
    await user.click(
      await within(
        screen.getByRole("navigation", { name: "主导航" }),
      ).findByRole("button", { name: /Codex/ }),
    );

    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Codex");
    expect(localStorage.getItem("skill-studio-view")).toBe("agent:codex");
  });

  it("恢复上次停留的视图", async () => {
    withAgents();
    localStorage.setItem("skill-studio-view", "projects");
    renderWithProviders(<App />);
    await waitFor(() =>
      expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
        "项目",
      ),
    );
  });

  it("主题切换会写入 localStorage 并给 <html> 加上对应 class", async () => {
    withAgents();
    const user = userEvent.setup();
    renderWithProviders(<App />);
    expect(screen.queryByRole("button", { name: "深色" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "设置" }));
    await user.click(await screen.findByRole("button", { name: "深色" }));
    expect(localStorage.getItem("skill-studio-theme")).toBe("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
  });

  it("启动期错误会以横幅提示，而不是静默吞掉", async () => {
    withAgents();
    handlers.set("get_init_error", () => "配置文件损坏，请从 backups/ 恢复");
    renderWithProviders(<App />);
    expect(await screen.findByText(/配置文件损坏/)).toBeInTheDocument();
  });
});

describe("设置视图", () => {
  it("进入设置后侧栏保持可见，返回键回到进入前停留的视图", async () => {
    withAgents();
    const user = userEvent.setup();
    renderWithProviders(<App />);
    await user.click(
      await within(
        screen.getByRole("navigation", { name: "主导航" }),
      ).findByRole("button", { name: /Codex/ }),
    );
    expect(
      screen.getByRole("navigation", { name: "主导航" }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "设置" }));
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("设置");
    expect(screen.getByRole("navigation", { name: "主导航" })).toBeVisible();

    await user.click(screen.getByRole("button", { name: "返回" }));
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Codex");
    expect(
      screen.getByRole("navigation", { name: "主导航" }),
    ).toBeInTheDocument();
  });

  it("设置是临时视图，不会被记成下次启动的落脚点", async () => {
    withAgents();
    const user = userEvent.setup();
    renderWithProviders(<App />);
    await user.click(screen.getByRole("button", { name: "设置" }));
    expect(localStorage.getItem("skill-studio-view")).toBe("library");
  });

  it("旧版本存下的 settings 不会让应用启动就停在设置页", async () => {
    withAgents();
    localStorage.setItem("skill-studio-view", "settings");
    renderWithProviders(<App />);
    await waitFor(() =>
      expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
        "Skill Hub",
      ),
    );
  });

  it("设置内容按顶部标签栏分页，默认停在「通用」", async () => {
    withAgents();
    const user = userEvent.setup();
    renderWithProviders(<App />);
    await user.click(screen.getByRole("button", { name: "设置" }));

    expect(await screen.findByText("默认链接方式")).toBeInTheDocument();
    expect(screen.queryByText("Agent 目录覆盖")).toBeNull();

    await user.click(screen.getByRole("tab", { name: "目录" }));
    expect(await screen.findByText("Agent 目录覆盖")).toBeInTheDocument();
    expect(screen.queryByText("默认链接方式")).toBeNull();
  });
});

it("navigation replaces the body immediately without waiting for exit animation", async () => {
  withAgents();
  renderWithProviders(<App />);
  await screen.findByRole("button", { name: "Skill Hub" });
  fireEvent.click(screen.getByRole("button", { name: "项目" }));
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("项目");
  expect(
    screen.getAllByRole("button", { name: "添加项目" }).length,
  ).toBeGreaterThan(0);
  fireEvent.click(screen.getByRole("button", { name: "设置" }));
  expect(await screen.findByText("默认链接方式")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "返回" }));
  expect(
    screen.getAllByRole("button", { name: "添加项目" }).length,
  ).toBeGreaterThan(0);
});

it("header search and create actions follow the current page without leaking searches", async () => {
  withAgents();
  handlers.set("scan_skills", () => [
    makeSkill({ name: "Alpha" }),
    makeSkill({ id: "s2", name: "Beta" }),
  ]);
  handlers.set("list_groups", () => [
    makeGroup({ name: "Dev", agentId: "codex" }),
    makeGroup({ id: "g2", name: "Write", agentId: "codex" }),
  ]);
  handlers.set("list_projects", () => [
    makeProject({ name: "Site" }),
    makeProject({ id: "p2", name: "Docs", root: "/work/docs" }),
  ]);
  renderWithProviders(<App />);
  const header = within(screen.getByRole("banner"));
  await screen.findByText("Alpha");
  fireEvent.change(header.getByRole("textbox"), { target: { value: "Alpha" } });
  expect(screen.queryByText("Beta")).toBeNull();
  fireEvent.click(header.getByRole("button", { name: "新增 skill" }));
  expect(
    screen.getByRole("heading", { name: "安装 skill" }),
  ).toBeInTheDocument();
  fireEvent.click(header.getByRole("button", { name: "返回" }));
  fireEvent.click(
    within(screen.getByRole("navigation")).getByRole("button", {
      name: /Codex/,
    }),
  );
  expect(header.getByRole("textbox")).toHaveValue("");
  await screen.findByText("Dev");
  fireEvent.change(header.getByRole("textbox"), { target: { value: "Write" } });
  expect(screen.queryByText("Dev")).toBeNull();
  fireEvent.click(header.getByRole("button", { name: "新建分组" }));
  expect(screen.getByLabelText("分组名称")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "取消" }));
  fireEvent.click(
    within(screen.getByRole("navigation")).getByRole("button", {
      name: "项目",
    }),
  );
  await screen.findByText("Site");
  fireEvent.change(header.getByRole("textbox"), { target: { value: "Docs" } });
  expect(screen.queryByText("Site")).toBeNull();
  expect(screen.getByText("Docs")).toBeInTheDocument();
  fireEvent.click(header.getByRole("button", { name: "添加项目" }));
  expect(screen.getByRole("dialog")).toBeInTheDocument();
});

it.each([false, true])(
  "manual skill preference persists or retains previous value on failure=%s",
  async (fail) => {
    withAgents();
    let settings = { ...defaultSettings(), preserveManualSkills: true };
    handlers.set("get_settings", () => settings);
    handlers.set("update_settings", (args) => {
      if (fail) throw new Error("disk full");
      settings = { ...settings, ...(args.patch as object) };
      return settings;
    });
    renderWithProviders(<App />);
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    const toggle = await screen.findByRole("switch", {
      name: "是否保留手动安装的 skill",
    });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    await waitFor(() =>
      expect(calls.find((c) => c.command === "update_settings")?.args).toEqual({
        patch: { preserveManualSkills: false },
      }),
    );
    await waitFor(() => expect(toggle).not.toBeDisabled());
    if (fail) expect(toggle).toBeChecked();
    else expect(toggle).not.toBeChecked();
    expect(
      within(screen.getByRole("banner")).queryByRole("textbox"),
    ).toBeNull();
  },
);

it("guards project drafts when leaving through the sidebar and search", async () => {
  withAgents();
  localStorage.setItem("skill-studio-view", "projects");
  handlers.set("list_projects", () => [makeProject()]);
  handlers.set("scan_skills", () => [makeSkill({ id: "a", name: "Alpha" })]);
  renderWithProviders(<App />);
  fireEvent.click(await screen.findByText("webapp"));
  fireEvent.click(await screen.findByRole("checkbox", { name: "Alpha" }));
  const header = within(screen.getByRole("banner"));
  fireEvent.click(
    within(screen.getByRole("complementary", { name: "侧边栏" })).getByRole(
      "button",
      { name: "设置" },
    ),
  );
  expect(await screen.findByRole("dialog")).toHaveTextContent("放弃未保存");
  fireEvent.click(screen.getByRole("button", { name: "继续编辑" }));
  fireEvent.change(header.getByRole("textbox"), { target: { value: "other" } });
  expect(await screen.findByRole("dialog")).toHaveTextContent("放弃未保存");
  fireEvent.click(screen.getByRole("button", { name: "放弃修改并离开" }));
  expect(screen.queryByRole("checkbox", { name: "Alpha" })).toBeNull();
  expect(calls.filter((c) => c.command === "update_project")).toHaveLength(0);
});

it("uses the selected Hub to scope Agent and project pages without a second resource selector", async () => {
  withAgents();
  handlers.set("list_projects", () => [
    makeProject({ id: "p", name: "Demo", root: "/work/demo" }),
  ]);
  const user = userEvent.setup();
  renderWithProviders(<App />);
  let nav = within(screen.getByRole("navigation", { name: "主导航" }));
  await user.click(await nav.findByRole("button", { name: "MCP Hub" }));
  await user.click(await nav.findByRole("button", { name: "Codex" }));
  expect(
    await screen.findByRole("button", { name: "新建 MCP 分组" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("tablist", { name: "资源类型" }),
  ).not.toBeInTheDocument();
  expect(nav.getByRole("button", { name: "MCP Hub" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await user.click(screen.getByRole("button", { name: "新建 MCP 分组" }));
  await user.type(
    screen.getByRole("textbox", { name: "MCP 分组名称" }),
    "取消的草稿",
  );
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", { name: "取消" }),
  );
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  await user.click(nav.getByRole("button", { name: "项目" }));
  await user.click(
    await screen.findByRole("button", { name: "查看 Demo 的 MCP" }),
  );
  expect(await screen.findByText("项目 MCP")).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "写入项目" }),
  ).not.toBeInTheDocument();
  expect(screen.queryByText("项目已有 skill")).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "设置" }));
  await user.click(screen.getByRole("button", { name: "返回" }));
  expect(screen.getByText("管理各项目的 MCP 接入")).toBeInTheDocument();
  nav = within(screen.getByRole("navigation", { name: "主导航" }));
  await user.click(nav.getByRole("button", { name: "Skill Hub" }));
  await user.click(nav.getByRole("button", { name: "Codex" }));
  expect(
    await screen.findByRole("button", { name: "新建分组" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("tab", { name: /^已配置 MCP/ }),
  ).not.toBeInTheDocument();
  await user.click(nav.getByRole("button", { name: "项目" }));
  await user.click(await screen.findByRole("button", { name: "Demo" }));
  expect(await screen.findByText("项目已有 skill")).toBeInTheDocument();
  expect(screen.queryByText("项目 MCP")).not.toBeInTheDocument();
});

it("restores MCP mode for a saved Agent page", async () => {
  withAgents();
  localStorage.setItem("skill-studio-view", "agent:codex");
  localStorage.setItem("skill-studio-resource", "mcp");
  renderWithProviders(<App />);
  expect(
    await screen.findByRole("button", { name: "新建 MCP 分组" }),
  ).toBeInTheDocument();
  expect(screen.queryByRole("tab", { name: "Skill" })).not.toBeInTheDocument();
});

it("hides MCP navigation and returns a saved MCP view to Skill management when disabled", async () => {
  withAgents();
  handlers.set("get_settings", () => ({
    ...defaultSettings(),
    manageMcp: false,
  }));
  localStorage.setItem("skill-studio-view", "mcp");
  localStorage.setItem("skill-studio-resource", "mcp");
  renderWithProviders(<App />);
  const nav = within(await screen.findByRole("navigation", { name: "主导航" }));
  await waitFor(() =>
    expect(nav.getByRole("button", { name: "Skill Hub" })).toHaveAttribute(
      "aria-current",
      "page",
    ),
  );
  expect(
    nav.queryByRole("button", { name: "MCP Hub" }),
  ).not.toBeInTheDocument();
  expect(screen.queryByText("网关已关闭")).not.toBeInTheDocument();
  expect(calls.filter((call) => call.command === "mcp_request")).toHaveLength(
    0,
  );
  expect(localStorage.getItem("skill-studio-resource")).toBe("skills");
});

it("returns from MCP settings to Skill Hub when management is turned off", async () => {
  withAgents();
  let settings = { ...defaultSettings(), manageMcp: true };
  handlers.set("get_settings", () => settings);
  handlers.set("update_settings", ({ patch }) => {
    settings = { ...settings, ...patch };
    return settings;
  });
  const user = userEvent.setup();
  renderWithProviders(<App />);
  const nav = within(screen.getByRole("navigation", { name: "主导航" }));
  await user.click(await nav.findByRole("button", { name: "MCP Hub" }));
  await user.click(screen.getByRole("button", { name: "设置" }));
  await user.click(
    await screen.findByRole("switch", { name: "启用 MCP 管理" }),
  );
  await waitFor(() => expect(settings.manageMcp).toBe(false));
  await user.click(screen.getByRole("button", { name: "返回" }));
  const skill = await within(
    screen.getByRole("navigation", { name: "主导航" }),
  ).findByRole("button", { name: "Skill Hub" });
  await waitFor(() => expect(skill).toHaveAttribute("aria-current", "page"));
  expect(
    screen.queryByRole("button", { name: "MCP Hub" }),
  ).not.toBeInTheDocument();
});

it("reuses shared Skill data and the page container when switching Agents, while manual refresh still fetches", async () => {
  withAgents();
  handlers.set("list_groups", () => [
    makeGroup({
      id: "claude-group",
      agentId: "claude-code",
      name: "Claude group",
    }),
    makeGroup({ id: "codex-group", agentId: "codex", name: "Codex group" }),
  ]);
  const user = userEvent.setup();
  renderWithProviders(<App />);
  const nav = within(screen.getByRole("navigation", { name: "主导航" }));
  await user.click(await nav.findByRole("button", { name: "Claude Code" }));
  await screen.findByText("Claude group");
  const frame = screen
    .getByText("Claude group")
    .closest("fieldset")?.firstElementChild;
  const tracked = [
    "list_groups",
    "get_config",
    "get_settings",
    "scan_skills",
    "list_skill_backups",
  ];
  const counts = () =>
    tracked.map(
      (command) => calls.filter((call) => call.command === command).length,
    );
  const before = counts();
  await user.click(nav.getByRole("button", { name: "Codex" }));
  await screen.findByText("Codex group");
  expect(
    screen.getByText("Codex group").closest("fieldset")?.firstElementChild,
  ).toBe(frame);
  expect(screen.queryByText("Claude group")).not.toBeInTheDocument();
  await user.click(nav.getByRole("button", { name: "Claude Code" }));
  await screen.findByText("Claude group");
  expect(counts()).toEqual(before);
  await user.click(screen.getByRole("button", { name: "重新扫描" }));
  await waitFor(() => expect(counts()[0]).toBeGreaterThan(before[0]));
});

it("reuses the MCP snapshot when switching Agents", async () => {
  withAgents();
  const user = userEvent.setup();
  renderWithProviders(<App />);
  const nav = within(screen.getByRole("navigation", { name: "主导航" }));
  await user.click(await nav.findByRole("button", { name: "MCP Hub" }));
  await screen.findByText("网关已关闭");
  await user.click(await nav.findByRole("button", { name: "Claude Code" }));
  await screen.findByRole("button", { name: "新建 MCP 分组" });
  const count = () =>
    calls.filter(
      (call) =>
        call.command === "mcp_request" &&
        (call.args as { method: string }).method === "list",
    ).length;
  const before = count();
  await user.click(nav.getByRole("button", { name: "Codex" }));
  await screen.findByRole("button", { name: "新建 MCP 分组" });
  await user.click(nav.getByRole("button", { name: "Claude Code" }));
  await screen.findByRole("button", { name: "新建 MCP 分组" });
  expect(count()).toBe(before);
});
