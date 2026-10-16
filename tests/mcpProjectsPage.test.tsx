import { fireEvent, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { McpProjectsPage } from "@/pages/McpProjectsPage";
import { renderWithProviders } from "./utils/render";
import { handlers, makeProject } from "./mocks/tauri";
import { setTarget } from "@/lib/api/transport";

/** 项目列表页（MCP）：按名字生成图标，整卡可点进详情 */
it("MCP 项目卡片按名字生成图标，点卡片进详情", async () => {
  setTarget({ id: "local", name: "本机", connected: true });
  handlers.set("list_projects", () => [
    makeProject({ id: "p1", name: "webapp", skillIds: [], groupIds: [] }),
    makeProject({ id: "p2", name: "note", skillIds: [], groupIds: [] }),
  ]);
  renderWithProviders(<McpProjectsPage />);
  await screen.findByText("webapp");
  // 不再共用同一个文件夹图标：首字来自项目名
  expect(screen.getByText("W")).toBeInTheDocument();
  expect(screen.getByText("N")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "查看 webapp 的 MCP" }));
  expect(await screen.findByText("项目 MCP")).toBeInTheDocument();
});
