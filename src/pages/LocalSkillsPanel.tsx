import { useTarget } from "@/components/targets/TargetProvider";
import { uploadLocalSkill } from "@/lib/api/transport";
import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { FolderOpen, Download, Loader2, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { projectsApi, skillsApi } from "@/lib/api";
import { useSkills } from "@/hooks/useData";

export function LocalSkillsPanel() {
  const target = useTarget();
  const [path, setPath] = useState("");
  const [error, setError] = useState<string | null>(null);
  const client = useQueryClient();
  const { data: installed = [] } = useSkills();
  const discover = useMutation({
    mutationFn: skillsApi.discoverLocal,
    onMutate: () => setError(null),
    onError: (e) => setError(String(e)),
  });
  const importing = useMutation({
    mutationFn: skillsApi.importLocal,
    onMutate: () => setError(null),
    onSuccess: async (result, path) => {
      await client.invalidateQueries({ queryKey: ["skills"] });
      if (!result.changed) {
        toast.success("Hub 里的副本已是最新");
        return;
      }
      const existing = installed.some(
        (s) => s.installation?.source === `local:${path}`,
      );
      toast.success(
        existing
          ? "已更新 Hub 里的副本，原目录已保留"
          : "已导入 Skill Hub，原目录已保留",
      );
    },
    onError: (e) => setError(String(e)),
  });
  const choose = async () => {
    try {
      const selected = await projectsApi.pickDirectory();
      if (typeof selected === "string") {
        setPath(selected);
        discover.mutate(selected);
      }
    } catch (e) {
      setError(String(e));
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
      {target.id !== "local" && (
        <Button
          className="mb-3 self-start"
          variant="outline"
          onClick={async () => {
            try {
              const uploaded = await uploadLocalSkill(() =>
                open({
                  directory: true,
                  multiple: false,
                  title: "选择本机上的单个 Skill",
                }),
              );
              if (uploaded) {
                await client.invalidateQueries({ queryKey: ["skills"] });
                toast.success(`已上传到 ${target.name}`);
              }
            } catch (e) {
              setError(String(e));
            }
          }}
        >
          从本机上传一个 Skill
        </Button>
      )}
      <p className="mb-4 text-sm text-muted-foreground">
        支持单个 skill 或包含多个 skill 的目录。导入后由 Skill Studio
        管理，保留原文件，不自动注册到 Agent。
      </p>
      {error && (
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
                const done = installed.some(
                  (s) =>
                    !s.diagnostics?.length &&
                    s.installation?.source === `local:${skill.path}`,
                );
                const pending =
                  importing.isPending && importing.variables === skill.path;
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
                        disabled={importing.isPending || !!skill.error}
                        onClick={() => importing.mutate(skill.path)}
                      >
                        {pending ? (
                          <Loader2 className="h-4 w-4 animate-spin" />
                        ) : done ? (
                          <RefreshCw className="h-4 w-4" />
                        ) : (
                          <Download className="h-4 w-4" />
                        )}
                        {pending ? "导入中…" : done ? "重新导入" : "导入"}
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
              选择{target.id === "local" ? "本机" : "服务器"}目录，导入你的
              skill
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
