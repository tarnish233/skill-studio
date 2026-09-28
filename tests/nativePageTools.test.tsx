import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { PageTools, PageToolsContext } from "@/components/common/PageTools";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useNativePageTools } from "@/hooks/useNativePageTools";
import { calls, handlers } from "./mocks/tauri";

vi.mock("@/lib/platform", () => ({ isMac: () => true }));

let send = (_payload: Record<string, unknown>) => {};
const off = vi.fn();
const created = vi.fn();

function Harness({
  scope = "skills",
  disabled = false,
}: {
  scope?: string;
  disabled?: boolean;
}) {
  const [search, setSearch] = useState<HTMLDivElement | null>(null);
  const [add, setAdd] = useState<HTMLDivElement | null>(null);
  const [query, setQuery] = useState("");
  const [dialog, setDialog] = useState(false);
  const native = useNativePageTools({ search, add }, true, scope);
  return (
    <TooltipProvider>
      <PageToolsContext.Provider value={{ search, add, native }}>
        <div ref={setSearch} />
        <div ref={setAdd} />
        <PageTools
          query={query}
          onQueryChange={setQuery}
          placeholder="搜索 skill…"
          createLabel="添加 skill"
          onCreate={created}
          createDisabledReason={disabled ? "只读" : undefined}
        />
        <output data-testid="query">{query}</output>
        <button onClick={() => setDialog(!dialog)}>切换弹窗</button>
        {dialog && <div role="dialog">确认</div>}
      </PageToolsContext.Provider>
    </TooltipProvider>
  );
}

function packet() {
  return (
    calls.filter((call) => call.command === "update_native_page_tools").at(-1)
      ?.args as
      | {
          packet: {
            owner: string;
            session: string;
            edit: number;
            query: string;
            visible: boolean;
            createDisabledReason: string;
          };
        }
      | undefined
  )?.packet;
}
function emit(kind: string, extra: Record<string, unknown> = {}) {
  const { owner, session } = packet()!;
  act(() => send({ owner, session, kind, value: "", edit: 0, ...extra }));
}

beforeEach(() => {
  // Reproduce an inactive WKWebView: no animation frame ever executes.
  vi.spyOn(window, "requestAnimationFrame").mockReturnValue(1);
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {},
  });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    x: 800,
    y: 8,
    width: 192,
    height: 32,
    top: 8,
    left: 800,
    right: 992,
    bottom: 40,
    toJSON: () => ({}),
  });
  vi.mocked(listen).mockImplementation(async (name, handler) => {
    send = (payload) =>
      handler({ event: String(name), id: 0, payload: payload as never });
    return off;
  });
  handlers.set("init_native_page_tools", () => true);
});
afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  vi.restoreAllMocks();
});

it("synchronizes native edits without accepting duplicate or older input events", async () => {
  render(<Harness />);
  await waitFor(() => expect(packet()?.visible).toBe(true));
  expect(screen.queryByPlaceholderText("搜索 skill…")).not.toBeInTheDocument();
  emit("search", { value: "工具", edit: 1 });
  await waitFor(() =>
    expect(packet()).toMatchObject({ query: "工具", edit: 1 }),
  );
  emit("search", { value: "工具箱", edit: 2 });
  await waitFor(() =>
    expect(packet()).toMatchObject({ query: "工具箱", edit: 2 }),
  );
  emit("search", { value: "旧内容", edit: 1 });
  expect(screen.getByTestId("query")).toHaveTextContent("工具箱");
  emit("search", { value: "", edit: 3 });
  await waitFor(() => expect(packet()).toMatchObject({ query: "", edit: 3 }));
  emit("create");
  expect(created).toHaveBeenCalledTimes(1);
});

it("blocks native controls behind dialogs and discards events from previous pages", async () => {
  const { rerender } = render(<Harness />);
  await waitFor(() => expect(packet()?.visible).toBe(true));
  const old = packet()!;
  fireEvent.click(screen.getByText("切换弹窗"));
  await waitFor(() => expect(packet()?.visible).toBe(false));
  emit("create");
  expect(created).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("切换弹窗"));
  await waitFor(() => expect(packet()?.visible).toBe(true));
  rerender(<Harness scope="remote:mcp" disabled />);
  await waitFor(() => expect(packet()?.createDisabledReason).toBe("只读"));
  expect(packet()?.owner).not.toBe(old.owner);
  act(() =>
    send({
      owner: old.owner,
      session: old.session,
      kind: "search",
      value: "过期",
      edit: 9,
    }),
  );
  expect(screen.getByTestId("query")).toBeEmptyDOMElement();
  emit("create");
  expect(created).not.toHaveBeenCalled();
});

it("restores usable web controls when native setup fails", async () => {
  handlers.set("init_native_page_tools", () => {
    throw new Error("unavailable");
  });
  render(<Harness />);
  const input = await screen.findByPlaceholderText("搜索 skill…");
  fireEvent.change(input, { target: { value: "fallback" } });
  expect(screen.getByTestId("query")).toHaveTextContent("fallback");
  fireEvent.click(screen.getByRole("button", { name: "添加 skill" }));
  expect(created).toHaveBeenCalledTimes(1);
  await waitFor(() =>
    expect(calls.some((c) => c.command === "destroy_native_page_tools")).toBe(
      true,
    ),
  );
});

it("disposes a native setup that completes after its React owner unmounts", async () => {
  let finish!: (value: boolean) => void;
  handlers.set(
    "init_native_page_tools",
    () =>
      new Promise<boolean>((resolve) => {
        finish = resolve;
      }),
  );
  const { unmount } = render(<Harness />);
  await waitFor(() => expect(finish).toBeTypeOf("function"));
  const init = calls.find((c) => c.command === "init_native_page_tools")!;
  unmount();
  await act(async () => {
    finish(true);
  });
  await waitFor(() =>
    expect(calls).toContainEqual({
      command: "destroy_native_page_tools",
      args: init.args,
    }),
  );
  expect(off).toHaveBeenCalledTimes(1);
});
