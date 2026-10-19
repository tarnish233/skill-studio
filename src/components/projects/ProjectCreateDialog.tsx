import { useState } from "react";
import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useCreateProject } from "@/hooks/useData";
import { projectsApi } from "@/lib/api";

/** Skill 项目与 MCP 项目共用的「添加项目」：两边是同一份项目列表 */
export function ProjectCreateDialog({
  open,
  onOpenChange,
  description,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  description: string;
}) {
  const create = useCreateProject();
  const [form, setForm] = useState({ name: "", root: "" });
  const [shown, setShown] = useState(false);
  // 每次打开都从空表单开始
  if (open !== shown) {
    setShown(open);
    if (open) setForm({ name: "", root: "" });
  }
  const pick = async () => {
    const root = await projectsApi.pickDirectory();
    if (root)
      setForm((prev) => ({
        root,
        name: prev.name || root.split(/[/\\]/).filter(Boolean).pop() || root,
      }));
  };
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => !create.isPending && onOpenChange(next)}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>添加项目</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4 px-6 py-5">
          <label className="block space-y-2 text-sm">
            <span>项目目录</span>
            <div className="flex gap-2">
              <Input
                aria-label="项目目录"
                value={form.root}
                placeholder="点右侧按钮选择"
                disabled={create.isPending}
                onChange={(e) => setForm({ ...form, root: e.target.value })}
              />
              <Button
                variant="outline"
                size="icon"
                aria-label="选择项目目录"
                title="选择项目目录"
                disabled={create.isPending}
                onClick={() => void pick()}
              >
                <FolderOpen className="h-4 w-4" />
              </Button>
            </div>
          </label>
          <label className="block space-y-2 text-sm">
            <span>项目名</span>
            <Input
              aria-label="项目名"
              value={form.name}
              placeholder="用于在列表里识别"
              disabled={create.isPending}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
            />
          </label>
        </div>
        <DialogFooter>
          <Button
            variant="outline"
            disabled={create.isPending}
            onClick={() => onOpenChange(false)}
          >
            取消
          </Button>
          <Button
            disabled={
              create.isPending || !form.name.trim() || !form.root.trim()
            }
            onClick={() =>
              create.mutate(form, { onSuccess: () => onOpenChange(false) })
            }
          >
            {create.isPending ? "添加中…" : "添加"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
