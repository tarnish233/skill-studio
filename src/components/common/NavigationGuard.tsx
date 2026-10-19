import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { toast } from "sonner";
import { ConfirmDialog } from "./ConfirmDialog";
import { getPending } from "@/lib/api/transport";
import { isMac } from "@/lib/platform";

type Blocker = () => { dirty: boolean; busy: boolean };
const Context = createContext<{
  request: (action: () => void) => void;
  /** 页面和它展开的详情可以同时登记（页面的操作进行中、详情里有未保存修改），合起来判断 */
  blockers: React.MutableRefObject<Set<Blocker>>;
} | null>(null);

export function NavigationGuard({ children }: { children: React.ReactNode }) {
  const parent = useContext(Context);
  return parent ? <>{children}</> : <GuardProvider>{children}</GuardProvider>;
}
function GuardProvider({ children }: { children: React.ReactNode }) {
  const blockers = useRef(new Set<Blocker>());
  const [pending, setPending] = useState<(() => void) | null>(null);
  /** 任一处在进行中就算进行中，任一处有未保存修改就算有修改 */
  const current = useCallback(() => {
    let dirty = false;
    let busy = false;
    for (const read of blockers.current) {
      const state = read();
      dirty ||= state.dirty;
      busy ||= state.busy;
    }
    return { dirty, busy };
  }, []);
  const request = useCallback(
    (action: () => void) => {
      const state = current();
      if (state.busy) {
        toast.info("正在处理当前目标的操作，请稍候");
        return;
      }
      if (state.dirty) setPending(() => action);
      else action();
    },
    [current],
  );
  useEffect(() => {
    const beforeUnload = (e: BeforeUnloadEvent) => {
      const state = current();
      if (state.dirty || state.busy || getPending()) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", beforeUnload);
    let disposed = false;
    let unlisten: (() => void) | undefined;
    if ("__TAURI_INTERNALS__" in window) {
      const win = getCurrentWindow();
      void win
        .onCloseRequested((event) => {
          const state = current();
          if (isMac()) {
            // Keep the app (and its MCP gateway) active in the Dock on Cmd+W.
            // Closing with unsaved work still goes through the discard dialog.
            event.preventDefault();
            request(() => {
              void win.hide().catch((e) => toast.error(String(e)));
            });
          } else if (state.dirty || state.busy || getPending()) {
            event.preventDefault();
            request(() => {
              void win.destroy().catch((e) => toast.error(String(e)));
            });
          }
        })
        .then((off) => {
          if (disposed) off();
          else unlisten = off;
        })
        .catch((e) => toast.error(String(e)));
    }
    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener("beforeunload", beforeUnload);
    };
  }, [request, current]);
  const value = useMemo(() => ({ request, blockers }), [request]);
  return (
    <Context.Provider value={value}>
      {children}
      <ConfirmDialog
        open={pending !== null}
        onOpenChange={(open) => {
          if (!open) setPending(null);
        }}
        title="放弃未保存的修改？"
        description="修改尚未保存。离开后将丢弃这些修改。"
        confirmText="放弃修改并离开"
        cancelText="继续编辑"
        onConfirm={() => {
          const action = pending;
          setPending(null);
          action?.();
        }}
      />
    </Context.Provider>
  );
}
export function useNavigationGuard() {
  const guard = useContext(Context);
  if (!guard) throw new Error("NavigationGuard missing");
  return guard.request;
}
export function useUnsavedProject(dirty: boolean, busy: boolean) {
  const guard = useContext(Context);
  const latest = useRef({ dirty, busy });
  latest.current = { dirty, busy };
  useLayoutEffect(() => {
    if (!guard) return;
    const read = () => latest.current;
    const blockers = guard.blockers.current;
    blockers.add(read);
    return () => {
      blockers.delete(read);
    };
  }, [guard]);
}
