import { afterEach, expect, it } from "vitest";

// floating-ui 每次定位浮层都会调用 element.matches(":popover-open" / ":modal")。
// nwsapi 2.2.27 匹配这类状态伪类时会再调 node.matches，在 jsdom 里等于调回自己，
// 曾经一路递归到栈溢出才返回 false：每打开一个提示框要卡几秒，CI 上几十秒。
const original = Element.prototype.matches;
afterEach(() => {
  Element.prototype.matches = original;
});

const countCalls = (run: () => boolean) => {
  let calls = 0;
  Element.prototype.matches = function (this: Element, selector: string) {
    calls += 1;
    return original.call(this, selector);
  };
  const result = run();
  Element.prototype.matches = original;
  return { result, calls };
};

it.each([":modal", ":fullscreen", ":open", ":closed", ":picture-in-picture"])(
  "%s 在 jsdom 里直接得出结果，不会反复调回 matches",
  (selector) => {
    const el = document.createElement("div");
    const { result, calls } = countCalls(() => el.matches(selector));
    expect(result).toBe(false);
    expect(calls).toBeLessThan(5);
  },
);

it("带 popover 属性的元素查 :popover-open 也不会反复调回 matches", () => {
  const el = document.createElement("div");
  el.setAttribute("popover", "");
  const { result, calls } = countCalls(() => el.matches(":popover-open"));
  expect(result).toBe(false);
  expect(calls).toBeLessThan(5);
});

it("nwsapi 自己能判断的状态照旧：打开的 details 匹配 :open", () => {
  const details = document.createElement("details");
  details.open = true;
  expect(details.matches(":open")).toBe(true);
  expect(details.matches(":closed")).toBe(false);
  details.open = false;
  expect(details.matches(":closed")).toBe(true);
  expect(document.createElement("div").matches("div:not(:modal)")).toBe(true);
});
