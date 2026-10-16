import { ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";

/**
 * 所有管理列表的行骨架。分割线由行自己的 border-b 提供。
 *
 * 交互统一为：能进详情的行整行可点（`onPreview`），行尾常驻一个淡淡的 › 作
 * 为提示；行内的按钮只做就地动作，点它们不会触发整行。只做就地切换的行传
 * `onClick`，此时不显示 ›。
 */
export function ListItemRow({
  children,
  isLast,
  card = false,
  className,
  onClick,
  onPreview,
  previewLabel,
}: {
  children: React.ReactNode;
  isLast?: boolean;
  card?: boolean;
  className?: string;
  onClick?: () => void;
  onPreview?: () => void;
  previewLabel?: string;
}) {
  return (
    <div
      role={onPreview ? "button" : undefined}
      tabIndex={onPreview ? 0 : undefined}
      aria-label={previewLabel}
      onKeyDown={(e) => {
        if (
          onPreview &&
          e.target === e.currentTarget &&
          (e.key === "Enter" || e.key === " ")
        ) {
          e.preventDefault();
          onPreview();
        }
      }}
      onClick={(e) => {
        if (!onPreview) {
          onClick?.();
          return;
        }
        if (
          e.target instanceof Element &&
          e.currentTarget.contains(e.target) &&
          !e.target.closest(
            "button,a,input,select,textarea,[role=switch],[role=checkbox],[role=dialog]",
          )
        )
          onPreview();
      }}
      className={cn(
        "group flex items-center gap-3 px-4 py-2.5 transition-colors hover:bg-muted/50",
        card
          ? "min-h-20 rounded-xl border border-border-default bg-card px-5 py-4"
          : !isLast && "border-b border-border-default",
        (onClick || onPreview) && "cursor-pointer",
        className,
      )}
    >
      {children}
      {onPreview && (
        <ChevronRight
          aria-hidden
          className="h-4 w-4 shrink-0 text-muted-foreground/40 transition-colors group-hover:text-muted-foreground"
        />
      )}
    </div>
  );
}

/** 列表外框。圆角 + 边框 + overflow-hidden，让首末行的圆角贴合。 */
export function ListContainer({
  children,
  cards = false,
  className,
}: {
  children: React.ReactNode;
  cards?: boolean;
  className?: string;
}) {
  return (
    <div
      className={cn(
        cards
          ? "flex flex-col gap-3"
          : "overflow-hidden rounded-xl border border-border-default",
        className,
      )}
    >
      {children}
    </div>
  );
}

/** Hover/focus reveals actions without changing the card's layout. */
export function RowActions({
  children,
  busy = false,
}: {
  children: React.ReactNode;
  busy?: boolean;
}) {
  return (
    <div
      aria-busy={busy}
      className={cn(
        "pointer-events-none flex flex-shrink-0 items-center gap-1 text-muted-foreground opacity-0 transition-opacity duration-150 focus-within:pointer-events-auto focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100 [&:has([data-state=open])]:pointer-events-auto [&:has([data-state=open])]:opacity-100",
        busy && "opacity-100 [&_button:disabled]:opacity-100",
      )}
    >
      {children}
    </div>
  );
}
