import { AlertTriangle, Info } from "lucide-react";
import { useSyncExternalStore } from "react";
import { getTarget, subscribeTransport } from "@/lib/api/transport";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface ConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: React.ReactNode;
  confirmText?: string;
  cancelText?: string;
  variant?: "destructive" | "info";
  pending?: boolean;
  confirmDisabled?: boolean;
  onConfirm: () => void;
}

/** 不可逆操作的二次确认。删除类用 destructive，其余用 info。 */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmText = "确认",
  cancelText = "取消",
  variant = "destructive",
  pending,
  confirmDisabled,
  onConfirm,
}: ConfirmDialogProps) {
  const target = useSyncExternalStore(subscribeTransport, getTarget, getTarget);
  const Icon = variant === "destructive" ? AlertTriangle : Info;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-sm" hideClose>
        <DialogHeader className="border-b-0 bg-transparent pb-2">
          <DialogTitle className="flex items-center gap-2">
            <Icon
              className={
                variant === "destructive"
                  ? "h-4 w-4 text-red-500"
                  : "h-4 w-4 text-blue-500"
              }
            />
            {title}
          </DialogTitle>
          {target.id !== "local" && (
            <p className="text-xs text-muted-foreground">
              操作目标：{target.name}
            </p>
          )}
          {description && (
            <DialogDescription className="pt-1 leading-relaxed">
              {description}
            </DialogDescription>
          )}
        </DialogHeader>
        <DialogFooter className="border-t-0 bg-transparent pt-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => onOpenChange(false)}
            disabled={pending}
          >
            {cancelText}
          </Button>
          <Button
            variant={variant === "destructive" ? "destructive" : "default"}
            size="sm"
            onClick={onConfirm}
            disabled={pending || confirmDisabled}
          >
            {pending ? "处理中…" : confirmText}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
