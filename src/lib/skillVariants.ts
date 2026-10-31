export function variantLabel(key: string): string {
  return (
    (
      {
        generic: "通用版",
        "claude-code": "Claude Code 专用版",
        codex: "Codex 专用版",
        opencode: "OpenCode 专用版",
        pi: "Pi 专用版",
        grok: "Grok Build 专用版",
        "deepseek-harness": "DeepSeek Harness 专用版",
      } as Record<string, string>
    )[key] ?? "来源待确认"
  );
}
