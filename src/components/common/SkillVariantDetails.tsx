import { useState } from "react";
import { toast } from "sonner";
import type { SkillView } from "@/types";
import { variantLabel } from "@/lib/skillVariants";
import { systemApi } from "@/lib/api";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";

export function SkillVariantDetails({ skill }: { skill: SkillView }) {
  const [open, setOpen] = useState(false);
  const variants = skill.installation?.variants ?? [];
  if (!variants.length) return null;
  return (
    <>
      <button
        className="rounded-md bg-blue-500/10 px-1.5 py-0.5 text-xs font-medium text-blue-600 dark:text-blue-300"
        onClick={() => setOpen(true)}
        aria-label={`${skill.name} 适配版本`}
      >
        {variants.length > 1 ? "多端适配" : "专用适配"}
      </button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{skill.name} · 适配版本</DialogTitle>
            <DialogDescription>
              Hub 统一管理一个 Skill，部署时优先使用目标 Agent
              的专用版，其次使用通用版。没有匹配版本时不会使用其他 Agent
              的专用版。
            </DialogDescription>
          </DialogHeader>
          <div className="min-h-0 space-y-4 overflow-y-auto px-6 py-4 text-sm">
            <p className="break-all text-xs text-muted-foreground">
              {skill.installation?.source}
            </p>
            {variants.map((variant) => (
              <div
                key={variant.key}
                className="flex items-center justify-between gap-3"
              >
                <div className="min-w-0">
                  <p className="font-medium">{variantLabel(variant.key)}</p>
                  <p className="break-all font-mono text-xs text-muted-foreground">
                    {variant.repositoryPath || "仓库根目录"}
                  </p>
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    void systemApi
                      .revealPath(
                        `${skill.sourcePath}/.skill-studio-variants/${variant.key}`,
                      )
                      .catch((e) => toast.error(String(e)))
                  }
                >
                  打开变体目录
                </Button>
              </div>
            ))}
            <div className="space-y-2 border-t pt-3">
              <p className="font-medium">各 Agent 的使用版本</p>
              {Object.entries(skill.agents).map(([agent, state]) => {
                const selected =
                  variants.find((v) => v.key === agent) ??
                  variants.find((v) => v.key === "generic");
                return (
                  <p key={agent} className="text-xs text-muted-foreground">
                    {variantLabel(agent).replace(" 专用版", "")}：
                    {state.unavailableReason ??
                      (selected ? variantLabel(selected.key) : "无匹配版本")}
                  </p>
                );
              })}
            </div>
            <p className="text-xs text-muted-foreground">
              通用版表示来源目录未限定 Agent，不代表已验证所有 Agent
              的完整兼容性。这里只安装 Skill，不包含插件、hooks 或扩展。
            </p>
            <p className="text-xs text-muted-foreground">
              Hub
              根目录保留安装时的预览副本；实际部署和编辑应使用上列变体目录。分组和项目仍引用同一个
              Skill。
            </p>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}
