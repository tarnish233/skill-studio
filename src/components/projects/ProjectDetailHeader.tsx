import type { ReactNode } from "react";
import { ArrowLeft, FolderOpen, Save } from "lucide-react";
import { Button } from "@/components/ui/button";
import { systemApi } from "@/lib/api";

/**
 * Skill 与 MCP 项目详情共用的标题栏：返回、项目名与路径、各 Agent 的启停（与卡片同一个），
 * 打开目录、页面自己的操作，最后是保存。保存即写入已启用的 Agent。
 */
export function ProjectDetailHeader({
  name,
  root,
  onBack,
  agents,
  actions,
  save,
}: {
  name: string;
  root: string;
  onBack: () => void;
  /** 各 Agent 的启停图标 */
  agents: ReactNode;
  actions?: ReactNode;
  save: {
    onClick: () => void;
    pending: boolean;
    disabled: boolean;
    /** 说明保存会写入哪里 */
    title: string;
  };
}) {
  return (
    <div className="flex items-center gap-3 py-4">
      <Button
        variant="outline"
        size="icon"
        title="返回"
        aria-label="返回"
        onClick={onBack}
      >
        <ArrowLeft className="h-4 w-4" />
      </Button>
      <div className="min-w-0 flex-1">
        <p className="truncate text-base font-semibold">{name}</p>
        <p className="truncate font-mono text-[11px] text-muted-foreground">
          {root}
        </p>
      </div>
      {agents}
      <Button
        variant="ghost"
        size="icon"
        title="打开项目目录"
        aria-label="打开项目目录"
        onClick={() => void systemApi.revealPath(root)}
      >
        <FolderOpen className="h-4 w-4" />
      </Button>
      {actions}
      <Button
        size="sm"
        title={save.title}
        disabled={save.pending || save.disabled}
        onClick={save.onClick}
      >
        <Save className="h-4 w-4" />
        {save.pending ? "保存中…" : "保存"}
      </Button>
    </div>
  );
}
