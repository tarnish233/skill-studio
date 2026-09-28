import { createContext, useContext, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type Theme = "light" | "dark" | "system";

const STORAGE_KEY = "skill-studio-theme";

type ThemeProviderState = {
  theme: Theme;
  setTheme: (theme: Theme) => void;
};

const ThemeProviderContext = createContext<ThemeProviderState>({
  theme: "system",
  setTheme: () => null,
});

function readStoredTheme(): Theme {
  try {
    const stored = localStorage.getItem(STORAGE_KEY) as Theme | null;
    if (stored === "light" || stored === "dark" || stored === "system") {
      return stored;
    }
  } catch {
    // localStorage 不可用时静默回落到 system
  }
  return "system";
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(readStoredTheme);

  useEffect(() => {
    let cancelled = false;
    void invoke<string>("get_window_material")
      .then((material) => {
        if (
          !cancelled &&
          (material === "liquid-glass" || material === "vibrancy")
        ) {
          document.documentElement.dataset.windowMaterial = material;
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      delete document.documentElement.dataset.windowMaterial;
    };
  }, []);

  // 把 theme 落到 <html> 的 class 上。light 也显式加类，方便按 .light 选择器覆写。
  useEffect(() => {
    const root = document.documentElement;
    const apply = () => {
      const dark =
        theme === "dark" ||
        (theme === "system" &&
          window.matchMedia("(prefers-color-scheme: dark)").matches);
      root.classList.remove("light", "dark");
      root.classList.add(dark ? "dark" : "light");
    };
    apply();

    // 同步原生标题栏配色；非 Tauri 环境（vitest / 浏览器预览）静默忽略
    void invoke("set_window_theme", { theme }).catch(() => {});

    if (theme !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);

  const setTheme = (next: Theme) => {
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // 忽略写入失败，内存态仍然生效
    }
    setThemeState(next);
  };

  return (
    <ThemeProviderContext.Provider value={{ theme, setTheme }}>
      {children}
    </ThemeProviderContext.Provider>
  );
}

export function useTheme() {
  return useContext(ThemeProviderContext);
}
