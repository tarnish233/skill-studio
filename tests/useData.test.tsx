import { queryKeys } from "@/lib/queryKeys";
import { act, renderHook, waitFor } from "@testing-library/react";
import {
  QueryClient,
  QueryClientProvider,
  focusManager,
} from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  useSkills,
  useSkillsAutoRefresh,
  useAgents,
  useSettings,
  useUpdateSettings,
} from "@/hooks/useData";
import { calls, handlers, makeSkill, defaultSettings } from "./mocks/tauri";

/**
 * scan_skills 在后台线程遍历全库并占用配置锁，避免导航触发无谓的重复扫描。
 * 这里验证的是"什么时候不该扫"和"什么时候必须扫"两侧。
 */
function wrapper(providedClient?: QueryClient) {
  // 刻意照 src/lib/query/queryClient.ts 的全局默认值，验证 useSkills 自己的
  // staleTime 确实压得住这两条默认行为
  const client =
    providedClient ??
    new QueryClient({
      defaultOptions: {
        queries: { retry: false, staleTime: 0, refetchOnWindowFocus: true },
      },
    });
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
}

function scans() {
  return calls.filter((c) => c.command === "scan_skills");
}

afterEach(() => {
  focusManager.setFocused(undefined);
  vi.mocked(listen).mockImplementation(async () => () => {});
});

describe("useSkills 的扫描时机", () => {
  it("导航离开再回来不重扫全库", async () => {
    handlers.set("scan_skills", () => [makeSkill()]);
    const Wrapper = wrapper();
    const first = renderHook(() => useSkills(), { wrapper: Wrapper });
    await waitFor(() => expect(first.result.current.data).toHaveLength(1));
    first.unmount();

    const second = renderHook(() => useSkills(), { wrapper: Wrapper });
    await waitFor(() => expect(second.result.current.data).toHaveLength(1));
    expect(scans()).toHaveLength(1);
  });

  it("窗口重新聚焦不重扫全库", async () => {
    handlers.set("scan_skills", () => [makeSkill()]);
    const { result } = renderHook(() => useSkills(), { wrapper: wrapper() });
    await waitFor(() => expect(result.current.data).toHaveLength(1));

    await act(async () => {
      focusManager.setFocused(false);
      focusManager.setFocused(true);
      await new Promise((r) => setTimeout(r, 20));
    });
    expect(scans()).toHaveLength(1);
  });

  it("文件监听仍然是权威的失效来源：skills-changed 照旧触发重扫", async () => {
    handlers.set("scan_skills", () => [makeSkill()]);
    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    vi.mocked(listen).mockImplementation(async (name, cb) => {
      listeners.set(name, cb as (e: { payload: unknown }) => void);
      return () => {};
    });

    const { result } = renderHook(
      () => {
        useSkillsAutoRefresh();
        return useSkills();
      },
      { wrapper: wrapper() },
    );
    await waitFor(() => expect(result.current.data).toHaveLength(1));
    await waitFor(() => expect(listeners.has("skills-changed")).toBe(true));

    await act(async () => {
      listeners.get("skills-changed")!({ payload: null });
      await new Promise((r) => setTimeout(r, 20));
    });
    expect(scans()).toHaveLength(2);
  });
});

describe("Studio 之外的改动", () => {
  function listenAll() {
    const listeners = new Map<string, (e: { payload: unknown }) => void>();
    vi.mocked(listen).mockImplementation(async (name, cb) => {
      listeners.set(name, cb as (e: { payload: unknown }) => void);
      return () => {};
    });
    return listeners;
  }
  function client() {
    const qc = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    for (const key of [
      queryKeys.groups,
      queryKeys.projects,
      queryKeys.settings,
      queryKeys.config,
      queryKeys.skills,
      queryKeys.agents,
      queryKeys.agentProbe,
      ["mcp", "local"],
    ])
      qc.setQueryData(key, []);
    return qc;
  }
  const stale = (qc: QueryClient, key: readonly unknown[]) =>
    qc.getQueryState(key)?.isInvalidated ?? false;

  it("别的进程改了配置：分组、项目、设置、skill 和 MCP 状态都重新读取，不重新探测 Agent 命令行", async () => {
    const listeners = listenAll();
    const qc = client();
    renderHook(() => useSkillsAutoRefresh(), { wrapper: wrapper(qc) });
    await waitFor(() => expect(listeners.has("config-changed")).toBe(true));
    act(() => listeners.get("config-changed")!({ payload: null }));
    for (const key of [
      queryKeys.groups,
      queryKeys.projects,
      queryKeys.settings,
      queryKeys.config,
      queryKeys.skills,
      queryKeys.agents,
      ["mcp", "local"],
    ])
      expect(stale(qc, key), JSON.stringify(key)).toBe(true);
    expect(stale(qc, queryKeys.agentProbe)).toBe(false);
  });

  it("项目 skill 目录被外部修改时刷新项目卡片和打开的本地 skill 列表", async () => {
    const listeners = listenAll();
    const qc = client();
    const localSkills = [...queryKeys.projects, "p1", "localSkills"];
    qc.setQueryData(localSkills, []);
    renderHook(() => useSkillsAutoRefresh(), { wrapper: wrapper(qc) });
    await waitFor(() => expect(listeners.has("skills-changed")).toBe(true));

    act(() => listeners.get("skills-changed")!({ payload: null }));
    expect(stale(qc, queryKeys.skills)).toBe(true);
    expect(stale(qc, queryKeys.projects)).toBe(true);
    expect(stale(qc, localSkills)).toBe(true);
    expect(stale(qc, queryKeys.groups)).toBe(false);
  });

  it("MCP 托管状态变了：只重新读取 MCP 状态，不等 30 秒的轮询", async () => {
    const listeners = listenAll();
    const qc = client();
    renderHook(() => useSkillsAutoRefresh(), { wrapper: wrapper(qc) });
    await waitFor(() => expect(listeners.has("mcp-changed")).toBe(true));
    act(() => listeners.get("mcp-changed")!({ payload: null }));
    expect(stale(qc, ["mcp", "local"])).toBe(true);
    expect(stale(qc, queryKeys.groups)).toBe(false);
    expect(stale(qc, queryKeys.skills)).toBe(false);
  });
});

it("defers data refresh after an Agent visibility change but refreshes on next navigation", async () => {
  let settings = defaultSettings();
  handlers.set("get_settings", () => settings);
  handlers.set(
    "update_settings",
    ({ patch }) => (settings = { ...settings, ...patch }),
  );
  handlers.set("scan_skills", () => [makeSkill()]);
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  client.setQueryData(queryKeys.groups, []);
  client.setQueryData(queryKeys.config, { settings });
  const Wrapper = wrapper(client);
  const first = renderHook(
    () => ({
      agents: useAgents(),
      settings: useSettings(),
      skills: useSkills(),
      update: useUpdateSettings(),
    }),
    { wrapper: Wrapper },
  );
  await waitFor(() => expect(first.result.current.skills.data).toHaveLength(1));
  await act(async () => {
    await first.result.current.update.mutateAsync({
      disabledAgents: ["opencode"],
    });
  });
  expect(
    calls.filter((c) => c.command === "list_agents").map((c) => c.args),
  ).toEqual([{ skipCliProbe: true }, { skipCliProbe: false }]);
  expect(scans()).toHaveLength(1);
  expect(client.getQueryState(queryKeys.config)?.isInvalidated).toBe(true);
  expect(client.getQueryState(queryKeys.groups)?.isInvalidated).toBe(true);
  first.unmount();
  const second = renderHook(() => useSkills(), { wrapper: Wrapper });
  await waitFor(() => expect(second.result.current.isFetching).toBe(false));
  expect(scans()).toHaveLength(2);
});

it("still refreshes Agent discovery and Skill data when config directories change", async () => {
  handlers.set("update_settings", ({ patch }) => ({
    ...defaultSettings(),
    ...patch,
  }));
  handlers.set("scan_skills", () => [makeSkill()]);
  const current = renderHook(
    () => ({
      agents: useAgents(),
      skills: useSkills(),
      update: useUpdateSettings(),
    }),
    { wrapper: wrapper() },
  );
  await waitFor(() =>
    expect(current.result.current.skills.data).toHaveLength(1),
  );
  await act(async () => {
    await current.result.current.update.mutateAsync({
      agentDirOverrides: { opencode: "/new/config" },
    });
  });
  await waitFor(() => expect(scans()).toHaveLength(2));
  for (const skipCliProbe of [true, false]) {
    expect(
      calls.filter(
        (c) =>
          c.command === "list_agents" &&
          (c.args as { skipCliProbe: boolean }).skipCliProbe === skipCliProbe,
      ),
    ).toHaveLength(2);
  }
});
