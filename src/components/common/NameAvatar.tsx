import { cn } from "@/lib/utils";
import { nameAvatarGlyph, nameAvatarTone } from "@/lib/nameAvatar";

/**
 * 列表卡片左侧的标记：按名字生成底色和首字，同一条目永远是同一个记号。
 * 尺寸跟随其他列表卡片的图标槽（40×40、圆角 12）。
 */
export function NameAvatar({
  name,
  className,
}: {
  name: string;
  className?: string;
}) {
  return (
    <div
      aria-hidden
      className={cn(
        "flex h-10 w-10 shrink-0 select-none items-center justify-center rounded-xl",
        nameAvatarTone(name),
        className,
      )}
    >
      <span className="text-[17px] font-semibold leading-none">
        {nameAvatarGlyph(name)}
      </span>
    </div>
  );
}
