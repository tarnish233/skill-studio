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
  badge?: string;
  /** Keep unavailable destinations in place and explain why they cannot be opened. */
  disabledReason?: string;
}

export interface NavSection<T extends string> {
  label?: string;
  items: NavItem<T>[];
}

interface NavSwitcherProps<T extends string> {
  sections: NavSection<T>[];
  active: T;
  selected?: T;
  onSelect: (id: T) => void;
}

/** Compact, grouped navigation in the persistent sidebar. */
export function NavSwitcher<T extends string>({
  sections,
  active,
  selected,
  onSelect,
}: NavSwitcherProps<T>) {
  const layoutId = useId();
  const reduceMotion = useReducedMotion();
  return (
    <LayoutGroup id={layoutId}>
      <div className="flex w-full flex-col items-center gap-2">
        {sections
          .filter((section) => section.items.length > 0)
          .map((section, i) => (
            <div
              key={section.label ?? i}
              role="group"
              aria-label={section.label ?? "Hub"}
              className="flex w-full flex-col items-center gap-1 border-t border-border/70 pt-2 first:border-t-0 first:pt-0"
            >
              {section.items.map(
                ({ id, label, icon, badge, disabledReason }) => {
                  const isActive = active === id || selected === id;
                  const description = badge ? `${label} · ${badge}` : label;
                  return (
                    <Tooltip key={id}>
                      <TooltipTrigger asChild>
                        <button
                          type="button"
                          data-tauri-no-drag
                          onClick={() => {
                            if (!disabledReason) onSelect(id);
                          }}
                          aria-label={description}
                          aria-disabled={disabledReason ? true : undefined}
                          aria-current={active === id ? "page" : undefined}
                          aria-pressed={selected === id ? true : undefined}
                          className={cn(
                            "relative isolate inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-xl transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                            disabledReason
                              ? "cursor-default text-muted-foreground opacity-40"
                              : selected === id
                                ? "text-foreground"
                                : isActive
                                  ? "bg-foreground/[0.07] text-foreground"
                                  : "text-muted-foreground hover:bg-foreground/[0.05] hover:text-foreground",
                          )}
                        >
                          {selected === id && (
                            <motion.span
                              aria-hidden="true"
                              layoutId="hub-selection"
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
                          <span className="sr-only">{description}</span>
                          {badge && (
                            <span
                              aria-hidden="true"
                              className="absolute right-1.5 top-1.5 h-1.5 w-1.5 rounded-full bg-amber-500"
                            />
                          )}
                        </button>
                      </TooltipTrigger>
                      <TooltipContent side="right">
                        {disabledReason ?? description}
                      </TooltipContent>
                    </Tooltip>
                  );
                },
              )}
            </div>
          ))}
      </div>
    </LayoutGroup>
  );
}
