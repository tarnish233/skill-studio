const HIDDEN = "<已隐藏>";

/** MCP 卡片、标题和选择器只展示安全的连接摘要；完整地址只留在编辑字段中。 */
export function redactMcpUrl(url: string): string {
  const fragmentAt = url.indexOf("#");
  const withoutFragment = fragmentAt < 0 ? url : url.slice(0, fragmentAt);
  const queryAt = withoutFragment.indexOf("?");
  let base = queryAt < 0 ? withoutFragment : withoutFragment.slice(0, queryAt);
  const query = queryAt < 0 ? null : withoutFragment.slice(queryAt + 1);

  const schemeAt = base.indexOf("://");
  if (schemeAt >= 0) {
    const authorityStart = schemeAt + 3;
    const slash = base.indexOf("/", authorityStart);
    const authorityEnd = slash < 0 ? base.length : slash;
    const at = base.lastIndexOf("@", authorityEnd - 1);
    if (at >= authorityStart) {
      base = base.slice(0, authorityStart) + HIDDEN + base.slice(at);
    }
  }

  const safeQuery = query?.split("&").map((pair) => {
    const equals = pair.indexOf("=");
    return equals > 0
      ? `${pair.slice(0, equals)}=${HIDDEN}`
      : pair
        ? HIDDEN
        : "";
  });
  return (
    base +
    (safeQuery ? `?${safeQuery.join("&")}` : "") +
    (fragmentAt >= 0 ? `#${HIDDEN}` : "")
  );
}

export function mcpTarget(definition: {
  url?: unknown;
  command?: unknown;
}): string {
  if (typeof definition.url === "string") return redactMcpUrl(definition.url);
  const command = definition.command;
  // Some Agent configs store command + args in one array. Only the executable is
  // safe to show by default; later elements can include --token and its value.
  if (Array.isArray(command)) return String(command[0] ?? "");
  return typeof command === "string" ? command : "";
}
