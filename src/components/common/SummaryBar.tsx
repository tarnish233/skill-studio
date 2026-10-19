import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type ReactNode,
} from "react";
import { AgentIcon } from "@/components/common/AgentIcon";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

/**
 * Hub、分组、项目页顶部的统计栏，Skill 与 MCP 两种资源共用：
 * - 左侧是以「全部」开头的单选筛选（`SummaryFilter`）。只有一眼认得出的图标
 *   （Agent 品牌、Skill Studio、共享 Agent 目录的小机器人）才用图标代替名称，
 *   其余都写出名称；
 * - 右侧是开关筛选（`SummaryToggle`，如「已托管」，和左侧叠加）、统计（`SummaryStat`）
 *   和打开详情的按钮（`SummaryAction`），都是文字。
 *
 * 数量两侧都写成「名称 数字」，不带「个」「次」「处」这样的单位：单位和统计口径
 * 放进悬停提示，提示以名称开头（「已备份：…共 3 次」）。token 合计是用量，保留「tokens」。
 */
export const summaryPillClass =
  "inline-flex h-7 shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded-full border px-2.5 py-0 text-xs font-medium leading-4";
/**
 * 可点的胶囊。边框必须走主题 token —— 裸 `border` 会吃到 preflight 推出的
 * #e4e4e7，深色下是一枚近白胶囊套在已经变暗的卡片里。
 */
const pressablePillClass = cn(
  summaryPillClass,
  "border-border-default transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50",
);

export function SummaryBar({ children }: { children: ReactNode }) {
  return (
    <div className="my-4 flex h-16 w-full shrink-0 items-center gap-3 rounded-xl border border-border-default px-4">
      {children}
    </div>
  );
}

/** 一个筛选项。数字是选中它时列表里会有几项（分组页是该 Agent 启用的分组数）。 */
export interface SummaryOption {
  id: string;
  /** 名称：读屏名称是「名称 数字」 */
  label: string;
  count: number;
  /**
   * 16px 图标，只给一眼认得出的（Agent 品牌、Skill Studio、共享目录）：有图标时胶囊只显示
   * 图标和数字，名称留给读屏和悬停提示；没有图标就写出名称
   */
  icon?: ReactNode;
  /** 悬停提示：要带上名称，默认就是名称 */
  title?: string;
}

/** Agent 用品牌图标 */
export function agentOption({
  id,
  label,
  count,
  title,
  iconId = id,
}: {
  id: string;
  label: string;
  count: number;
  title?: string;
  /** 图标按 Skill 侧的 Agent id 取；MCP 后端的 id 不同时传这个 */
  iconId?: string;
}): SummaryOption {
  return {
    id,
    label,
    count,
    title,
    icon: <AgentIcon agentId={iconId} size="sm" />,
  };
}

/**
 * 统计栏左侧的筛选：第一枚固定是「全部 N」，其余单选；点已选中的那枚回到「全部」。
 * 选中项不在选项里（例如那个 Agent 刚被关闭管理）时按「全部」显示。
 */
export function SummaryFilter({
  label,
  total,
  allTitle,
  options,
  selected,
  onSelect,
  title,
}: {
  /** 读屏用的分组名称 */
  label: string;
  total: number;
  allTitle?: string;
  options: SummaryOption[];
  selected: string | null;
  onSelect: (id: string | null) => void;
  title?: string;
}) {
  const current = options.some((o) => o.id === selected) ? selected : null;
  const { ref, fade, measure } = useOverflowFade();
  return (
    <div
      ref={ref}
      role="group"
      aria-label={label}
      title={title}
      data-fade={fade}
      onScroll={measure}
      className="summary-filter flex h-9 min-w-0 flex-1 items-center gap-2 overflow-x-auto px-1 [scrollbar-width:none]"
    >
      <FilterPill
        pressed={current === null}
        title={allTitle}
        onClick={() => onSelect(null)}
      >
        全部 {total}
      </FilterPill>
      {options.map((option) => (
        <FilterPill
          key={option.id}
          pressed={current === option.id}
          title={option.title ?? option.label}
          aria-label={`${option.label} ${option.count}`}
          onClick={() => onSelect(current === option.id ? null : option.id)}
        >
          {option.icon ?? `${option.label} `}
          {option.count}
        </FilterPill>
      ))}
    </div>
  );
}

/**
 * 窄窗口里放不下时，记下哪一侧有被裁掉的内容，样式在那一侧渐隐（见 index.css），
 * 不然被切掉一半的胶囊像是坏了。数字和选项变化都会重新渲染，所以每次渲染后、
 * 窗口缩放和滚动时重新量一次就够。
 */
function useOverflowFade() {
  const ref = useRef<HTMLDivElement>(null);
  const [fade, setFade] = useState<"none" | "start" | "end" | "both">("none");
  const measure = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    // 只剩容器的 4px 内边距没露出来时不算被裁掉
    const hidden = {
      start: el.scrollLeft,
      end: el.scrollWidth - el.scrollLeft - el.clientWidth,
    };
    const start = hidden.start > 6;
    const end = hidden.end > 6;
    setFade(start && end ? "both" : start ? "start" : end ? "end" : "none");
  }, []);
  useLayoutEffect(measure);
  useEffect(() => {
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [measure]);
  return { ref, fade, measure };
}

/** 左侧一组里的选项：没选中的淡一些，选中的那枚最醒目 */
function FilterPill({
  pressed,
  className,
  ...props
}: ComponentPropsWithoutRef<"button"> & { pressed: boolean }) {
  return (
    <SummaryToggle
      pressed={pressed}
      className={cn(
        !pressed && "text-muted-foreground hover:text-foreground",
        className,
      )}
      {...props}
    />
  );
}

/** 右侧的开关筛选（如「已托管」）：和左侧的筛选叠加，开着时左侧的数字按它收窄 */
export function SummaryToggle({
  pressed,
  className,
  ...props
}: ComponentPropsWithoutRef<"button"> & { pressed: boolean }) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      className={cn(
        pressablePillClass,
        "text-foreground",
        pressed && "bg-muted ring-1 ring-current",
        className,
      )}
      {...props}
    />
  );
}

export function SummaryEnd({
  className,
  ...props
}: ComponentPropsWithoutRef<"div">) {
  return (
    <div
      className={cn(
        "flex h-9 shrink-0 items-center gap-2 whitespace-nowrap border-l border-border-default pl-3",
        className,
      )}
      {...props}
    />
  );
}

/** 右侧的统计：只读 */
export function SummaryStat({
  className,
  ...props
}: ComponentPropsWithoutRef<"div">) {
  return (
    <Badge
      variant="outline"
      className={cn(summaryPillClass, className)}
      {...props}
    />
  );
}

/** 右侧打开详情的按钮（如备份列表）：和统计同样的胶囊，可点 */
export function SummaryAction({
  className,
  ...props
}: ComponentPropsWithoutRef<"button">) {
  return (
    <button
      type="button"
      className={cn(pressablePillClass, "text-foreground", className)}
      {...props}
    />
  );
}
