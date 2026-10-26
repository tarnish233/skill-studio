import { describe, expect, it } from "vitest";
import { mcpTarget, redactMcpUrl } from "@/lib/mcpDisplay";

describe("MCP display summaries", () => {
  it("hides URL credentials without changing normal URLs", () => {
    expect(
      redactMcpUrl(
        "https://user:password@example.com/mcp?api_key=token&bare-token#fragment",
      ),
    ).toBe(
      "https://<已隐藏>@example.com/mcp?api_key=<已隐藏>&<已隐藏>#<已隐藏>",
    );
    expect(redactMcpUrl("https://example.com/mcp")).toBe(
      "https://example.com/mcp",
    );
  });

  it("never displays arguments embedded in an Agent's command array", () => {
    expect(mcpTarget({ command: ["npx", "--token", "secret"] })).toBe("npx");
  });
});
