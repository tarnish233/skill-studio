import { FolderGit2 } from "lucide-react";
import type { BoardLabels } from "@/components/groups/GroupBoard";
import type { CardLabels } from "@/components/groups/groupCardState";

/** Skill 与 MCP 项目页共用的说法；`what` 是成员的叫法：「skill」或「MCP」 */
export const PROJECT_BOARD_LABELS = (what: string): BoardLabels => ({
  item: "项目",
  search: "搜索项目…",
  create: "添加项目",
  emptyTitle: "还没有项目",
  emptyDescription: `添加项目目录，选择只在这个项目里用的 ${what}，再在卡片上为需要的 Agent 启用。`,
  emptyIcon: FolderGit2,
});
export const PROJECT_CARD_LABELS = (what: string): CardLabels => ({
  subject: "项目",
  empty: `项目还没有选择 ${what}`,
  changed: "成员或 Hub 里的源在写入后有变化，还没写入：",
});
export const PROJECT_REMOVAL = {
  verb: "移除",
  title: "移除项目",
  blocked: "请先停用所有 Agent",
};
