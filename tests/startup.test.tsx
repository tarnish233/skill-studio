import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "@/App";
import { StartupBoundary } from "@/components/common/StartupBoundary";
import { readInitialView } from "@/lib/initialView";
import { renderWithProviders } from "./utils/render";
import {
  calls,
  handlers,
  claudeAgent,
  defaultSettings,
  makeSkill,
  makeProject,
} from "./mocks/tauri";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function start() {
  return renderWithProviders(
    <StartupBoundary>
      <App />
    </StartupBoundary>,
  );
}

describe("startup readiness", () => {
  it("reveals navigation, counts and skills together without waiting for CLI probes", async () => {
    const settings = deferred<ReturnType<typeof defaultSettings>>();
    const skills = deferred<ReturnType<typeof makeSkill>[]>();
    const probe = deferred<(typeof claudeAgent)[]>();
    handlers.set("get_settings", () => settings.promise);
    handlers.set("scan_skills", () => skills.promise);
    handlers.set("list_agents", ({ skipCliProbe }) =>
      skipCliProbe ? [{ ...claudeAgent, detected: false }] : probe.promise,
    );
    start();
    expect(screen.getByRole("status")).toHaveTextContent("正在准备工作区");
    expect(screen.getByRole("status").children).toHaveLength(0);
    expect(screen.queryByRole("navigation")).toBeNull();
    expect(calls.some((c) => c.command === "scan_skills")).toBe(false);
    await act(async () => settings.resolve(defaultSettings()));
    await waitFor(() =>
      expect(calls.some((c) => c.command === "scan_skills")).toBe(true),
    );
    expect(screen.queryByRole("navigation")).toBeNull();
    await act(async () => skills.resolve([makeSkill({ name: "Ready skill" })]));
    expect(await screen.findByText("Ready skill")).toBeInTheDocument();
    expect(
      screen.getByRole("navigation", { name: "主导航" }),
    ).toBeInTheDocument();
    expect(screen.getByText("已安装 1 个")).toBeInTheDocument();
    expect(calls.filter((c) => c.command === "scan_skills")).toHaveLength(1);
    expect(
      calls.filter((c) => c.command === "list_agents").map((c) => c.args),
    ).toEqual([{ skipCliProbe: true }, { skipCliProbe: false }]);
    await act(async () => probe.resolve([claudeAgent]));
  });

  it("does not briefly mount Skill Hub while restoring MCP with delayed settings", async () => {
    localStorage.setItem("skill-studio-view", "mcp");
    const settings = deferred<ReturnType<typeof defaultSettings>>();
    handlers.set("get_settings", () => settings.promise);
    start();
    expect(screen.queryByRole("heading")).toBeNull();
    expect(localStorage.getItem("skill-studio-view")).toBe("mcp");
    await act(async () =>
      settings.resolve({ ...defaultSettings(), manageMcp: true }),
    );
    expect(
      await screen.findByRole("heading", { name: "MCP Hub" }),
    ).toBeInTheDocument();
    expect(calls.some((c) => c.command === "scan_skills")).toBe(false);
    expect(
      calls.filter(
        (c) =>
          c.command === "mcp_request" &&
          (c.args as { method: string }).method === "list",
      ),
    ).toHaveLength(1);
  });

  it("waits for the restored project's dependencies instead of showing an empty project list", async () => {
    localStorage.setItem("skill-studio-view", "projects");
    const projects = deferred<ReturnType<typeof makeProject>[]>();
    handlers.set("list_projects", () => projects.promise);
    start();
    await waitFor(() =>
      expect(calls.some((c) => c.command === "list_projects")).toBe(true),
    );
    expect(screen.queryByRole("navigation")).toBeNull();
    await act(async () => projects.resolve([makeProject()]));
    expect(await screen.findByText("webapp")).toBeInTheDocument();
    expect(calls.filter((c) => c.command === "list_projects")).toHaveLength(1);
  });

  it("offers retry on initial failure and never hides the app on background refresh", async () => {
    handlers.set("scan_skills", () => {
      throw new Error("scan failed");
    });
    start();
    expect(await screen.findByRole("alert")).toHaveTextContent("scan failed");
    handlers.set("scan_skills", () => [makeSkill({ name: "Retained" })]);
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(await screen.findByText("Retained")).toBeInTheDocument();
    const refresh = deferred<ReturnType<typeof makeSkill>[]>();
    handlers.set("scan_skills", () => refresh.promise);
    fireEvent.click(screen.getByTitle("重新扫描"));
    expect(screen.getByText("Retained")).toBeInTheDocument();
    expect(screen.queryByRole("status", { name: "正在准备工作区" })).toBeNull();
    await act(async () => refresh.resolve([makeSkill({ name: "Updated" })]));
    expect(await screen.findByText("Updated")).toBeInTheDocument();
  });

  it("allows opening recovery UI rather than trapping the user behind a failed scan", async () => {
    handlers.set("scan_skills", () => {
      throw new Error("read-only recovery");
    });
    handlers.set("get_init_error", () => "配置文件损坏，请恢复备份");
    start();
    fireEvent.click(
      await screen.findByRole("button", { name: "继续打开应用" }),
    );
    expect(
      await screen.findByText("配置文件损坏，请恢复备份"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "设置" })).toBeInTheDocument();
  });

  it("resolves disabled MCP and hidden Agents before choosing startup queries", () => {
    localStorage.setItem("skill-studio-view", "mcp");
    expect(readInitialView({ ...defaultSettings(), manageMcp: false })).toBe(
      "hub",
    );
    // 升级前的单 agent 视图现在统一落在共享的“分组”页：哪个 Agent 被关闭管理
    // 由该页内部的 Agent 选择器过滤，不再需要这层路由做牙门校验。
    localStorage.setItem("skill-studio-view", "agent:codex");
    expect(
      readInitialView({ ...defaultSettings(), disabledAgents: ["codex"] }),
    ).toBe("groups");
    localStorage.setItem("skill-studio-view", "settings");
    expect(readInitialView(defaultSettings())).toBe("hub");
  });
});
