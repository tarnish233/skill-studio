import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { ListItemRow } from "@/components/common/ListItemRow";

it("能进详情的卡片整卡可点，行尾不再放 › 提示", () => {
  const onPreview = vi.fn();
  render(
    <ListItemRow card onPreview={onPreview} previewLabel="编辑 前端">
      <span>前端</span>
      <button type="button">启用</button>
    </ListItemRow>,
  );
  const card = screen.getByRole("button", { name: "编辑 前端" });
  // 卡片里只有自己的内容：最后一个元素就是行内按钮，后面没有额外的图标
  expect(card.querySelector("svg")).toBeNull();
  expect(card.lastElementChild).toBe(
    screen.getByRole("button", { name: "启用" }),
  );
  // 整卡可点、键盘可开；行内按钮只做就地动作
  fireEvent.click(screen.getByText("前端"));
  fireEvent.keyDown(card, { key: "Enter" });
  expect(onPreview).toHaveBeenCalledTimes(2);
  fireEvent.click(screen.getByRole("button", { name: "启用" }));
  expect(onPreview).toHaveBeenCalledTimes(2);
});
