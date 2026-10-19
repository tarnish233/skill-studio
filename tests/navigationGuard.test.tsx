import { useState } from "react";
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { toast } from "sonner";
import {
  NavigationGuard,
  useNavigationGuard,
  useUnsavedProject,
} from "@/components/common/NavigationGuard";
import { renderWithProviders } from "./utils/render";

const native = vi.hoisted(() => ({
  handler: null as null | ((e: { preventDefault: () => void }) => void),
  destroy: vi.fn(async () => {}),
  hide: vi.fn(async () => {}),
  off: vi.fn(),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onCloseRequested: async (handler: typeof native.handler) => {
      native.handler = handler;
      return native.off;
    },
    destroy: native.destroy,
    hide: native.hide,
  }),
}));
function Draft() {
  useUnsavedProject(true, false);
  return <p>编辑中</p>;
}
beforeEach(() => {
  vi.stubGlobal("matchMedia", () => ({
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));
});
afterEach(() => {
  vi.unstubAllGlobals();
  delete (window as any).__TAURI_INTERNALS__;
  native.handler = null;
  vi.restoreAllMocks();
  native.destroy.mockClear();
  native.hide.mockClear();
});
it("blocks native close and destroys the window only after discard confirmation", async () => {
  (window as any).__TAURI_INTERNALS__ = {};
  renderWithProviders(
    <NavigationGuard>
      <Draft />
    </NavigationGuard>,
  );
  await waitFor(() => expect(native.handler).not.toBeNull());
  const preventDefault = vi.fn();
  act(() => native.handler!({ preventDefault }));
  expect(preventDefault).toHaveBeenCalled();
  expect(native.destroy).not.toHaveBeenCalled();
  fireEvent.click(await screen.findByRole("button", { name: "继续编辑" }));
  expect(native.destroy).not.toHaveBeenCalled();
  act(() => native.handler!({ preventDefault }));
  fireEvent.click(
    await screen.findByRole("button", { name: "放弃修改并离开" }),
  );
  expect(native.destroy).toHaveBeenCalledTimes(1);
});

it("hides the macOS window on Cmd+W while keeping the app alive", async () => {
  vi.spyOn(window.navigator, "userAgent", "get").mockReturnValue(
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
  );
  (window as any).__TAURI_INTERNALS__ = {};
  renderWithProviders(<NavigationGuard>窗口内容</NavigationGuard>);
  await waitFor(() => expect(native.handler).not.toBeNull());
  const preventDefault = vi.fn();
  act(() => native.handler!({ preventDefault }));
  expect(preventDefault).toHaveBeenCalledOnce();
  await waitFor(() => expect(native.hide).toHaveBeenCalledOnce());
  expect(native.destroy).not.toHaveBeenCalled();
});

it("confirms unsaved macOS work before hiding the window", async () => {
  vi.spyOn(window.navigator, "userAgent", "get").mockReturnValue(
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
  );
  (window as any).__TAURI_INTERNALS__ = {};
  renderWithProviders(
    <NavigationGuard>
      <Draft />
    </NavigationGuard>,
  );
  await waitFor(() => expect(native.handler).not.toBeNull());
  const preventDefault = vi.fn();
  act(() => native.handler!({ preventDefault }));
  expect(preventDefault).toHaveBeenCalledOnce();
  expect(native.hide).not.toHaveBeenCalled();
  fireEvent.click(await screen.findByRole("button", { name: "继续编辑" }));
  expect(native.hide).not.toHaveBeenCalled();
  act(() => native.handler!({ preventDefault }));
  fireEvent.click(screen.getByRole("button", { name: "放弃修改并离开" }));
  await waitFor(() => expect(native.hide).toHaveBeenCalledOnce());
  expect(native.destroy).not.toHaveBeenCalled();
});

/** 页面自己的操作一直在进行；详情可以后打开、再关掉 */
function BusyPage() {
  const request = useNavigationGuard();
  const [detail, setDetail] = useState(false);
  const [left, setLeft] = useState(false);
  useUnsavedProject(false, true);
  return (
    <>
      <button onClick={() => setDetail((v) => !v)}>
        {detail ? "关闭详情" : "打开详情"}
      </button>
      <button onClick={() => request(() => setLeft(true))}>离开</button>
      {left && <p>已离开</p>}
      {detail && <Draft />}
    </>
  );
}
it("页面在进行中时打开有修改的详情：两处登记都算数，详情关掉后页面的仍然拦住离开", () => {
  const info = vi.spyOn(toast, "info");
  renderWithProviders(
    <NavigationGuard>
      <BusyPage />
    </NavigationGuard>,
  );
  fireEvent.click(screen.getByRole("button", { name: "打开详情" }));
  fireEvent.click(screen.getByRole("button", { name: "离开" }));
  // 进行中优先：只提示稍候，不弹放弃修改的确认
  expect(info).toHaveBeenCalledWith("正在处理当前目标的操作，请稍候");
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "关闭详情" }));
  fireEvent.click(screen.getByRole("button", { name: "离开" }));
  expect(info).toHaveBeenCalledTimes(2);
  expect(screen.queryByText("已离开")).toBeNull();
});

function Clean() {
  useUnsavedProject(false, false);
  return null;
}
function TwoEditors() {
  const request = useNavigationGuard();
  const [second, setSecond] = useState(false);
  const [left, setLeft] = useState(false);
  return (
    <>
      <Draft />
      {second && <Clean />}
      <button onClick={() => setSecond(true)}>再打开一个</button>
      <button onClick={() => request(() => setLeft(true))}>离开</button>
      {left && <p>已离开</p>}
    </>
  );
}
it("后登记的没有修改，也不会盖掉先登记的未保存修改", async () => {
  renderWithProviders(
    <NavigationGuard>
      <TwoEditors />
    </NavigationGuard>,
  );
  fireEvent.click(screen.getByRole("button", { name: "再打开一个" }));
  fireEvent.click(screen.getByRole("button", { name: "离开" }));
  expect(
    await screen.findByRole("dialog", { name: "放弃未保存的修改？" }),
  ).toBeInTheDocument();
  expect(screen.queryByText("已离开")).toBeNull();
});
