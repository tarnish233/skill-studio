import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isMac } from "@/lib/platform";

export interface PageToolsSpec {
  owner: string;
  query: string;
  placeholder: string;
  createLabel?: string;
  createDisabledReason?: string;
  onQueryChange: (query: string) => void;
  onCreate?: () => void;
}

export interface NativePageToolsBridge {
  ready: boolean;
  scope: string;
  register: (tools: PageToolsSpec) => () => void;
}

type Hosts = { search: HTMLElement | null; add: HTMLElement | null };
type ToolEvent = {
  session: string;
  owner: string;
  kind: "search" | "create";
  value: string;
  edit: number;
};

// Serialize setup, updates and disposal, including React StrictMode/remounts.
let commands: Promise<unknown> = Promise.resolve();
let sessionNumber = 0;
function nativeCommand<T>(command: string, args: Record<string, unknown>) {
  const next = commands.then(() => invoke<T>(command, args));
  commands = next.catch(() => {});
  return next;
}

function overlayOpen() {
  return !!document.querySelector(
    '.app-shell[inert], [role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"]',
  );
}

export function useNativePageTools(
  hosts: Hosts,
  enabled: boolean,
  scope: string,
): NativePageToolsBridge {
  const [ready, setReady] = useState(false);
  const current = useRef<PageToolsSpec | null>(null);
  const active = useRef(enabled);
  active.current = enabled;
  const edit = useRef(0);
  const owner = useRef("");
  const schedule = useRef(() => {});
  const register = useCallback((tools: PageToolsSpec) => {
    if (owner.current !== tools.owner) {
      owner.current = tools.owner;
      edit.current = 0;
    }
    current.current = tools;
    schedule.current();
    return () => {
      if (current.current === tools) {
        current.current = null;
        schedule.current();
      }
    };
  }, []);

  useEffect(() => {
    schedule.current();
  }, [enabled]);

  useEffect(() => {
    setReady(false);
    const { search, add } = hosts;
    if (!isMac() || !("__TAURI_INTERNALS__" in window) || !search || !add)
      return;
    const session = `${Date.now()}-${++sessionNumber}`;
    let disposed = false;
    let supported = false;
    let revision = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let unlisten: UnlistenFn | undefined;
    let inFlight = false;
    let pending = false;
    const rect = (host: HTMLElement) => {
      const { x, y, width, height } = host.getBoundingClientRect();
      return { x, y, width, height };
    };
    const fail = () => {
      supported = false;
      if (!disposed) setReady(false);
      void nativeCommand("destroy_native_page_tools", { session }).catch(
        () => {},
      );
    };
    const flush = () => {
      if (disposed || !supported) return;
      if (inFlight) {
        pending = true;
        return;
      }
      const tools = current.current;
      const searchRect = rect(search);
      const packet = {
        session,
        revision: ++revision,
        owner: tools?.owner ?? "",
        query: tools?.query ?? "",
        edit: edit.current,
        placeholder: tools?.placeholder ?? "",
        createLabel: tools?.createLabel ?? "",
        createDisabledReason: tools?.createDisabledReason ?? "",
        showAdd: !!(tools?.onCreate || tools?.createDisabledReason),
        visible:
          !!tools &&
          active.current &&
          !overlayOpen() &&
          searchRect.width > 0 &&
          searchRect.height > 0,
        viewportWidth: window.innerWidth,
        search: searchRect,
        add: rect(add),
      };
      inFlight = true;
      void nativeCommand("update_native_page_tools", { packet })
        .catch(fail)
        .finally(() => {
          inFlight = false;
          if (pending) {
            pending = false;
            requestFlush();
          }
        });
    };
    const requestFlush = () => {
      if (disposed || timer !== undefined) return;
      // Inactive transparent WKWebViews can pause animation frames. Native
      // controls must still be mounted and synchronized before activation.
      timer = setTimeout(() => {
        timer = undefined;
        flush();
      }, 0);
    };
    schedule.current = requestFlush;
    const resize = new ResizeObserver(requestFlush);
    resize.observe(search);
    resize.observe(add);
    const overlays = new MutationObserver(requestFlush);
    overlays.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["inert", "data-state"],
    });
    window.addEventListener("resize", requestFlush);
    void (async () => {
      try {
        const off = await listen<ToolEvent>(
          "native-page-tools",
          ({ payload }) => {
            const tools = current.current;
            if (
              disposed ||
              !supported ||
              !active.current ||
              overlayOpen() ||
              !tools ||
              payload.session !== session ||
              payload.owner !== tools.owner
            )
              return;
            if (payload.kind === "search" && payload.edit > edit.current) {
              edit.current = payload.edit;
              flushSync(() => tools.onQueryChange(payload.value));
              requestFlush();
            } else if (
              payload.kind === "create" &&
              !tools.createDisabledReason
            ) {
              flushSync(() => tools.onCreate?.());
            }
          },
        );
        if (disposed) {
          off();
          return;
        }
        unlisten = off;
        supported = await nativeCommand<boolean>("init_native_page_tools", {
          session,
        });
        if (disposed) return;
        setReady(supported);
        requestFlush();
      } catch {
        fail();
      }
    })();
    return () => {
      disposed = true;
      if (timer !== undefined) clearTimeout(timer);
      resize.disconnect();
      overlays.disconnect();
      unlisten?.();
      window.removeEventListener("resize", requestFlush);
      if (schedule.current === requestFlush) schedule.current = () => {};
      void nativeCommand("destroy_native_page_tools", { session }).catch(
        () => {},
      );
    };
  }, [hosts.search, hosts.add]);

  return useMemo(() => ({ ready, scope, register }), [ready, scope, register]);
}
