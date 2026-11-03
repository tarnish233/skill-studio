import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { PropsWithChildren } from "react";
import { expect, it } from "vitest";
import { useMcpGatewayCheck } from "@/hooks/useMcpGatewayCheck";
import type { ManagedMcp } from "@/lib/api/mcpManagement";
import { calls, handlers } from "./mocks/tauri";

it("coalesces rapid edits, blocks stale success and ignores binding-only changes", async () => {
  handlers.set("mcp_request", () => ({ issue: null }));
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const wrapper = ({ children }: PropsWithChildren) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const entry: ManagedMcp = {
    id: "test",
    name: "Test",
    mode: "gateway",
    definition: { type: "http", url: "https://example.com/first" },
    oauth: false,
    clientId: null,
    scopes: [],
    bindings: [],
  };
  const checks = () =>
    calls.filter(
      (call) =>
        call.command === "mcp_request" &&
        (call.args as { method: string }).method === "gatewayCheck",
    );
  const { result, rerender, unmount } = renderHook(
    ({ entry }) => useMcpGatewayCheck(entry, true),
    { initialProps: { entry }, wrapper },
  );
  await waitFor(() => expect(result.current.isPending).toBe(false));
  expect(checks()).toHaveLength(1);
  act(() => {
    for (let index = 0; index < 12; index++)
      rerender({
        entry: {
          ...entry,
          definition: { type: "http", url: `https://example.com/${index}` },
        },
      });
  });
  expect(result.current.isPending).toBe(true);
  expect(checks()).toHaveLength(1);
  await waitFor(() => expect(result.current.isPending).toBe(false));
  expect(checks()).toHaveLength(2);
  expect(checks()[1].args).toMatchObject({
    params: { entry: { definition: { url: "https://example.com/11" } } },
  });
  rerender({
    entry: {
      ...entry,
      definition: { type: "http", url: "https://example.com/11" },
      bindings: [
        {
          id: "new-binding",
          agent: "codex",
          path: "/config.toml",
          project: null,
          key: "test",
          original: null,
          installed: {},
        },
      ],
    },
  });
  expect(result.current.isPending).toBe(false);
  expect(checks()).toHaveLength(2);
  unmount();
  client.clear();
});
