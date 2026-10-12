import { describe, expect, it } from "vitest";
import { canonical } from "@/lib/api/mcpManagement";

describe("Pi native MCP state", () => {
  it("uses enabled even when a legacy adapter flag disagrees", () => {
    for (const enabled of [true, false]) {
      expect(
        canonical({ command: "demo", enabled, disabled: enabled }, "pi")
          .enabled,
      ).toBe(enabled);
    }
    expect(
      canonical({ command: "demo", disabled: true }, "pi").enabled,
    ).toBeUndefined();
  });
});
