import { createContext, useContext, useEffect, useRef } from "react";

/**
 * 再点一次当前所在的导航项时递增：页面回到列表（关掉页内展开的详情），搜索和筛选保留。
 * App 先过一遍未保存修改的确认，再递增。
 */
const Context = createContext(0);
export const PageResetProvider = Context.Provider;

/** 页内会展开详情的页面登记：导航项被再次点击时调用 `reset` */
export function usePageReset(reset: () => void) {
  const count = useContext(Context);
  const latest = useRef(reset);
  latest.current = reset;
  // 挂载时的值不算：只响应挂载之后的点击
  const initial = useRef(count);
  useEffect(() => {
    if (count !== initial.current) latest.current();
  }, [count]);
}
