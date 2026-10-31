import { fireEvent, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import {
  AgentToggleIcon,
  type AgentToggleItem,
} from "@/components/common/AgentToggleIcon";
import { renderWithProviders } from "./utils/render";
import { TooltipProvider } from "@/components/ui/tooltip";

const item = (over: Partial<AgentToggleItem> = {}): AgentToggleItem => ({
  agentId: "pi",
  label: "Pi 开关",
  hint: "Pi 状态",
  tone: "off",
  onClick: vi.fn(),
  ...over,
});

it("inactive Pi is pale grayscale and does not light up on hover", () => {
  renderWithProviders(<AgentToggleIcon item={item()} busy={false} />);
  const button = screen.getByRole("button", { name: "Pi 开关" });
  const mark = button.querySelector("svg")!;
  expect(mark).toHaveClass("h-4", "w-4", "grayscale", "opacity-40");
  expect(mark.className.baseVal).not.toContain("grayscale-0");
  expect(button).toHaveClass("bg-transparent", "border-transparent");
  expect(button.className).not.toMatch(/emerald/);
});
it("enabled Pi shows its original brand colors without a green background", () => {
  renderWithProviders(
    <AgentToggleIcon item={item({ tone: "on" })} busy={false} />,
  );
  const button = screen.getByRole("button", { name: "Pi 开关" });
  const mark = button.querySelector("svg")!;
  expect(mark).not.toHaveClass("grayscale", "opacity-40");
  expect(button).toHaveClass(
    "text-foreground",
    "bg-transparent",
    "border-transparent",
  );
  expect(button.className).not.toMatch(/emerald/);
  expect(
    [...mark.querySelectorAll("path")].map((path) => path.getAttribute("fill")),
  ).toEqual(["#F09082", "#4D9ABF", "#F1BE58"]);
});
it("enabled monochrome brands use their normal foreground rather than green tint", () => {
  renderWithProviders(
    <AgentToggleIcon
      item={item({ agentId: "codex", tone: "on" })}
      busy={false}
    />,
  );
  const button = screen.getByRole("button", { name: "Pi 开关" });
  expect(button).toHaveClass("text-foreground");
  expect(button.className).not.toMatch(/text-emerald|text-amber/);
  expect(button.querySelector("path")).toHaveAttribute("fill", "currentColor");
});
it("attention adds a reminder dot without changing an enabled logo or tile color", () => {
  renderWithProviders(
    <AgentToggleIcon
      item={item({ tone: "on", attention: true })}
      busy={false}
    />,
  );
  const button = screen.getByRole("button", { name: "Pi 开关" });
  expect(button.querySelector(".bg-amber-500")).toBeInTheDocument();
  expect(button.className).not.toMatch(/bg-amber|text-amber|emerald/);
  expect(button.querySelector("svg")).not.toHaveClass("grayscale");
});
it("unavailable inactive Agents are dimmed further, while an enabled Agent stays lit", () => {
  const view = renderWithProviders(
    <AgentToggleIcon item={item({ dimmed: true })} busy={false} />,
  );
  expect(
    screen.getByRole("button", { name: "Pi 开关" }).querySelector("svg"),
  ).toHaveClass("opacity-25");
  view.rerender(
    <TooltipProvider>
      <AgentToggleIcon item={item({ tone: "on", dimmed: true })} busy={false} />
    </TooltipProvider>,
  );
  expect(
    screen.getByRole("button", { name: "Pi 开关" }).querySelector("svg"),
  ).not.toHaveClass("grayscale", "opacity-25");
});
it("keeps the original click and busy behavior", () => {
  const onClick = vi.fn();
  const view = renderWithProviders(
    <AgentToggleIcon item={item({ onClick })} busy={false} />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Pi 开关" }));
  expect(onClick).toHaveBeenCalledTimes(1);
  view.rerender(
    <TooltipProvider>
      <AgentToggleIcon item={item({ onClick })} busy />
    </TooltipProvider>,
  );
  expect(screen.getByRole("button", { name: "Pi 开关" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Pi 开关" }));
  expect(onClick).toHaveBeenCalledTimes(1);
});
