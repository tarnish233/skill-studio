import { useId, type ReactNode } from "react";
import { LayoutGroup, motion, useReducedMotion } from "framer-motion";
import { cn } from "@/lib/utils";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

export interface NavItem<T extends string> {
  id: T;
  label: string;
  icon: ReactNode;
  /** Keep unavailable destinations in place and explain why they cannot be opened. */
  disabledReason?: string;
}

interface NavSwitcherProps<T extends string> {
  items: NavItem<T>[];
  active: T;
  onSelect: (id: T) => void;
}

/** Compact navigation in the persistent sidebar. */
export function NavSwitcher<T extends string>({
  items,
  active,
  onSelect,
}: NavSwitcherProps<T>) {
  const layoutId = useId();
  const reduceMotion = useReducedMotion();
  return (
    <LayoutGroup id={layoutId}>
      <div className="flex w-full flex-col items-center gap-1">
        {items.map(({ id, label, icon, disabledReason }) => {
          const isActive = active === id;
          return (
            <Tooltip key={id}>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  data-tauri-no-drag
                  onClick={() => {
                    if (!disabledReason) onSelect(id);
                  }}
                  aria-label={label}
                  aria-disabled={disabledReason ? true : undefined}
                  aria-current={isActive ? "page" : undefined}
                  className={cn(
                    "relative isolate inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-xl transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    disabledReason
                      ? "cursor-default text-muted-foreground opacity-40"
                      : isActive
                        ? "text-foreground"
                        : "text-muted-foreground hover:bg-foreground/[0.05] hover:text-foreground",
                  )}
                >
                  {isActive && (
                    <motion.span
                      aria-hidden="true"
                      layoutId="nav-selection"
                      initial={false}
                      transition={{
                        duration: reduceMotion ? 0 : 0.22,
                        ease: [0.22, 1, 0.36, 1],
                      }}
                      className="pointer-events-none absolute inset-0 -z-10 rounded-xl bg-foreground/[0.07]"
                    />
                  )}
                  <span
                    aria-hidden="true"
                    className="flex h-5 w-5 items-center justify-center"
                  >
                    {icon}
                  </span>
                  <span className="sr-only">{label}</span>
                </button>
              </TooltipTrigger>
              <TooltipContent side="right">
                {disabledReason ?? label}
              </TooltipContent>
            </Tooltip>
          );
        })}
      </div>
    </LayoutGroup>
  );
}
