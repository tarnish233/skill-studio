import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { Toaster } from "@/components/ui/sonner";
import { ThemeProvider, useTheme } from "@/components/theme-provider";

/** 提示框所在列表上的主题：sonner 按它切换整套配色 */
async function toasterTheme() {
  await waitFor(() =>
    expect(document.querySelector("[data-sonner-toaster]")).not.toBeNull(),
  );
  return document
    .querySelector("[data-sonner-toaster]")!
    .getAttribute("data-sonner-theme");
}

function ThemeSwitch() {
  const { setTheme } = useTheme();
  return (
    <button type="button" onClick={() => setTheme("light")}>
      浅色
    </button>
  );
}

function renderToaster() {
  render(
    <ThemeProvider>
      <ThemeSwitch />
      <Toaster />
    </ThemeProvider>,
  );
  act(() => {
    toast.success("已保存");
  });
}

afterEach(() => {
  act(() => toast.dismiss());
  localStorage.removeItem("skill-studio-theme");
  vi.restoreAllMocks();
});

it("深色主题下提示框也用深色，切换主题时跟着变", async () => {
  localStorage.setItem("skill-studio-theme", "dark");
  renderToaster();
  expect(await toasterTheme()).toBe("dark");
  fireEvent.click(screen.getByRole("button", { name: "浅色" }));
  await waitFor(async () => expect(await toasterTheme()).toBe("light"));
});

it("跟随系统时，系统是深色提示框就是深色", async () => {
  localStorage.setItem("skill-studio-theme", "system");
  vi.spyOn(window, "matchMedia").mockImplementation(
    (query: string) =>
      ({
        matches: query === "(prefers-color-scheme: dark)",
        media: query,
        onchange: null,
        addListener: vi.fn(),
        removeListener: vi.fn(),
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(),
      }) as unknown as MediaQueryList,
  );
  renderToaster();
  expect(await toasterTheme()).toBe("dark");
});
