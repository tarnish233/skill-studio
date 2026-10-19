import { useState, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";

/** 编辑弹窗里可选的一个成员 */
export interface MemberOption {
  id: string;
  name: string;
  /** 名称下的一行说明 */
  detail?: string;
  /** 整行的悬停提示，如 skill 的路径 */
  title?: string;
  /** 名称后的来源标记 */
  marks?: ReactNode;
  /** 不能选作成员的原因 */
  disabledReason?: string;
}

export interface GroupDraft {
  /** 新建时为 null */
  id: string | null;
  name: string;
  memberIds: string[];
  /** 打开弹窗时已有的成员：排在列表最前，勾选过程中不跟着跳动 */
  pinned: string[];
}

/**
 * Skill 与 MCP 分组共用的编辑弹窗：名称、成员（已选的排在最前）、已缺失的成员、错误。
 * 保存只记录成员；是否对某个 Agent 生效在卡片上单独决定。
 */
export function GroupEditor({
  draft,
  onChange,
  onCancel,
  onSave,
  pending,
  saveDisabled,
  error,
  noun,
  hub,
  searchLabel,
  options,
  emptyHub,
  note,
}: {
  draft: GroupDraft | null;
  onChange: (draft: GroupDraft) => void;
  onCancel: () => void;
  onSave: () => void;
  pending: boolean;
  /** 数据还没读完、读取失败等：不能保存 */
  saveDisabled?: boolean;
  error: string | null;
  /** 成员的叫法与所在 Hub：「skill」/「Skill Hub」，「MCP」/「MCP Hub」 */
  noun: string;
  hub: string;
  searchLabel: string;
  options: MemberOption[];
  /** Hub 里还没有可选成员时的提示 */
  emptyHub: string;
  /** 补充在说明后面的一句话 */
  note?: string;
}) {
  const [search, setSearch] = useState("");
  const [open, setOpen] = useState(false);
  // 每次打开都从空搜索开始
  if (!!draft !== open) {
    setOpen(!!draft);
    if (!draft) setSearch("");
  }
  const memberIds = draft?.memberIds ?? [];
  const pinned = draft?.pinned ?? [];
  const ordered = [
    ...pinned.flatMap((id) => options.filter((o) => o.id === id)),
    ...options.filter((o) => !pinned.includes(o.id)),
  ];
  const keyword = search.trim().toLowerCase();
  const shown = ordered.filter((o) =>
    `${o.name} ${o.detail ?? ""}`.toLowerCase().includes(keyword),
  );
  const missing = memberIds.filter((id) => !options.some((o) => o.id === id));
  const set = (memberIds: string[]) =>
    draft && onChange({ ...draft, memberIds });
  return (
    <Dialog
      open={!!draft}
      onOpenChange={(open) => {
        if (!open && !pending) onCancel();
      }}
    >
      <DialogContent className="max-h-[85vh] overflow-hidden sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{draft?.id ? "编辑分组" : "新建分组"}</DialogTitle>
          <DialogDescription>
            选择 {noun} 组合，保存后可在卡片上为需要的 Agent 启用。
            {note}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-3 overflow-y-auto px-6 py-4">
          <p className="text-sm font-medium">分组名称</p>
          <Input
            aria-label="分组名称"
            placeholder="例如：前端开发"
            value={draft?.name ?? ""}
            disabled={pending}
            onChange={(e) =>
              draft && onChange({ ...draft, name: e.target.value })
            }
          />
          <p className="text-sm font-medium">
            选择 {noun}{" "}
            <span className="font-normal text-muted-foreground">
              · 已选 {memberIds.length} 个
            </span>
          </p>
          <Input
            aria-label={searchLabel}
            placeholder={`搜索 ${hub}…`}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <div className="max-h-72 space-y-2 overflow-y-auto">
            {shown.map((option) => {
              const checked = memberIds.includes(option.id);
              const blocked = !!option.disabledReason && !checked;
              return (
                <label
                  key={option.id}
                  title={option.title}
                  className={cn(
                    "flex items-center gap-3 rounded-lg border border-border-default p-3",
                    blocked ? "cursor-not-allowed" : "cursor-pointer",
                  )}
                >
                  <Checkbox
                    aria-label={`选择 ${option.name}`}
                    checked={checked}
                    disabled={pending || blocked}
                    onCheckedChange={(on) =>
                      set(
                        on
                          ? [...memberIds, option.id]
                          : memberIds.filter((id) => id !== option.id),
                      )
                    }
                  />
                  <div
                    className={cn("min-w-0 flex-1", blocked && "opacity-60")}
                  >
                    <div className="flex min-w-0 items-center gap-2">
                      <p className="truncate text-sm font-medium">
                        {option.name}
                      </p>
                      {option.marks}
                    </div>
                    {option.detail && (
                      <p className="truncate text-xs text-muted-foreground">
                        {option.detail}
                      </p>
                    )}
                    {option.disabledReason && (
                      <p className="mt-1 text-xs text-amber-600 dark:text-amber-400">
                        {option.disabledReason}
                      </p>
                    )}
                  </div>
                </label>
              );
            })}
            {missing.map((id) => (
              <label
                key={id}
                className="flex items-center gap-3 px-3 text-sm text-red-600"
              >
                <Checkbox
                  checked
                  aria-label={`移除缺失成员 ${id}`}
                  disabled={pending}
                  onCheckedChange={() => set(memberIds.filter((x) => x !== id))}
                />
                已缺失：{id}
              </label>
            ))}
            {!options.length ? (
              <p className="py-4 text-sm text-muted-foreground">{emptyHub}</p>
            ) : (
              !shown.length && (
                <p className="py-4 text-sm text-muted-foreground">
                  没有匹配的 {noun}
                </p>
              )
            )}
          </div>
          {error && (
            <p role="alert" className="text-sm text-red-600">
              {error}
            </p>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" disabled={pending} onClick={onCancel}>
            取消
          </Button>
          <Button
            disabled={pending || saveDisabled || !draft?.name.trim()}
            onClick={onSave}
          >
            {pending ? "保存中…" : "保存"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
