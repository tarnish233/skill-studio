import type { ReactNode } from "react";
import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DetailHeader,
  type DetailSave,
} from "@/components/common/DetailHeader";
import { systemApi } from "@/lib/api";

/**
 * Skill 与 MCP 项目详情的标题栏：共用的详情标题栏，第二行是项目路径，
 * 操作前多一个「打开项目目录」。保存即写入已启用的 Agent。
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
  save: DetailSave;
}) {
  return (
    <DetailHeader
      title={name}
      subtitle={root}
      mono
      onBack={onBack}
      agents={agents}
      actions={
        <>
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
        </>
      }
      save={save}
    />
  );
}
