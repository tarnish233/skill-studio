import { useMemo, useState, type ReactNode } from "react";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { ListContainer, ListItemRow } from "@/components/common/ListItemRow";

export interface ChecklistItem {
  id: string;
  name: string;
  /** 名称下的一行说明 */
  detail?: string;
  /** 整行的悬停提示，如 skill 的路径 */
  title?: string;
  /** 名称后的标记，如来源 */
  marks?: ReactNode;
  /** 右侧的数量徽标，如分组的成员数 */
  badge?: string;
  /** 不能选的原因；已选中的仍可取消 */
  disabledReason?: string;
}

/**
 * 项目详情与分组编辑里的成员清单（绑定分组、选择 skill / MCP）：点一行即勾选或取消。
 * `pinned` 里的（打开详情时已选的）排在最前，勾选过程中顺序不跳动。
 */
export function MemberChecklist({
  title,
  items,
  selected,
  pinned = [],
  onToggle,
  disabled,
  searchPlaceholder,
  searchLabel,
  emptyText,
}: {
  title: string;
  items: ChecklistItem[];
  selected: string[];
  pinned?: string[];
  onToggle: (id: string) => void;
  disabled?: boolean;
  /** 有就显示搜索框 */
  searchPlaceholder?: string;
  /** 搜索框的无障碍名称，默认同占位文字 */
  searchLabel?: string;
  emptyText: string;
}) {
  const [query, setQuery] = useState("");
  const ordered = useMemo(
    () => [
      ...pinned.flatMap((id) => items.filter((i) => i.id === id)),
      ...items.filter((i) => !pinned.includes(i.id)),
    ],
    [items, pinned],
  );
  const keyword = query.trim().toLowerCase();
  const shown = ordered.filter((i) =>
    `${i.name} ${i.detail ?? ""}`.toLowerCase().includes(keyword),
  );
  return (
    <section className="space-y-2">
      <h3 className="text-sm font-semibold">
        {title}{" "}
        <span className="font-normal text-muted-foreground">
          · 已选 {selected.length} 个
        </span>
      </h3>
      {searchPlaceholder && (
        <Input
          aria-label={searchLabel ?? searchPlaceholder}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={searchPlaceholder}
        />
      )}
      {!shown.length ? (
        <p className="py-4 text-sm text-muted-foreground">
          {items.length ? "没有匹配的项" : emptyText}
        </p>
      ) : (
        <ListContainer>
          {shown.map((item, i) => {
            const checked = selected.includes(item.id);
            const blocked = !!item.disabledReason && !checked;
            return (
              <ListItemRow
                key={item.id}
                isLast={i === shown.length - 1}
                onClick={() => !disabled && !blocked && onToggle(item.id)}
                className={blocked ? "cursor-not-allowed" : undefined}
              >
                <Checkbox
                  checked={checked}
                  disabled={disabled || blocked}
                  aria-label={item.name}
                />
                <div className="min-w-0 flex-1" title={item.title}>
                  <div className="flex min-w-0 items-center gap-2">
                    <p className="truncate text-sm font-medium">{item.name}</p>
                    {item.marks}
                  </div>
                  {item.detail && (
                    <p className="truncate text-xs text-muted-foreground">
                      {item.detail}
                    </p>
                  )}
                  {item.disabledReason && (
                    <p className="text-xs text-amber-600 dark:text-amber-400">
                      {item.disabledReason}
                    </p>
                  )}
                </div>
                {item.badge && (
                  <Badge variant="outline" className="h-4 px-1.5 text-[10px]">
                    {item.badge}
                  </Badge>
                )}
              </ListItemRow>
            );
          })}
        </ListContainer>
      )}
    </section>
  );
}
