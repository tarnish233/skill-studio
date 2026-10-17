import { readFileSync } from "node:fs";
import path from "node:path";
import { render, screen, waitFor } from "@testing-library/react";
import { expect, it } from "vitest";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";

const read = (file: string) =>
  readFileSync(path.resolve(process.cwd(), file), "utf8");

/**
 * 弹窗和列表 ⇄ 详情的过渡是线性缓动的，而且改「原生材质窗口下不播放动画」这条
 * 规则时必须只关掉退场：入场动画本身没有副作用，退场卡住才会留下残影。
 */
it("弹窗入场动画是线性的，起点保持可见", () => {
  const css = read("src/index.css");
  const dialog = read("src/components/ui/dialog.tsx");
  expect(css).toContain("@keyframes ss-dialog-in");
  expect(css).toContain("@keyframes ss-dialog-overlay-in");
  // 起点不能是全透明：动画时钟若被挂起，弹窗会一直看不见
  const keyframes = css.slice(css.indexOf("@keyframes ss-dialog-in"));
  expect(keyframes).toMatch(/from\s*{[^}]*opacity:\s*0\.\d+/);
  expect(dialog).toContain("animate-[ss-dialog-in_180ms_linear]");
});

it("原生材质窗口只停掉退场动画，入场照常播放", () => {
  const css = read("src/index.css");
  const rule = css.slice(css.indexOf("html[data-window-material] [data-slot="));
  const selector = rule.slice(0, rule.indexOf("{"));
  expect(selector).toContain('[data-state="closed"]');
  expect(selector).toContain('[data-slot="dialog-content"]');
  expect(selector).not.toContain("animate-in");
});

it("列表 ⇄ 详情用同一段线性过渡，并跟随系统的减少动态效果", () => {
  const source = read("src/components/common/DetailTransition.tsx");
  expect(source).toContain('ease: "linear"');
  expect(source).toContain("useReducedMotion");
  // 同弹窗：起点保持可见，动画不跑时也不能是一片空白
  expect(source).toContain("opacity: 0.4");
  expect(source).toContain('data-transition="detail"');
});

it("入场动画到点仍没播完（动画时钟停住）就摘掉：弹窗不会停在半透明的起点", async () => {
  const css = read("src/index.css");
  // 只摘入场：退场仍由下面材质窗口那条规则单独处理
  expect(css).toMatch(
    /\[data-slot="dialog-content"\]\[data-state="open"\]\[data-entered\]\s*{\s*animation:\s*none;/,
  );
  render(
    <Dialog open>
      <DialogContent>
        <DialogTitle>标题</DialogTitle>
        <DialogDescription>说明</DialogDescription>
      </DialogContent>
    </Dialog>,
  );
  const dialog = screen.getByRole("dialog");
  const overlay = document.querySelector('[data-slot="dialog-overlay"]');
  // 刚打开时入场动画照常播放
  expect(dialog).not.toHaveAttribute("data-entered");
  await waitFor(
    () => {
      expect(dialog).toHaveAttribute("data-entered");
      expect(overlay).toHaveAttribute("data-entered");
    },
    { timeout: 1000 },
  );
});
