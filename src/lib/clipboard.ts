/**
 * 复制文本。桌面应用的 WebView 里 `navigator.clipboard` 不一定可用（自定义协议下可能不算
 * 安全上下文，Linux 的 WebKitGTK 也常常拒绝），这时退回选中临时文本再执行复制命令。
 */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    // 继续走退回方案
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.top = "0";
  area.style.opacity = "0";
  document.body.appendChild(area);
  try {
    area.select();
    if (!document.execCommand("copy")) throw new Error("浏览器拒绝了复制");
  } finally {
    area.remove();
  }
}
