import {
  createContext,
  useContext,
  useId,
  useLayoutEffect,
  useRef,
} from "react";
import { createPortal } from "react-dom";
import { Plus, Search, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { NativePageToolsBridge } from "@/hooks/useNativePageTools";
import { cn } from "@/lib/utils";

export const PageToolsContext = createContext<{
  search: HTMLElement | null;
  add: HTMLElement | null;
  native?: NativePageToolsBridge;
} | null>(null);

export function PageTools({
  query,
  onQueryChange,
  placeholder,
  createLabel,
  onCreate,
  createDisabledReason,
}: {
  query: string;
  onQueryChange: (value: string) => void;
  placeholder: string;
  createLabel?: string;
  onCreate?: () => void;
  createDisabledReason?: string;
}) {
  const hosts = useContext(PageToolsContext);
  const id = useId();
  const owner = `${hosts?.native?.scope ?? ""}:${id}`;
  const callbacks = useRef({ onQueryChange, onCreate });
  callbacks.current = { onQueryChange, onCreate };
  const register = hosts?.native?.register;
  const hasCreate = !!onCreate;
  useLayoutEffect(
    () =>
      register?.({
        owner,
        query,
        placeholder,
        createLabel,
        createDisabledReason,
        onQueryChange: (value) => callbacks.current.onQueryChange(value),
        onCreate: hasCreate ? () => callbacks.current.onCreate?.() : undefined,
      }),
    [
      register,
      owner,
      query,
      placeholder,
      createLabel,
      createDisabledReason,
      hasCreate,
    ],
  );
  if (hosts?.native?.ready) return null;
  const search = (
    <div className={`relative min-w-0 ${hosts ? "w-full" : "w-48 max-w-full"}`}>
      <Search className="pointer-events-none absolute left-2 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
      <Input
        aria-label={placeholder}
        placeholder={placeholder}
        value={query}
        onChange={(e) => onQueryChange(e.target.value)}
        className="h-8 pl-8 pr-8 text-[13px]"
      />
      {query && (
        <button
          type="button"
          aria-label="清除搜索"
          onClick={() => onQueryChange("")}
          className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground"
        >
          <X className="h-4 w-4" />
        </button>
      )}
    </div>
  );
  const addButton = (onCreate || createDisabledReason) && (
    <Button
      size="icon"
      variant={createDisabledReason ? "secondary" : "default"}
      title={createDisabledReason ? undefined : createLabel}
      aria-label={createLabel}
      aria-disabled={createDisabledReason ? true : undefined}
      onClick={() => {
        if (!createDisabledReason) onCreate?.();
      }}
      className={cn(
        "h-8 w-8 shrink-0 rounded-full",
        createDisabledReason
          ? "cursor-default bg-muted text-muted-foreground hover:bg-muted hover:text-muted-foreground"
          : "bg-blue-500 text-white shadow-md shadow-blue-500/20 hover:bg-blue-600",
      )}
    >
      <Plus className="h-4 w-4" />
    </Button>
  );
  const add = createDisabledReason ? (
    <Tooltip>
      <TooltipTrigger asChild>{addButton}</TooltipTrigger>
      <TooltipContent side="bottom">{createDisabledReason}</TooltipContent>
    </Tooltip>
  ) : (
    addButton
  );
  if (!hosts)
    return (
      <div className="flex justify-end gap-3 py-3">
        {search}
        {add}
      </div>
    );
  return (
    <>
      {hosts.search && createPortal(search, hosts.search)}
      {hosts.add && createPortal(add, hosts.add)}
    </>
  );
}
