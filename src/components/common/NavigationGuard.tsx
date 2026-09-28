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
  blocker: React.MutableRefObject<Blocker | null>;
} | null>(null);

export function NavigationGuard({ children }: { children: React.ReactNode }) {
  const parent = useContext(Context);
  return parent ? <>{children}</> : <GuardProvider>{children}</GuardProvider>;
}
function GuardProvider({ children }: { children: React.ReactNode }) {
  const blocker = useRef<Blocker | null>(null);
  const [pending, setPending] = useState<(() => void) | null>(null);
  const request = useCallback((action: () => void) => {
    const state = blocker.current?.();
    if (state?.busy) {
      toast.info("正在处理当前目标的操作，请稍候");
      return;
    }
    if (state?.dirty) setPending(() => action);
    else action();
  }, []);
  useEffect(() => {
    const beforeUnload = (e: BeforeUnloadEvent) => {
      const state = blocker.current?.();
      if (state?.dirty || state?.busy || getPending()) {
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
          const state = blocker.current?.();
          if (isMac()) {
            // Keep the app (and its MCP gateway) active in the Dock on Cmd+W.
            // Closing with unsaved work still goes through the discard dialog.
            event.preventDefault();
            request(() => {
              void win.hide().catch((e) => toast.error(String(e)));
            });
          } else if (state?.dirty || state?.busy || getPending()) {
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
  }, [request]);
  const value = useMemo(() => ({ request, blocker }), [request]);
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
    guard.blocker.current = read;
    return () => {
      if (guard.blocker.current === read) guard.blocker.current = null;
    };
  }, [guard]);
}
