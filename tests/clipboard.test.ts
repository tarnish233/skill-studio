import { afterEach, describe, expect, it, vi } from "vitest";
import { copyText } from "@/lib/clipboard";

const clipboard = () => {
  const writeText = vi.fn(async () => {});
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
  return writeText;
};

afterEach(() => {
  Reflect.deleteProperty(navigator, "clipboard");
});

describe("copyText", () => {
  it("能直接用剪贴板时用它", async () => {
    const writeText = clipboard();
    await copyText("skill-studio status");
    expect(writeText).toHaveBeenCalledWith("skill-studio status");
  });

  it("剪贴板不可用时退回选中文本再复制，临时节点用完就删", async () => {
    const writeText = vi.fn(async () => {
      throw new Error("不是安全上下文");
    });
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    const exec = vi.fn(() => true);
    Object.defineProperty(document, "execCommand", {
      value: exec,
      configurable: true,
    });
    const before = document.body.childElementCount;
    await copyText("/Applications/Skill Studio.app");
    expect(exec).toHaveBeenCalledWith("copy");
    expect(document.body.childElementCount).toBe(before);
    expect(
      document.querySelector("textarea") === null,
      "临时文本框没有留下",
    ).toBe(true);
  });

  it("浏览器连退回方案也拒绝时报错", async () => {
    Object.defineProperty(document, "execCommand", {
      value: vi.fn(() => false),
      configurable: true,
    });
    await expect(copyText("x")).rejects.toThrow("浏览器拒绝了复制");
    expect(document.querySelector("textarea")).toBeNull();
  });
});
