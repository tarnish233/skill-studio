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

/** 本地读一篇 SKILL.md 通常只要几毫秒；超过这个时间（大文件 / 远程）就先开窗显示「读取中」 */
const SLOW_READ_MS = 120;
let previewSeq = 0;

type Preview = {
  name: string;
  path: string;
  /** 每次打开换一个：查询键随之变化，重开一定重新读文件 */
  seq: number;
  /** 关窗后为 false：晚到的读取结果不会再把弹窗打开 */
  open: boolean;
  /** 已经等过 SLOW_READ_MS 仍没读完 */
  slow: boolean;
};

async function loadDocument(path: string): Promise<SkillDocument> {
  const payload = await invoke<SkillDocument | string>("read_skill_document", {
    path,
  });
  // 旧版远程辅助程序仍返回纯文本；按整篇正文展示，不因为字段形状而空屏
  return typeof payload === "string"
    ? { raw: payload, frontmatter: {}, body: payload }
    : payload;
}

export function useSkillPreview() {
  const [preview, setPreview] = useState<Preview | null>(null);
  const target = useTarget();
  // 远程服务器上的文件打不开在本机：入口置灰并说明，而不是抛出远程错误
  const local = target.id === "local";
  // 点击那一刻查询就挂在这个 observer 上，由它自己读。不要先 fetchQuery 预取再交接：
  // gcTime: 0 的缓存会在「预取完成 → 弹窗 effect 订阅」的空档里被回收（取决于
  // setTimeout(0) 与 React 调度谁先跑），结果是开窗后又读一次、闪出「读取中」。
  const query = useQuery({
    queryKey: ["skill-preview", preview?.path, preview?.seq],
    queryFn: () => loadDocument(preview!.path),
    enabled: !!preview,
    staleTime: Infinity,
    gcTime: 0,
    retry: false,
  });
  // 读完（或失败）才开窗：第一帧就是最终内容和高度，不会「小窗淡入 → 文档到达 →
  // 瞬间长高」。读得慢时先开窗显示「读取中」，并给读取态留出高度。
  const open = !!preview?.open && (!query.isPending || preview.slow);
  const document = query.data;
  const frontmatter = document?.frontmatter;
  const extraKeys = frontmatter?.extraKeys ?? [];
  const documentPath = preview ? `${preview.path}/SKILL.md` : "";
  return {
    previewSkill: (name: string, path: string) => {
      const seq = ++previewSeq;
      setPreview({ name, path, seq, open: true, slow: false });
      // 只认最后一次点击：先点的慢请求、关窗后才到的计时都不会再改弹窗
      setTimeout(
        () => setPreview((p) => (p?.seq === seq ? { ...p, slow: true } : p)),
        SLOW_READ_MS,
      );
    },
    previewDialog: (
      <Dialog
        open={open}
        onOpenChange={(next) => {
          // 关窗只收起，不清空：退场动画期间标题和正文保持不变
          if (!next) setPreview((p) => (p ? { ...p, open: false } : p));
        }}
      >
        {/* overflow-hidden：滚动区是方形盒子，必须由外层圆角裁掉四角 */}
        <DialogContent className="max-h-[85vh] max-w-3xl overflow-hidden">
          <DialogHeader>
            <DialogTitle>{preview?.name}</DialogTitle>
            <DialogDescription className="break-all">
              {documentPath}
            </DialogDescription>
          </DialogHeader>
          {/* 内边距与 DialogHeader 一致；min-h-0 让长文档在这个高度内滚动 */}
          <div className="min-h-0 space-y-3 overflow-y-auto px-6 py-4">
            {query.isPending && (
              <p className="flex min-h-[40vh] text-sm text-muted-foreground">
                读取中…
              </p>
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
                  await skillsApi.openDocument(preview!.path);
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
