import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { McpBackupsDialog } from "@/components/mcp/McpBackupsDialog";
import type { McpBackupList } from "@/lib/api/mcpManagement";
import { calls, handlers } from "./mocks/tauri";
import { renderWithProviders } from "./utils/render";

const list: McpBackupList = {
  backups: [
    {
      id: "newest",
      createdAt: 1_791_950_000,
      paths: ["/home/u/.codex/config.toml"],
      size: 10_240,
      status: "restorable",
    },
    {
      id: "older",
      createdAt: 1_791_940_000,
      paths: ["/home/u/.codex/config.toml"],
      size: 10_240,
      status: "afterNewer",
      reason: "需先恢复更新的备份",
    },
    {
      id: "stale",
      createdAt: 1_791_930_000,
      paths: ["/home/u/.claude.json"],
      size: 105_472,
      status: "unrestorable",
      reason: "配置已在备份后修改：/home/u/.claude.json",
    },
  ],
  legacy: [],
};
const time = (seconds: number) => new Date(seconds * 1000).toLocaleString();
const mcpCalls = (method: string) =>
  calls.filter(
    (c) =>
      c.command === "mcp_request" &&
      (c.args as { method: string }).method === method,
  );

function setup(data: McpBackupList = list) {
  handlers.set("mcp_request", ({ method }) =>
    method === "listBackups" ? data : { removed: 1 },
  );
  const onChanged = vi.fn();
  renderWithProviders(
    <McpBackupsDialog open onOpenChange={() => {}} onChanged={onChanged} />,
  );
  return onChanged;
}

function confirmIn(title: string, button: string) {
  const dialog = screen.getByRole("dialog", { name: title });
  fireEvent.click(within(dialog).getByRole("button", { name: button }));
}

it("marks which backups can still be restored and offers restore only for those", async () => {
  const onChanged = setup();
  expect(
    await screen.findByText("共 3 次 · 可恢复 1 次 · 占用 123 KB"),
  ).toBeInTheDocument();
  expect(screen.getByText("可恢复")).toBeVisible();
  expect(screen.getByText("需先恢复更新的备份")).toBeVisible();
  expect(screen.getByText("无法恢复")).toBeVisible();
  expect(
    screen.getByText("配置已在备份后修改：/home/u/.claude.json"),
  ).toBeVisible();
  const restore = screen.getAllByRole("button", { name: "恢复" });
  expect(restore).toHaveLength(1);
  fireEvent.click(restore[0]);
  expect(mcpCalls("restoreBackup")).toHaveLength(0);
  confirmIn("恢复 MCP 配置备份？", "恢复");
  await waitFor(() =>
    expect(mcpCalls("restoreBackup")[0]?.args).toMatchObject({
      params: { id: "newest" },
    }),
  );
  await waitFor(() => expect(onChanged).toHaveBeenCalled());
});

it("deletes one backup, the unrestorable ones or all of them, each after confirmation", async () => {
  setup();
  fireEvent.click(
    await screen.findByRole("button", {
      name: `删除 ${time(1_791_930_000)} 的备份`,
    }),
  );
  expect(
    screen.getByRole("dialog", { name: "删除这次备份？" }),
  ).toHaveTextContent("已无法恢复");
  confirmIn("删除这次备份？", "删除");
  await waitFor(() => expect(mcpCalls("deleteBackups")).toHaveLength(1));
  expect(mcpCalls("deleteBackups")[0].args).toMatchObject({
    params: { ids: ["stale"] },
  });

  fireEvent.click(screen.getByRole("button", { name: "删除无法恢复的 1 次" }));
  confirmIn("删除无法恢复的 1 次备份？", "删除");
  await waitFor(() => expect(mcpCalls("deleteBackups")).toHaveLength(2));
  expect(mcpCalls("deleteBackups")[1].args).toMatchObject({
    params: { which: "unrestorable" },
  });

  fireEvent.click(screen.getByRole("button", { name: "全部删除" }));
  const all = screen.getByRole("dialog", { name: "删除全部 3 次备份？" });
  expect(all).toHaveTextContent("不能再撤销");
  fireEvent.click(within(all).getByRole("button", { name: "取消" }));
  expect(mcpCalls("deleteBackups")).toHaveLength(2);
  fireEvent.click(screen.getByRole("button", { name: "全部删除" }));
  confirmIn("删除全部 3 次备份？", "全部删除");
  await waitFor(() => expect(mcpCalls("deleteBackups")).toHaveLength(3));
  expect(mcpCalls("deleteBackups")[2].args).toMatchObject({
    params: { which: "all" },
  });
});

it("warns that deleting a backup older ones still need cuts them off too", async () => {
  setup();
  // 「older」要先撤销「newest」才能恢复：删掉 newest，older 就回不去了
  fireEvent.click(
    await screen.findByRole("button", {
      name: `删除 ${time(1_791_950_000)} 的备份`,
    }),
  );
  const dialog = screen.getByRole("dialog", { name: "删除这次备份？" });
  expect(dialog).toHaveTextContent("更早的备份也可能因此无法恢复");
  fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
  // 比「older」更早的只剩已无法恢复的那次，不必再提醒
  fireEvent.click(
    screen.getByRole("button", { name: `删除 ${time(1_791_940_000)} 的备份` }),
  );
  expect(
    screen.getByRole("dialog", { name: "删除这次备份？" }),
  ).not.toHaveTextContent("更早的备份");
});

it("lists files older versions left beside configs and deletes them only on request", async () => {
  setup({
    backups: [],
    legacy: [
      { path: "/home/u/.claude.studio-backup-6e643728", size: 1536 },
      { path: "/home/u/.codex/config.studio-backup-183949f4", size: 512 },
    ],
  });
  const section = await screen.findByRole("region", {
    name: "旧版本留下的备份文件",
  });
  expect(section).toHaveTextContent("2 个 · 2.0 KB");
  expect(section).toHaveTextContent(
    "/home/u/.codex/config.studio-backup-183949f4",
  );
  expect(screen.getByText("暂无备份")).toBeVisible();
  expect(screen.queryByRole("button", { name: "全部删除" })).toBeNull();
  fireEvent.click(
    within(section).getByRole("button", { name: "删除这些文件" }),
  );
  expect(mcpCalls("deleteLegacyBackups")).toHaveLength(0);
  confirmIn("删除 2 个旧备份文件？", "删除");
  await waitFor(() => expect(mcpCalls("deleteLegacyBackups")).toHaveLength(1));
});
