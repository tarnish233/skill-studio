import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { expect, it } from "vitest";

/**
 * 弹窗本身不带内边距：标题区（DialogHeader）自带 px-6 py-5 和一条分隔线，
 * 正文、底部各自补齐（正文 px-6 py-4）。外层再加 p-6 会把标题区连底色一起缩进去，
 * 和正文错开；正文漏了内边距，文字会贴着弹窗边缘或分隔线。
 */
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return path.endsWith(".tsx") && !name.startsWith("__") ? [path] : [];
  });
}
const files = sources("src").map((path) => ({
  path,
  text: readFileSync(path, "utf8"),
}));
const className = (attrs: string) =>
  /className="([^"]*)"/.exec(attrs)?.[1] ?? "";

it("弹窗外层不加内边距，免得标题区缩进、和正文错开", () => {
  const offenders = files.flatMap(({ path, text }) =>
    [...text.matchAll(/<DialogContent\b([^>]*)>/g)]
      .filter((m) => /(^|\s)p[xy]?-\d/.test(className(m[1])))
      .map((m) => `${path}: ${className(m[1])}`),
  );
  expect(offenders).toEqual([]);
});

it("紧跟标题区的正文自带左右与上边距，不贴边、不贴分隔线", () => {
  // 只看普通标签；DialogFooter、SshHostPicker 这类组件各自负责内边距
  const offenders = files.flatMap(({ path, text }) =>
    [...text.matchAll(/<\/DialogHeader>\s*\n\s*<([a-z][a-z0-9]*)\b([^>]*)>/g)]
      .filter((m) => {
        const cls = className(m[2]);
        return !/(^|\s)px-6(\s|$)/.test(cls) || !/(^|\s)(py|pt)-\d/.test(cls);
      })
      .map((m) => `${path}: <${m[1]} class="${className(m[2])}">`),
  );
  expect(offenders).toEqual([]);
});
