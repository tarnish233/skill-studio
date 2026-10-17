import { useEffect, useState, type ReactNode } from "react";
import { motion, useReducedMotion } from "framer-motion";

/**
 * 列表 ⇄ 详情（以及分组卡点开后的编辑面板）用同一段过渡：线性缓动、淡入 + 轻轻上浮。
 *
 * 页面级的 Hub / 分组 / 项目切换有自己的滑动过渡，这里只负责「同一页内点开一条
 * 记录」的那种展开：不动 layout，不延迟切换，退出时直接换（返回时的列表同样淡入）。
 * 系统开了「减少动态效果」就直接上最终状态。
 *
 * 起点写成 0.4 而不是 0：透明原生窗口下动画帧可能被拖后，内容必须第一帧就看得见
 * （和 App 的页面过渡同一个理由），顺带也让测试里没跑动画的详情保持可见。
 *
 * appear 由页面传 useDetailAppear()：页面刚进入的那一帧不播，否则和页面过渡叠在一起，
 * 起点成了 0.4 × 0.4。
 */
export function DetailTransition({
  children,
  appear = true,
}: {
  children: ReactNode;
  appear?: boolean;
}) {
  const reduceMotion = useReducedMotion();
  // 播不播只在挂载时决定（framer-motion 也只在挂载时读 initial）：之后 appear 变化
  // 不影响已经挂载的这一份
  const [animateIn] = useState(() => appear && !reduceMotion);
  return (
    <motion.div
      data-transition="detail"
      // 这次挂载是否播放过渡（测试和排查用）
      data-appear={animateIn ? "" : undefined}
      className="flex min-h-0 min-w-0 flex-1 flex-col"
      initial={animateIn ? { opacity: 0.4, y: 8 } : false}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: animateIn ? 0.2 : 0, ease: "linear" }}
    >
      {children}
    </motion.div>
  );
}

/**
 * 页面第一次渲染时为 false，之后为 true：页面内的切换（点开详情、返回列表）才播放
 * DetailTransition；进入页面时只有页面级过渡。
 */
export function useDetailAppear() {
  const [entered, setEntered] = useState(false);
  useEffect(() => setEntered(true), []);
  return entered;
}
