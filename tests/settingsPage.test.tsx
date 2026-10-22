import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useQuery } from "@tanstack/react-query";
import { describe, it, expect, vi } from "vitest";
import { managementApi } from "@/lib/api/mcpManagement";
import { NAVIGATION_STALE_TIME } from "@/lib/queryKeys";
import { SettingsPage } from "@/pages/SettingsPage";
import { renderWithProviders } from "./utils/render";
import {
  handlers,
  calls,
  defaultSettings,
  claudeAgent,
  codexAgent,
} from "./mocks/tauri";

it("only saves backup retention as an integer within 0..100 and never saves an empty input as zero", async () => {
  let settings = defaultSettings();
  handlers.set("get_settings", () => settings);
  handlers.set("update_settings", ({ patch }) => {
    settings = { ...settings, ...patch };
    return settings;
  });
  const user = userEvent.setup();
  renderWithProviders(<SettingsPage />);
  await user.click(await screen.findByRole("tab", { name: "维护" }));
  const input = screen.getByRole("spinbutton", { name: "配置备份保留份数" });
  for (const value of ["101", "-1", "1.5", ""]) {
    fireEvent.change(input, { target: { value } });
  }
  expect(calls.filter((c) => c.command === "update_settings")).toHaveLength(0);
  fireEvent.change(input, { target: { value: "25" } });
  await waitFor(() =>
    expect(calls.filter((c) => c.command === "update_settings")).toHaveLength(
      1,
    ),
  );
  expect(settings.backupKeep).toBe(25);
});

/** MCP Hub 顶部「已备份 N」读的就是这份 MCP 状态；导航时沿用快照，只有失效才会重读 */
function McpBackupCount() {
  const { data } = useQuery({
    queryKey: ["mcp", "local"],
    queryFn: () => managementApi.list(),
    staleTime: NAVIGATION_STALE_TIME,
  });
  return <p>MCP 已备份 {data?.backups?.length ?? "-"}</p>;
}

it("after lowering the retention, shows the backups the backend kept instead of ones it already deleted", async () => {
  let settings = { ...defaultSettings(), backupKeep: 3 };
  let backups = [
    "/tmp/backups/config-3.json",
    "/tmp/backups/config-2.json",
    "/tmp/backups/config-1.json",
  ];
  let mcpBackups = 3;
  handlers.set("get_settings", () => settings);
  handlers.set("list_backups", () => backups);
  handlers.set("mcp_request", ({ method }) =>
    method === "list"
      ? {
          running: false,
          entries: [],
          servers: [],
          discovered: [],
          scanWarnings: [],
          backups: Array.from({ length: mcpBackups }, (_, i) => ({
            id: `b${i}`,
            paths: [],
            createdAt: i,
          })),
        }
      : undefined,
  );
  // 后端保存时按新的份数轮转配置备份，并立即清理 MCP 备份
  handlers.set("update_settings", ({ patch }) => {
    settings = { ...settings, ...patch };
    backups = backups.slice(0, settings.backupKeep);
    mcpBackups = Math.min(mcpBackups, settings.backupKeep!);
    return settings;
  });
  const user = userEvent.setup();
  renderWithProviders(
    <>
      <SettingsPage />
      <McpBackupCount />
    </>,
  );
  await user.click(await screen.findByRole("tab", { name: "维护" }));
  expect(await screen.findByText("config-1.json")).toBeInTheDocument();
  expect(await screen.findByText("MCP 已备份 3")).toBeInTheDocument();
  fireEvent.change(
    screen.getByRole("spinbutton", { name: "配置备份保留份数" }),
    { target: { value: "1" } },
  );
  await waitFor(() => expect(screen.queryByText("config-1.json")).toBeNull());
  expect(screen.queryByText("config-2.json")).toBeNull();
  expect(screen.getByText("config-3.json")).toBeInTheDocument();
  expect(await screen.findByText("MCP 已备份 1")).toBeInTheDocument();
});

it("says why full backups cannot be listed instead of claiming there are none", async () => {
  // 恢复模式下后端拒绝列出：配置没加载，判断不了哪份仍被引用
  handlers.set("list_hub_backups", () => {
    throw "配置尚未正常加载，请先恢复配置备份并重启应用";
  });
  const user = userEvent.setup();
  renderWithProviders(<SettingsPage />);
  await user.click(await screen.findByRole("tab", { name: "维护" }));
  expect(await screen.findByText(/配置尚未正常加载/)).toBeInTheDocument();
  expect(screen.queryByText(/暂无完整备份/)).toBeNull();
  // 完整备份卡片的「N 份 · 大小」也不该给出一个 0
  expect(screen.queryByText(/^0 份 · /)).toBeNull();
});

it("backup restore requires confirmation and cancel does not restore", async () => {
  handlers.set("list_backups", () => ["/tmp/config-2026.json"]);
  const user = userEvent.setup();
  renderWithProviders(<SettingsPage />);
  await user.click(await screen.findByRole("tab", { name: "维护" }));
  await user.click(await screen.findByRole("button", { name: "恢复" }));
  expect(await screen.findByRole("dialog")).toHaveTextContent("从备份恢复配置");
  await user.click(screen.getByRole("button", { name: "取消" }));
  expect(calls.filter((c) => c.command === "restore_backup")).toHaveLength(0);
});

it("deletes a config backup only after confirmation", async () => {
  handlers.set("list_backups", () => ["/tmp/backups/config-2026.json"]);
  handlers.set("delete_backup", () => undefined);
  const user = userEvent.setup();
  renderWithProviders(<SettingsPage />);
  await user.click(await screen.findByRole("tab", { name: "维护" }));
  await user.click(
    await screen.findByRole("button", { name: "删除 config-2026.json" }),
  );
  const dialog = await screen.findByRole("dialog", {
    name: "删除这份配置备份？",
  });
  expect(calls.filter((c) => c.command === "delete_backup")).toHaveLength(0);
  await user.click(within(dialog).getByRole("button", { name: "删除" }));
  await waitFor(() =>
    expect(calls.find((c) => c.command === "delete_backup")?.args).toEqual({
      path: "/tmp/backups/config-2026.json",
    }),
  );
});

it("keeps the full backup that restores a skill and deletes unused ones", async () => {
  const backup = (id: string, operation: string, inUse: boolean) => ({
    id,
    name: id === "c" ? "other" : "demo",
    operation,
    originalPath: `/home/u/.claude/skills/${id === "c" ? "other" : "demo"}`,
    createdAt: 1_791_950_000,
    size: 4096,
    inUse,
  });
  handlers.set("list_hub_backups", () => [
    backup("a", "adopt", true),
    backup("b", "release", false),
    backup("c", "adopt", false),
  ]);
  handlers.set("delete_hub_backups", ({ ids }) => ids.length);
  const user = userEvent.setup();
  renderWithProviders(<SettingsPage />);
  await user.click(await screen.findByRole("tab", { name: "维护" }));
  expect(await screen.findByText("3 份 · 12 KB")).toBeInTheDocument();
  expect(screen.getByText("使用中")).toBeVisible();
  // 被引用的那份没有删除按钮：两份 demo 里只有移出 Hub 时的那份能删
  expect(
    screen.getAllByRole("button", { name: "删除 demo 的完整备份" }),
  ).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: "删除未使用的 2 份" }));
  const dialog = await screen.findByRole("dialog", {
    name: "删除未使用的 2 份完整备份？",
  });
  await user.click(within(dialog).getByRole("button", { name: "删除" }));
  await waitFor(() =>
    expect(calls.find((c) => c.command === "delete_hub_backups")?.args).toEqual(
      { ids: ["b", "c"] },
    ),
  );
});

it("does not persist partial Hub paths while typing", async () => {
  const user = userEvent.setup();
  renderWithProviders(<SettingsPage />);
  await user.click(await screen.findByRole("tab", { name: "目录" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Hub 目录" }), {
    target: { value: "/tmp/new-hub" },
  });
  expect(calls.filter((c) => c.command === "update_settings")).toHaveLength(0);
  await user.click(screen.getByRole("button", { name: "保存目录" }));
  await waitFor(() =>
    expect(calls.filter((c) => c.command === "update_settings")).toHaveLength(
      1,
    ),
  );
  expect(calls.find((c) => c.command === "update_settings")?.args).toEqual({
    patch: { hubDir: "/tmp/new-hub" },
  });
});

it("persists application toggles and allows all applications to be hidden and restored", async () => {
  let settings = defaultSettings();
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  handlers.set("get_settings", () => settings);
  handlers.set("update_settings", ({ patch }) => {
    settings = { ...settings, ...patch };
    return settings;
  });
  const user = userEvent.setup();
  renderWithProviders(<SettingsPage />);
  const claude = await screen.findByRole("button", { name: "Claude Code" });
  const codex = screen.getByRole("button", { name: "Codex" });
  expect(claude).toHaveAttribute("aria-pressed", "true");
  await user.click(claude);
  await waitFor(() => expect(claude).toHaveAttribute("aria-pressed", "false"));
  await waitFor(() => expect(codex).toHaveAttribute("aria-disabled", "false"));
  await user.click(codex);
  await waitFor(() =>
    expect(settings.disabledAgents).toEqual(["claude-code", "codex"]),
  );
  await waitFor(() => expect(codex).toHaveAttribute("aria-disabled", "false"));
  await user.click(codex);
  await waitFor(() => expect(settings.disabledAgents).toEqual(["claude-code"]));
});

it("defaults builtin MCP visibility off and saves the display preference", async () => {
  let settings = { ...defaultSettings(), showCodexBuiltinMcp: false };
  handlers.set("get_settings", () => settings);
  handlers.set("update_settings", ({ patch }) => {
    settings = { ...settings, ...patch };
    return settings;
  });
  renderWithProviders(<SettingsPage />);
  const toggle = await screen.findByRole("switch", {
    name: "显示 Codex App 内置 MCP",
  });
  expect(toggle).not.toBeChecked();
  fireEvent.click(toggle);
  await waitFor(() => expect(toggle).toBeChecked());
  expect(settings.showCodexBuiltinMcp).toBe(true);
  fireEvent.click(toggle);
  await waitFor(() => expect(toggle).not.toBeChecked());
  expect(settings.showCodexBuiltinMcp).toBe(false);
});

it("switches MCP management independently and hides MCP-only preferences while off", async () => {
  let settings = { ...defaultSettings(), manageMcp: true };
  handlers.set("get_settings", () => settings);
  handlers.set("update_settings", ({ patch }) => {
    settings = { ...settings, ...patch };
    return settings;
  });
  const user = userEvent.setup();
  renderWithProviders(<SettingsPage />);
  const toggle = await screen.findByRole("switch", { name: "启用 MCP 管理" });
  expect(toggle).toBeChecked();
  await user.click(toggle);
  await waitFor(() => expect(toggle).not.toBeChecked());
  expect(
    screen.queryByRole("switch", { name: "显示 Codex App 内置 MCP" }),
  ).not.toBeInTheDocument();
  expect(
    calls.find((call) => call.command === "update_settings")?.args,
  ).toEqual({ patch: { manageMcp: false } });
  await user.click(toggle);
  await waitFor(() => expect(toggle).toBeChecked());
  expect(
    screen.getByRole("switch", { name: "显示 Codex App 内置 MCP" }),
  ).toBeInTheDocument();
});

it("keeps Agent buttons and focus stable while saving and rolls back failed changes", async () => {
  handlers.set("list_agents", () => [claudeAgent, codexAgent]);
  let rejectSave: (error: Error) => void = () => {};
  handlers.set(
    "update_settings",
    () =>
      new Promise((_, reject) => {
        rejectSave = reject;
      }),
  );
  renderWithProviders(<SettingsPage />);
  const button = await screen.findByRole("button", { name: "Codex" });
  button.focus();
  fireEvent.click(button);
  await waitFor(() => expect(button).toHaveAttribute("aria-pressed", "false"));
  expect(screen.getByRole("button", { name: "Codex" })).toBe(button);
  expect(button).toHaveFocus();
  expect(button).not.toBeDisabled();
  expect(button).toHaveAttribute("aria-disabled", "true");
  expect(
    screen.getByRole("switch", { name: "是否保留手动安装的 skill" }),
  ).toBeEnabled();
  await act(async () => rejectSave(new Error("disk full")));
  await waitFor(() => expect(button).toHaveAttribute("aria-pressed", "true"));
  expect(button).toHaveFocus();
  expect(
    calls.filter((c) => c.command === "list_agents").map((c) => c.args),
  ).toEqual([{ skipCliProbe: true }, { skipCliProbe: false }]);
  expect(calls.filter((c) => c.command === "get_config_dir")).toHaveLength(1);
});

describe("关于 › 命令行工具", () => {
  const clipboard = () => {
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    return writeText;
  };
  async function openAbout() {
    const user = userEvent.setup();
    renderWithProviders(<SettingsPage />);
    await user.click(await screen.findByRole("tab", { name: "关于" }));
    return user;
  }

  it("Homebrew 链接好之后：说明终端和 Agent 能直接运行，并能复制示例命令", async () => {
    handlers.set("get_cli_info", () => ({
      bundled: "/Applications/Skill Studio.app/Contents/MacOS/skill-studio-cli",
      command: "skill-studio",
      location: "/opt/homebrew/bin/skill-studio",
      stable: true,
    }));
    const user = await openAbout();
    // user-event 会换上自己的剪贴板，所以在它之后再装
    const writeText = clipboard();
    expect(
      await screen.findByText(/终端和 Agent 可以直接运行 skill-studio/),
    ).toBeVisible();
    expect(screen.getByText("/opt/homebrew/bin/skill-studio")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "复制示例命令" }));
    expect(writeText).toHaveBeenCalledWith("skill-studio status");
  });

  it("还没放进 PATH：给出 App 自带程序的链接命令，路径带引号可直接运行", async () => {
    handlers.set("get_cli_info", () => ({
      bundled: "/Applications/Skill Studio.app/Contents/MacOS/skill-studio-cli",
      command: null,
      location: null,
      stable: true,
    }));
    const user = await openAbout();
    const writeText = clipboard();
    const line =
      'ln -s "/Applications/Skill Studio.app/Contents/MacOS/skill-studio-cli" /usr/local/bin/skill-studio';
    expect(await screen.findByText(line)).toBeVisible();
    expect(screen.getByText(/用 Homebrew 安装或升级/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "复制" }));
    expect(writeText).toHaveBeenCalledWith(line);
  });

  it("从 AppImage 运行时不给链接命令，说明临时挂载点不能长期链接", async () => {
    handlers.set("get_cli_info", () => ({
      bundled: "/tmp/.mount_skill0/usr/bin/skill-studio-cli",
      command: null,
      location: null,
      stable: false,
    }));
    const user = await openAbout();
    await user.click(await screen.findByRole("tab", { name: "关于" }));
    expect(await screen.findByText(/当前是从 AppImage 运行的/)).toBeVisible();
    expect(screen.queryByText(/^ln -s/)).toBeNull();
  });

  it("这个版本没有附带命令行程序时不显示", async () => {
    handlers.set("get_cli_info", () => ({
      bundled: null,
      command: null,
      location: null,
      stable: false,
    }));
    await openAbout();
    expect(await screen.findByText("Agent 与项目")).toBeVisible();
    await waitFor(() =>
      expect(calls.some((c) => c.command === "get_cli_info")).toBe(true),
    );
    expect(screen.queryByText("命令行工具")).toBeNull();
  });
});
