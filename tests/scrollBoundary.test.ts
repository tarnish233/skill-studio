import { readFileSync } from "node:fs";
import { expect, it } from "vitest";

it("分组详情滚动到末端时不把触控板滚动传给透明的顶层 WebView", () => {
  const css = readFileSync("src/index.css", "utf8");
  const group = readFileSync("src/components/groups/GroupDetail.tsx", "utf8");

  // WKWebView 在透明窗口里移动整张页面，会露出后面的原生灰色底板。
  // 根文档永远不可滚动，只有清单自己的容器负责长内容。
  expect(css).toMatch(
    /html,\s*body,\s*#root\s*\{[^}]*height:\s*100%;[^}]*overflow:\s*hidden;/s,
  );
  expect(group).toMatch(/overflow-y-auto\s+overscroll-y-none/);
});
