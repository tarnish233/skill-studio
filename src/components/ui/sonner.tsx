import { Toaster as Sonner } from "sonner";
import { useTheme } from "@/components/theme-provider";

/**
 * 全局提示：顶部居中、带状态配色。sonner 不认 <html> 上的 dark 类，要把应用的主题
 * 传给它，否则深色模式下提示框仍是浅色。「跟随系统」原样传入，sonner 用同一条
 * prefers-color-scheme 查询解析并跟着系统变化。
 */
export function Toaster() {
  const { theme } = useTheme();
  return (
    <Sonner theme={theme} position="top-center" richColors duration={2000} />
  );
}
