import { useTarget } from "@/components/targets/TargetProvider";
import { getTarget } from "@/lib/api/transport";
import { useEffect, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { FolderOpen, PackagePlus, Loader2, Check } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { projectsApi, skillsApi } from "@/lib/api";
import { useSkills, useInvalidateAfterWrite } from "@/hooks/useData";

type Selection = {
  path: string;
  name: string;
  targetId: string;
  targetName: string;
};

export function LocalSkillsPanel() {
  const target = useTarget();
  const [path, setPath] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [selection, setSelection] = useState<Selection | null>(null);
  const [collected, setCollected] = useState<Set<string>>(new Set());
  const invalidate = useInvalidateAfterWrite();
  const { data: installed = [] } = useSkills();
  const discover = useMutation({
    mutationFn: skillsApi.discoverLocal,
    onMutate: () => setError(null),
    onError: (e) => setError(String(e)),
  });
  const importing = useMutation({
    mutationFn: (value: Selection) => {
      if (getTarget().id !== value.targetId)
        throw new Error("管理目标已切换，请重新选择来源");
      return skillsApi.importLocal(value.path);
    },
    onMutate: () => setError(null),
    onSuccess: (result, value) => {
      if (getTarget().id === value.targetId) {
        setCollected((previous) => new Set([...previous, value.path]));
        setSelection(null);
      }
      toast.success(
        result.changed
          ? `已迁移收录到 ${value.targetName} 的 Hub，原位置仅保留软链接`
          : "真身已在 Hub，无需再次迁移",
      );
    },
    onError: (e, value) => {
      if (getTarget().id === value.targetId) setError(String(e));
    },
    onSettled: invalidate,
  });
  useEffect(() => {
    setPath("");
    setError(null);
    setSelection(null);
    setCollected(new Set());
    discover.reset();
  }, [target.id]);
  const choose = async () => {
    const destination = target.id;
    try {
      const selected = await projectsApi.pickDirectory();
      if (typeof selected === "string" && getTarget().id === destination) {
        setPath(selected);
        discover.mutate(selected);
      }
    } catch (e) {
      if (getTarget().id === destination) setError(String(e));
    }
  };
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <form
        className="flex gap-3 py-4"
        onSubmit={(e) => {
          e.preventDefault();
          if (path.trim()) discover.mutate(path.trim());
        }}
      >
        <Input
          aria-label={target.id === "local" ? "本地目录" : "服务器目录"}
          placeholder={
            target.id === "local"
              ? "选择或输入本地 skill 目录…"
              : "选择或输入服务器上的 skill 目录…"
          }
          value={path}
          onChange={(e) => setPath(e.target.value)}
        />
        <Button
          type="button"
          variant="outline"
          disabled={discover.isPending || importing.isPending}
          onClick={() => void choose()}
        >
          <FolderOpen className="h-4 w-4" />
          选择目录
        </Button>
        <Button
          type="submit"
          disabled={!path.trim() || discover.isPending || importing.isPending}
        >
          扫描
        </Button>
      </form>
      <p className="mb-4 text-sm text-muted-foreground">
        收录与托管均会将原件迁入 Hub，原位置仅保留指向 Hub 的软链接。
        不能创建软链接时会停止并回滚，不保留独立原件，不改变原有启停策略。
        {target.id !== "local" &&
          "远程收录请选择服务器上的目录，不支持跨机器复制上传。"}
      </p>
      {error && !selection && (
        <p role="alert" className="mb-4 text-sm text-destructive">
          {error}
        </p>
      )}
      <div className="min-h-0 flex-1 overflow-auto pb-6">
        {discover.isPending ? (
          <p className="py-16 text-center text-muted-foreground">
            正在识别 skill…
          </p>
        ) : discover.isSuccess ? (
          discover.data.length ? (
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
              {discover.data.map((skill) => {
                const done =
                  collected.has(skill.path) ||
                  installed.some(
                    (source) =>
                      !source.diagnostics?.length &&
                      source.origin.kind === "hub" &&
                      source.id === skill.hubId,
                  );
                const pending =
                  importing.isPending &&
                  importing.variables?.path === skill.path;
                return (
                  <article
                    key={skill.path}
                    className="flex min-w-0 flex-col rounded-xl border bg-card"
                  >
                    <div className="flex-1 p-5">
                      <h2 className="break-words font-semibold">
                        {skill.name}
                      </h2>
                      {skill.description && (
                        <p className="mt-2 line-clamp-2 text-sm text-muted-foreground">
                          {skill.description}
                        </p>
                      )}
                      <p className="mt-2 break-all text-xs text-muted-foreground">
                        {skill.path}
                      </p>
                      {skill.error && (
                        <p className="mt-2 text-sm text-destructive">
                          {skill.error}
                        </p>
                      )}
                    </div>
                    <div className="border-t p-3">
                      <Button
                        className="w-full bg-blue-500 text-white hover:bg-blue-600"
                        disabled={importing.isPending || !!skill.error || done}
                        onClick={() =>
                          setSelection({
                            path: skill.path,
                            name: skill.name,
                            targetId: target.id,
                            targetName: target.name,
                          })
                        }
                      >
                        {pending ? (
                          <Loader2 className="h-4 w-4 animate-spin" />
                        ) : done ? (
                          <Check className="h-4 w-4" />
                        ) : (
                          <PackagePlus className="h-4 w-4" />
                        )}
                        {pending ? "迁移中…" : done ? "已托管" : "收录到 Hub"}
                      </Button>
                    </div>
                  </article>
                );
              })}
            </div>
          ) : (
            <p className="py-16 text-center text-muted-foreground">
              该目录未发现包含 SKILL.md 的 skill。
            </p>
          )
        ) : (
          <div className="flex flex-col items-center gap-4 py-20 text-muted-foreground">
            <FolderOpen className="h-12 w-12 opacity-30" />
            <p>
              选择{target.id === "local" ? "本机" : "服务器"}目录，将 skill
              原件迁入 Hub
            </p>
          </div>
        )}
      </div>
      <ConfirmDialog
        open={selection !== null}
        onOpenChange={(open) => {
          if (!open && !importing.isPending) {
            setSelection(null);
            setError(null);
          }
        }}
        title={`迁移收录 ${selection?.name ?? "skill"}？`}
        variant="info"
        confirmText="迁移到 Hub"
        pending={importing.isPending}
        description={
          <>
            将原件迁入 {selection?.targetName} 的
            Hub，保存完整备份，原位置仅留软链接。
            无法创建软链接时回滚，不回退复制；原有启停状态不变，不额外部署到
            Agent。
            <span className="mt-2 block break-all font-mono text-xs">
              {selection?.path}
            </span>
            {error && (
              <span role="alert" className="mt-2 block text-destructive">
                {error}
              </span>
            )}
          </>
        }
        onConfirm={() => {
          if (selection) importing.mutate(selection);
        }}
      />
    </div>
  );
}
