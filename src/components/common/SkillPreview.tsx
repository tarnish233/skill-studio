import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ExternalLink } from "lucide-react";
import { toast } from "sonner";
import { invoke } from "@/lib/api/transport";
import { skillsApi } from "@/lib/api/skills";
import { useTarget } from "@/components/targets/TargetProvider";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";

/** 与 Rust `SkillDocument` 对应：原文、frontmatter、去掉 frontmatter 的正文 */
type SkillDocument = {
  raw: string;
  frontmatter: {
    name?: string | null;
    description?: string | null;
    extraKeys?: string[];
    malformed?: boolean;
    error?: string | null;
  };
  body: string;
};

export function useSkillPreview() {
  const [skill, setSkill] = useState<{ name: string; path: string } | null>(
    null,
  );
  const target = useTarget();
  // 远程服务器上的文件打不开在本机：入口置灰并说明，而不是抛出远程错误
  const local = target.id === "local";
  const query = useQuery({
    queryKey: ["skill-preview", skill?.path],
    queryFn: async () => {
      const payload = await invoke<SkillDocument | string>(
        "read_skill_document",
        { path: skill!.path },
      );
      // 旧版远程辅助程序仍返回纯文本；按整篇正文展示，不因为字段形状而空屏
      return typeof payload === "string"
        ? { raw: payload, frontmatter: {}, body: payload }
        : payload;
    },
    enabled: !!skill,
    staleTime: 0,
    gcTime: 0,
    retry: false,
  });
  const document = query.data;
  const frontmatter = document?.frontmatter;
  const extraKeys = frontmatter?.extraKeys ?? [];
  const documentPath = skill ? `${skill.path}/SKILL.md` : "";
  return {
    previewSkill: (name: string, path: string) => setSkill({ name, path }),
    previewDialog: (
      <Dialog
        open={!!skill}
        onOpenChange={(open) => {
          if (!open) setSkill(null);
        }}
      >
        {/* overflow-hidden：滚动区是方形盒子，必须由外层圆角裁掉四角 */}
        <DialogContent className="max-h-[85vh] max-w-3xl overflow-hidden">
          <DialogHeader>
            <DialogTitle>{skill?.name}</DialogTitle>
            <DialogDescription className="break-all">
              {documentPath}
            </DialogDescription>
          </DialogHeader>
          {/* 内边距与 DialogHeader 一致；min-h-0 让长文档在这个高度内滚动 */}
          <div className="min-h-0 space-y-3 overflow-y-auto px-6 py-4">
            {query.isPending && (
              <p className="text-sm text-muted-foreground">读取中…</p>
            )}
            {query.isError && (
              <p role="alert" className="text-sm text-destructive">
                无法预览：{String(query.error)}
              </p>
            )}
            {document && (
              <>
                {frontmatter?.description && (
                  <p className="text-xs text-muted-foreground">
                    {frontmatter.description}
                  </p>
                )}
                {frontmatter?.malformed && (
                  <p role="alert" className="text-xs text-destructive">
                    frontmatter 解析失败
                    {frontmatter.error ? `：${frontmatter.error}` : ""}
                  </p>
                )}
                {extraKeys.length > 0 && (
                  <p className="text-xs text-muted-foreground">
                    其他 frontmatter 字段：{extraKeys.join("、")}
                  </p>
                )}
                {document.body ? (
                  <pre className="whitespace-pre-wrap break-words font-mono text-sm leading-relaxed">
                    {document.body}
                  </pre>
                ) : (
                  <p className="text-sm text-muted-foreground">
                    {document.raw
                      ? "此文档只有 frontmatter，没有正文"
                      : "文件为空"}
                  </p>
                )}
              </>
            )}
          </div>
          <footer className="flex shrink-0 items-center justify-end gap-3 border-t border-border-default px-6 py-4">
            {!local && (
              <p className="text-xs text-muted-foreground">
                文件在 {target.name} 上，无法用本机应用打开
              </p>
            )}
            <Button
              variant="outline"
              size="sm"
              disabled={!local}
              onClick={async () => {
                try {
                  await skillsApi.openDocument(skill!.path);
                } catch (error) {
                  // 后端消息已是可展示文案（包含“无法用默认应用打开”前缀）
                  toast.error(String(error));
                }
              }}
            >
              <ExternalLink className="h-4 w-4" />
              用默认应用打开
            </Button>
          </footer>
        </DialogContent>
      </Dialog>
    ),
  };
}
