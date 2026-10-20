import { queryKeys, NAVIGATION_STALE_TIME } from "@/lib/queryKeys";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@/lib/api/transport";
import { ArchiveRestore, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { ConfirmDialog } from "./ConfirmDialog";
import { SummaryAction } from "./SummaryBar";

interface Backup {
  id: string;
  name: string;
  scope: string;
  originalPath: string;
  deletedAt: number;
}
export function SkillBackups({ scope }: { scope: string }) {
  const [open, setOpen] = useState(false);
  const [purging, setPurging] = useState<Backup | null>(null);
  const qc = useQueryClient();
  const { data = [], error } = useQuery({
    queryKey: queryKeys.skillBackups,
    queryFn: () => invoke<Backup[]>("list_skill_backups"),
    staleTime: NAVIGATION_STALE_TIME,
  });
  const backups = data.filter((r) =>
    scope === "projects"
      ? r.scope.startsWith("project:")
      : scope === "hub"
        ? r.scope === "hub" || r.scope.startsWith("agent:")
        : r.scope === scope,
  );
  const action = useMutation({
    mutationFn: ({ id, purge }: { id: string; purge: boolean }) =>
      invoke(purge ? "purge_skill_file" : "restore_skill_file", { id }),
    onSuccess: async (_, args) => {
      setPurging(null);
      await qc.invalidateQueries();
      toast.success(args.purge ? "备份已彻底删除" : "skill 已恢复");
    },
    onError: (e) => toast.error(String(e)),
  });
  return (
    <>
      <SummaryAction
        onClick={() => setOpen(true)}
        title={`已备份：删除 skill 时留下的备份，共 ${backups.length} 个。点击查看，可恢复到原位置或彻底删除`}
      >
        已备份 {backups.length}
      </SummaryAction>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>已备份的 skill</DialogTitle>
            <DialogDescription>
              恢复到原位置；已有同名内容时不会覆盖。
            </DialogDescription>
          </DialogHeader>
          <div className="max-h-96 min-h-0 space-y-3 overflow-y-auto px-6 py-4">
            {error && (
              <p role="alert" className="text-sm text-destructive">
                {String(error)}
              </p>
            )}
            {!backups.length && (
              <p className="text-sm text-muted-foreground">暂无备份</p>
            )}
            {backups.map((r) => (
              <div key={r.id} className="rounded-xl border p-3">
                <p className="font-medium">{r.name}</p>
                <p className="break-all text-xs text-muted-foreground">
                  {r.originalPath}
                </p>
                <p className="text-xs text-muted-foreground">
                  {new Date(r.deletedAt * 1000).toLocaleString()}
                </p>
                <div className="mt-2 flex gap-2">
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={action.isPending}
                    onClick={() => action.mutate({ id: r.id, purge: false })}
                  >
                    <ArchiveRestore className="h-4 w-4" />
                    恢复
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={action.isPending}
                    onClick={() => setPurging(r)}
                  >
                    彻底删除
                  </Button>
                </div>
              </div>
            ))}
          </div>
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        open={!!purging}
        onOpenChange={(v) => {
          if (!v && !action.isPending) setPurging(null);
        }}
        title="彻底删除备份？"
        description={`删除 ${purging?.name ?? ""} 后无法恢复。`}
        pending={action.isPending}
        onConfirm={() => {
          if (purging) action.mutate({ id: purging.id, purge: true });
        }}
      />
    </>
  );
}

export function DeleteSkillButton({
  scope,
  path,
  name,
  disabled = false,
}: {
  scope: string;
  path: string;
  name: string;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const qc = useQueryClient();
  const mutation = useMutation({
    mutationFn: () => invoke("delete_skill_file", { scope, path }),
    onSuccess: async () => {
      setOpen(false);
      await qc.invalidateQueries();
      toast.success("已删除；非软链接文件已保留备份");
    },
    onError: (e) => toast.error(String(e)),
  });
  return (
    <>
      <Button
        variant="ghost"
        size="icon"
        title={`删除 ${path}`}
        aria-label={`删除 ${name}`}
        disabled={disabled || mutation.isPending}
        onClick={() => setOpen(true)}
      >
        <Trash2 className="h-4 w-4" />
      </Button>
      <ConfirmDialog
        open={open}
        onOpenChange={(v) => {
          if (!v && !mutation.isPending) setOpen(false);
        }}
        title={`删除 ${name}？`}
        description={
          <>
            非软链接文件会保留备份，可以恢复；软链接只移除链接，不备份或删除目标。
            <span className="block break-all pt-2 text-xs">{path}</span>
          </>
        }
        pending={mutation.isPending}
        onConfirm={() => mutation.mutate()}
      />
    </>
  );
}
