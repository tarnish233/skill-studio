import type { ReactNode } from "react";
import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  sortableKeyboardCoordinates,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { LayoutGrid, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/common/EmptyState";
import { ListContainer } from "@/components/common/ListItemRow";
import { PageTools } from "@/components/common/PageTools";

/**
 * Skill 与 MCP 分组页共用的骨架：工具栏、统计栏、读取中 / 读取失败 / 没有分组的状态，
 * 以及可拖拽排序的卡片列表。卡片由页面按各自的数据渲染（见 `GroupCard`）。
 */
export function GroupBoard({
  query,
  onQueryChange,
  onCreate,
  summary,
  loading,
  error,
  ids,
  visible,
  reorderDisabled,
  onReorder,
  noun,
  hub,
  children,
}: {
  query: string;
  onQueryChange: (value: string) => void;
  onCreate: () => void;
  /** 顶部统计栏（`GroupAgentFilter`） */
  summary: ReactNode;
  loading: boolean;
  error?: unknown;
  /** 全部分组按顺序的 id，拖拽排序用 */
  ids: string[];
  /** 搜索后还剩几张卡片 */
  visible: number;
  reorderDisabled: boolean;
  onReorder: (ids: string[]) => void;
  /** 成员的叫法与所在 Hub：「skill」/「Skill Hub」，「MCP」/「MCP Hub」 */
  noun: string;
  hub: string;
  children: ReactNode;
}) {
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );
  const onDragEnd = ({ active, over }: DragEndEvent) => {
    if (!over || active.id === over.id || reorderDisabled) return;
    const from = ids.indexOf(String(active.id));
    const to = ids.indexOf(String(over.id));
    if (from < 0 || to < 0) return;
    onReorder(arrayMove(ids, from, to));
  };
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageTools
        query={query}
        onQueryChange={onQueryChange}
        placeholder="搜索分组…"
        createLabel="新建分组"
        onCreate={onCreate}
      />
      {summary}
      <div className="min-h-0 flex-1 overflow-y-auto pb-6">
        {error ? (
          <p role="alert" className="mb-3 text-sm text-red-600">
            读取分组失败：{String(error)}
          </p>
        ) : loading ? (
          <p className="p-8 text-sm text-muted-foreground">正在读取分组…</p>
        ) : !ids.length ? (
          <EmptyState
            icon={LayoutGrid}
            title="还没有分组"
            description={`新建一个分组，从 ${hub} 选择需要的 ${noun}，再在卡片上为需要的 Agent 启用。`}
            action={
              <Button onClick={onCreate}>
                <Plus className="h-4 w-4" />
                新建分组
              </Button>
            }
          />
        ) : (
          !visible && (
            <p className="p-8 text-center text-sm text-muted-foreground">
              没有匹配的分组
            </p>
          )
        )}
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={onDragEnd}
        >
          <SortableContext items={ids} strategy={verticalListSortingStrategy}>
            <ListContainer cards>{children}</ListContainer>
          </SortableContext>
        </DndContext>
      </div>
    </div>
  );
}
