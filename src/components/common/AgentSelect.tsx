/**
 * 单 Agent 视角（「已安装 / 未托管」标签）用的 Agent 选择器。
 *
 * 用原生 <select> 而不是 Radix：本项目的 jsdom 测试约定不测 Radix 浮层的展开交互
 * （floating-ui 在 jsdom 里单次展开要 3–50 秒，必然抖），这里只是个单选切换。
 */
export function AgentSelect({
  agents,
  value,
  onChange,
}: {
  agents: { id: string; displayName: string }[];
  value: string;
  onChange: (id: string) => void;
}) {
  return (
    <select
      aria-label="选择 Agent"
      className="h-9 shrink-0 rounded-md border border-border-default bg-background px-2 text-sm"
      value={value}
      onChange={(e) => onChange(e.target.value)}
    >
      {agents.map((a) => (
        <option key={a.id} value={a.id}>
          {a.displayName}
        </option>
      ))}
    </select>
  );
}
