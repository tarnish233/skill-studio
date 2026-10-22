import { useQuery } from "@tanstack/react-query";
import { Copy, SquareTerminal } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { SettingCard } from "@/components/common/SettingCard";
import { systemApi } from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import { isWindows } from "@/lib/platform";
import { queryKeys } from "@/lib/queryKeys";

async function copy(text: string, done: string) {
  try {
    await copyText(text);
    toast.success(done);
  } catch (error) {
    toast.error(`复制失败：${String(error)}`);
  }
}

const quoted = (path: string) => `"${path.replace(/(["\\$`])/g, "\\$1")}"`;

/**
 * 设置页「命令行工具」：终端和 Agent 能不能直接运行 skill-studio。
 * 还没放进 PATH 时给出 App 自带程序的位置和链接命令；这个版本没有附带命令行时不显示。
 */
export function CommandLineCard() {
  const { data: cli } = useQuery({
    queryKey: queryKeys.cliInfo,
    queryFn: () => systemApi.getCliInfo(),
    staleTime: 60_000,
  });
  if (!cli?.bundled) return null;
  if (cli.command) {
    const example = `${cli.command} status`;
    return (
      <SettingCard
        title="命令行工具"
        icon={<SquareTerminal />}
        description={
          <>
            <span className="block">
              终端和 Agent 可以直接运行 {cli.command}：{example} 查看总览，
              {cli.command} --help 查看全部命令。改动会实时出现在这里。
            </span>
            <span className="mt-1 block break-all font-mono text-[11px]">
              {cli.location}
            </span>
          </>
        }
      >
        <Button
          variant="outline"
          size="sm"
          onClick={() => void copy(example, "已复制示例命令")}
        >
          <Copy className="h-5 w-5" />
          复制示例命令
        </Button>
      </SettingCard>
    );
  }
  const windows = isWindows();
  if (!cli.stable) {
    // AppImage 运行时 App 在临时挂载点里，链接过去重启就失效
    return (
      <SettingCard
        title="命令行工具"
        icon={<SquareTerminal />}
        description="当前是从 AppImage 运行的，程序在临时挂载点里，不能长期链接。用 deb 包安装，或用 Homebrew 安装 Skill Studio 就会装好 skill-studio 命令。"
      />
    );
  }
  const line = windows
    ? cli.bundled
    : `ln -s ${quoted(cli.bundled)} /usr/local/bin/skill-studio`;
  return (
    <SettingCard
      title="命令行工具"
      icon={<SquareTerminal />}
      description={
        windows
          ? "App 自带命令行程序，还没放进 PATH：把它所在的目录加入 PATH，终端和 Agent 就能运行。"
          : "App 自带命令行程序，还没放进 PATH。用 Homebrew 安装或升级 Skill Studio 会自动链接为 skill-studio；也可以运行下面的命令手动链接。"
      }
      details={
        <div className="flex items-center gap-2">
          <code
            className="min-w-0 flex-1 select-text truncate rounded-md border border-border-default bg-background px-3 py-2 font-mono text-xs"
            title={line}
          >
            {line}
          </code>
          <Button
            variant="outline"
            size="sm"
            onClick={() =>
              void copy(line, windows ? "已复制程序位置" : "已复制链接命令")
            }
          >
            <Copy className="h-5 w-5" />
            复制
          </Button>
        </div>
      }
    />
  );
}
