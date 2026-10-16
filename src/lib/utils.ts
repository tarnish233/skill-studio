import { type ClassValue, clsx } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

/**
 * tailwind-merge 只认识它内置的那套值，`rounded-xl-inner`（同心圆角，见
 * tailwind.config.cjs）不在其中 —— 不登记的话 `cn("rounded-md", "rounded-xl-inner")`
 * 会把两个类都留下，最终谁生效取决于 CSS 里的先后顺序，等于把结果交给运气。
 * 这里把它并入 border-radius 组，后写的就能确定性地覆盖前面的。
 */
const twMerge = extendTailwindMerge({
  extend: { classGroups: { rounded: [{ rounded: ["xl-inner"] }] } },
});

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** 两组 id 是否为同一组成员：与顺序、重复无关（分组部署结果只取决于成员集合） */
export function sameMembers(a: readonly string[], b: readonly string[]) {
  const left = new Set(a);
  const right = new Set(b);
  return left.size === right.size && [...right].every((id) => left.has(id));
}
