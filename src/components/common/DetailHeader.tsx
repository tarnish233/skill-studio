import type { ReactNode } from "react";
import { ArrowLeft, Save } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export interface DetailSave {
  /** 有 `form` 时由表单的提交处理保存，这里不用再传 */
  onClick?: () => void;
  /** 关联的表单 id：保存按钮成为它的提交按钮，在输入框里按回车也会保存 */
  form?: string;
  pending: boolean;
  disabled: boolean;
  /** 说明保存会写到哪里 */
  title: string;
  /** 默认「保存」 */
  label?: string;
  /** 进行中的说法，默认「保存中…」 */
  pendingLabel?: string;
}

/**
 * 页内展开的详情（项目、分组、MCP）共用的标题栏：返回、名称与一行说明、各 Agent 的启停
 * （与卡片同一个）、页面自己的操作，最后是保存。
 */
export function DetailHeader({
  title,
  badges,
  subtitle,
  mono,
  onBack,
  agents,
  actions,
  save,
}: {
  title: string;
  /** 名称后的徽标 */
  badges?: ReactNode;
  subtitle?: ReactNode;
  /** 说明是路径、命令或地址：用等宽字体 */
  mono?: boolean;
  onBack: () => void;
  /** 各 Agent 的启停图标 */
  agents?: ReactNode;
  actions?: ReactNode;
  save?: DetailSave;
}) {
  return (
    <div className="flex items-center gap-3 py-4">
      <Button
        type="button"
        variant="outline"
        size="icon"
        title="返回"
        aria-label="返回"
        onClick={onBack}
      >
        <ArrowLeft className="h-4 w-4" />
      </Button>
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-2">
          <p className="truncate text-base font-semibold">{title}</p>
          {badges}
        </div>
        {subtitle && (
          <p
            className={cn(
              "truncate text-muted-foreground",
              mono ? "font-mono text-[11px]" : "text-xs",
            )}
            title={typeof subtitle === "string" ? subtitle : undefined}
          >
            {subtitle}
          </p>
        )}
      </div>
      {agents}
      {actions}
      {save && (
        <Button
          type={save.form ? "submit" : "button"}
          form={save.form}
          size="sm"
          title={save.title}
          disabled={save.pending || save.disabled}
          onClick={save.form ? undefined : save.onClick}
        >
          <Save className="h-4 w-4" />
          {save.pending
            ? (save.pendingLabel ?? "保存中…")
            : (save.label ?? "保存")}
        </Button>
      )}
    </div>
  );
}
