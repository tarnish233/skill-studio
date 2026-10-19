import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArchiveRestore, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { useSettings } from "@/hooks/useData";
import { mcpRequest } from "@/lib/api/mcp";
import type { McpBackupList, McpBackupView } from "@/lib/api/mcpManagement";
import { formatBytes } from "@/lib/format";
import { queryKeys } from "@/lib/queryKeys";
import { cn } from "@/lib/utils";

const STATUS = {
  restorable: {
    label: "可恢复",
    className: "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
  },
  afterNewer: {
    label: "需先恢复更新的备份",
    className: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
  },
  unrestorable: {
    label: "无法恢复",
    className: "bg-muted text-muted-foreground",
  },
} as const;

type Pending =
  | { kind: "restore"; backup: McpBackupView }
  /** cutsOff：更早、本来还能恢复的备份，要先撤销这次才能恢复 */
  | { kind: "delete"; backup: McpBackupView; cutsOff: boolean }
  | { kind: "unrestorable"; count: number }
  | { kind: "all"; count: number }
  | { kind: "legacy"; count: number; size: number };

const when = (backup: McpBackupView) =>
  new Date(backup.createdAt * 1000).toLocaleString();

function confirmCopy(pending: Pending | null): {
  title: string;
  description: string;
  confirmText: string;
  /** 默认是 destructive */
  variant?: "info";
} {
  const untouched = "配置文件本身不受影响。";
  switch (pending?.kind) {
    case "restore":
      return {
        title: "恢复 MCP 配置备份？",
        description:
          "将配置文件及对应托管状态恢复到备份前。若之后的配置或托管状态已修改，操作会停止。",
        confirmText: "恢复",
        variant: "info",
      };
    case "delete":
      return {
        title: "删除这次备份？",
        description:
          pending.backup.status === "unrestorable"
            ? `${when(pending.backup)} 的这次备份已无法恢复，删除只清理备份文件。${untouched}`
            : `删除后不能再撤销 ${when(pending.backup)} 的这次改动${pending.cutsOff ? "，更早的备份也可能因此无法恢复" : ""}。${untouched}`,
        confirmText: "删除",
      };
    case "unrestorable":
      return {
        title: `删除无法恢复的 ${pending.count} 次备份？`,
        description: `删除时会按当时的配置重新判断，仍能恢复的备份会保留。${untouched}`,
        confirmText: "删除",
      };
    case "all":
      return {
        title: `删除全部 ${pending.count} 次备份？`,
        description: `删除后不能再撤销之前的 MCP 改动。${untouched}`,
        confirmText: "全部删除",
      };
    case "legacy":
      return {
        title: `删除 ${pending.count} 个旧备份文件？`,
        description: `这些文件共 ${formatBytes(pending.size)}，是旧版本写在配置文件旁边的，不在任何备份记录里，无法从这里恢复，删除后也无法找回。`,
        confirmText: "删除",
      };
    default:
      return { title: "", description: "", confirmText: "确认" };
  }
}

/** MCP Hub「已备份 N 次」：每次写入 Agent 配置前的备份，标出能否恢复，可单条或批量删除。 */
export function McpBackupsDialog({
  open,
  onOpenChange,
  onChanged,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 恢复或删除之后刷新 MCP 状态（按钮上的次数） */
  onChanged: () => unknown;
}) {
  const { data: settings } = useSettings();
  const qc = useQueryClient();
  const list = useQuery({
    queryKey: queryKeys.mcpBackups,
    queryFn: () => mcpRequest<McpBackupList>("listBackups"),
    enabled: open,
  });
  const [pending, setPending] = useState<Pending | null>(null);
  const [busy, setBusy] = useState(false);
  const backups = list.data?.backups ?? [];
  const legacy = list.data?.legacy ?? [];
  const count = (status: McpBackupView["status"]) =>
    backups.filter((backup) => backup.status === status).length;
  const unrestorable = count("unrestorable");
  const total = backups.reduce((sum, backup) => sum + backup.size, 0);
  const legacySize = legacy.reduce((sum, file) => sum + file.size, 0);
  const copy = confirmCopy(pending);
  const keep = settings?.backupKeep;
  // 旧文件按所在目录分组，只列文件名：同一目录下的长文件名不用反复折行
  const legacyDirs = [
    ...legacy
      .reduce((dirs, file) => {
        const cut = Math.max(
          file.path.lastIndexOf("/"),
          file.path.lastIndexOf("\\"),
        );
        const dir = file.path.slice(0, cut + 1);
        dirs.set(dir, [...(dirs.get(dir) ?? []), file.path.slice(cut + 1)]);
        return dirs;
      }, new Map<string, string[]>())
      .entries(),
  ];

  async function confirm() {
    if (!pending) return;
    setBusy(true);
    try {
      if (pending.kind === "restore") {
        await mcpRequest("restoreBackup", { id: pending.backup.id });
        toast.success("配置已恢复，请在 Agent 中重新加载 MCP");
      } else if (pending.kind === "legacy") {
        const { removed } = await mcpRequest<{ removed: number }>(
          "deleteLegacyBackups",
        );
        toast.success(`已删除 ${removed} 个旧备份文件`);
      } else {
        const { removed } = await mcpRequest<{ removed: number }>(
          "deleteBackups",
          pending.kind === "delete"
            ? { ids: [pending.backup.id] }
            : { which: pending.kind },
        );
        toast.success(`已删除 ${removed} 次备份`);
      }
      setPending(null);
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
      await Promise.all([
        qc.invalidateQueries({ queryKey: queryKeys.mcpBackups }),
        onChanged(),
      ]);
    }
  }

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="max-w-xl">
          <DialogHeader>
            <DialogTitle>MCP 配置备份</DialogTitle>
            <DialogDescription>
              {list.isPending
                ? "读取中…"
                : `共 ${backups.length} 次 · 可恢复 ${count("restorable")} 次 · 占用 ${formatBytes(total)}`}
            </DialogDescription>
          </DialogHeader>
          <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-6 py-4">
            <p className="text-xs text-muted-foreground">
              写入 Agent 配置前自动备份，最多保留 {keep ?? "-"} 次（在「设置 ›
              维护」调整），超出时先删无法恢复的。备份后配置或托管状态又被改过的，就无法恢复了。
              {keep !== undefined && backups.length > keep && (
                <span className="font-medium text-foreground">
                  {" "}
                  现在多出 {backups.length - keep} 次，下次写入配置时自动清理。
                </span>
              )}
            </p>
            {list.error && (
              <p role="alert" className="text-sm text-red-600">
                {String(list.error)}
              </p>
            )}
            {legacy.length > 0 && (
              <section
                aria-label="旧版本留下的备份文件"
                className="rounded-xl border border-dashed border-border-default p-3"
              >
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-sm font-medium">
                    旧版本留下的备份文件
                  </span>
                  <span className="text-xs tabular-nums text-muted-foreground">
                    {legacy.length} 个 · {formatBytes(legacySize)}
                  </span>
                  <Button
                    size="sm"
                    variant="outline"
                    className="ml-auto"
                    disabled={busy}
                    onClick={() =>
                      setPending({
                        kind: "legacy",
                        count: legacy.length,
                        size: legacySize,
                      })
                    }
                  >
                    删除这些文件
                  </Button>
                </div>
                <p className="mt-1 text-xs text-muted-foreground">
                  写在配置文件旁边，不在任何备份记录里，无法从这里恢复。
                </p>
                <details className="mt-2 text-xs text-muted-foreground">
                  <summary className="cursor-pointer select-none">
                    查看文件
                  </summary>
                  <div className="mt-1.5 space-y-1.5 font-mono text-[11px]">
                    {legacyDirs.map(([dir, names]) => (
                      <div key={dir}>
                        <p className="break-all text-foreground/80">{dir}</p>
                        <ul className="pl-3">
                          {names.map((name) => (
                            <li key={name} className="break-all">
                              {name}
                            </li>
                          ))}
                        </ul>
                      </div>
                    ))}
                  </div>
                </details>
              </section>
            )}
            {!list.isPending && !list.error && !backups.length && (
              <p className="text-sm text-muted-foreground">暂无备份</p>
            )}
            {backups.map((backup, index) => {
              const status = STATUS[backup.status];
              return (
                <div
                  key={backup.id}
                  className="flex items-start gap-3 rounded-xl border border-border-default p-3"
                >
                  <div className="min-w-0 flex-1 space-y-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="text-sm font-medium tabular-nums">
                        {when(backup)}
                      </span>
                      <span
                        className={cn(
                          "rounded-full px-2 py-0.5 text-xs font-medium",
                          status.className,
                        )}
                      >
                        {status.label}
                      </span>
                      <span className="text-xs tabular-nums text-muted-foreground">
                        {formatBytes(backup.size)}
                      </span>
                    </div>
                    <div className="text-xs text-muted-foreground">
                      {backup.paths.map((path) => (
                        <p key={path} className="break-words">
                          {path}
                        </p>
                      ))}
                    </div>
                    {backup.status === "unrestorable" && backup.reason && (
                      <p className="break-all text-xs text-muted-foreground">
                        {backup.reason}
                      </p>
                    )}
                  </div>
                  <div className="flex shrink-0 gap-1.5">
                    {backup.status === "restorable" && (
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={busy}
                        onClick={() => setPending({ kind: "restore", backup })}
                      >
                        <ArchiveRestore className="h-4 w-4" /> 恢复
                      </Button>
                    )}
                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={busy}
                      aria-label={`删除 ${when(backup)} 的备份`}
                      onClick={() =>
                        setPending({
                          kind: "delete",
                          backup,
                          // 无法恢复的那次不在任何撤销链上，删了不影响别的
                          cutsOff:
                            backup.status !== "unrestorable" &&
                            backups
                              .slice(index + 1)
                              .some((b) => b.status !== "unrestorable"),
                        })
                      }
                    >
                      <Trash2 className="h-4 w-4" /> 删除
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>
          {backups.length > 0 && (
            <DialogFooter className="py-3 sm:justify-between">
              {unrestorable > 0 ? (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  onClick={() =>
                    setPending({ kind: "unrestorable", count: unrestorable })
                  }
                >
                  删除无法恢复的 {unrestorable} 次
                </Button>
              ) : (
                <span />
              )}
              <Button
                size="sm"
                variant="ghost"
                className="text-red-600 hover:text-red-600 dark:text-red-400"
                disabled={busy}
                onClick={() =>
                  setPending({ kind: "all", count: backups.length })
                }
              >
                全部删除
              </Button>
            </DialogFooter>
          )}
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        open={!!pending}
        onOpenChange={(next) => !next && !busy && setPending(null)}
        title={copy.title}
        description={copy.description}
        confirmText={copy.confirmText}
        variant={copy.variant ?? "destructive"}
        pending={busy}
        onConfirm={() => void confirm()}
      />
    </>
  );
}
