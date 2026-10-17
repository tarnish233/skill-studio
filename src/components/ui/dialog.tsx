import * as React from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { cn } from "@/lib/utils";

const Dialog = DialogPrimitive.Root;
const DialogTrigger = DialogPrimitive.Trigger;
const DialogPortal = DialogPrimitive.Portal;
const DialogClose = DialogPrimitive.Close;

/**
 * 入场动画 180ms；过了 ENTRY_SETTLE_MS 就打上 data-entered，CSS 据此摘掉入场动画、直接显示终态。
 * 正常情况下动画早已播完，这一步没有任何可见变化；但透明原生窗口未激活时 WKWebView 的
 * 动画时钟可能停住（v0.5.3 原生工具栏实测，见 1015bbe），弹窗会一直停在半透明的
 * 起点——定时器走任务队列，不受动画时钟影响，能把它拉回来。
 */
const ENTRY_SETTLE_MS = 400;

function useEntrySettledRef<T extends HTMLElement>(
  forwarded: React.ForwardedRef<T>,
) {
  return React.useCallback(
    (node: T | null) => {
      if (typeof forwarded === "function") forwarded(node);
      else if (forwarded) forwarded.current = node;
      if (node)
        window.setTimeout(
          () => node.setAttribute("data-entered", ""),
          ENTRY_SETTLE_MS,
        );
    },
    [forwarded],
  );
}

const DialogOverlay = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Overlay>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay>
>(({ className, ...props }, ref) => {
  const settledRef = useEntrySettledRef(ref);
  return (
    <DialogPrimitive.Overlay
      ref={settledRef}
      data-slot="dialog-overlay"
      className={cn(
        "fixed inset-0 z-50 bg-black/50 backdrop-blur-sm",
        "data-[state=open]:animate-[ss-dialog-overlay-in_180ms_linear] data-[state=closed]:animate-out data-[state=closed]:fade-out-0",
        className,
      )}
      {...props}
    />
  );
});
DialogOverlay.displayName = DialogPrimitive.Overlay.displayName;

const DialogContent = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content> & {
    hideClose?: boolean;
  }
>(({ className, children, hideClose, ...props }, ref) => {
  const settledRef = useEntrySettledRef(ref);
  return (
    <DialogPortal>
      <DialogOverlay />
      <DialogPrimitive.Content
        ref={settledRef}
        data-slot="dialog-content"
        // 点遮罩不关闭：表单填一半被误关很恼人（cc-switch 的处理）
        onInteractOutside={(e) => e.preventDefault()}
        // Independent translate keeps centering intact when animate-in/out changes transform.
        className={cn(
          "fixed left-1/2 top-1/2 z-50 flex max-h-[90vh] w-full max-w-lg [translate:-50%_-50%] flex-col",
          "border border-border-default bg-background text-foreground shadow-lg duration-200 sm:rounded-lg",
          "data-[state=open]:animate-[ss-dialog-in_180ms_linear] data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95",
          className,
        )}
        {...props}
      >
        {children}
        {!hideClose && (
          <DialogPrimitive.Close className="absolute right-4 top-4 rounded-sm text-muted-foreground opacity-70 transition-opacity hover:opacity-100 focus:outline-none disabled:pointer-events-none">
            <X className="h-4 w-4" />
            <span className="sr-only">关闭</span>
          </DialogPrimitive.Close>
        )}
      </DialogPrimitive.Content>
    </DialogPortal>
  );
});
DialogContent.displayName = DialogPrimitive.Content.displayName;

function DialogHeader({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn(
        "flex flex-shrink-0 flex-col space-y-1.5 border-b border-border-default bg-muted/20 px-6 py-5 text-left",
        className,
      )}
      {...props}
    />
  );
}

function DialogFooter({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn(
        "flex flex-shrink-0 flex-col-reverse gap-2 border-t border-border-default bg-muted/20 px-6 py-5 sm:flex-row sm:items-center sm:justify-end",
        className,
      )}
      {...props}
    />
  );
}

const DialogTitle = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Title>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Title
    ref={ref}
    className={cn(
      "text-lg font-semibold leading-tight tracking-tight",
      className,
    )}
    {...props}
  />
));
DialogTitle.displayName = DialogPrimitive.Title.displayName;

const DialogDescription = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Description>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Description
    ref={ref}
    className={cn("text-sm text-muted-foreground", className)}
    {...props}
  />
));
DialogDescription.displayName = DialogPrimitive.Description.displayName;

export {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
};
