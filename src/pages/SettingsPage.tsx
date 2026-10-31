import {
  ServerSettings,
  ServerConnectionSetting,
} from "@/components/targets/TargetProvider";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { SkillStudioIcon } from "@/components/common/SkillStudioIcon";
import { ThemeToggle } from "@/components/common/ThemeToggle";
import { Switch } from "@/components/ui/switch";
import { useEffect, useState } from "react";
import {
  FolderOpen,
  Github,
  ExternalLink,
  History,
  Sparkles,
  Palette,
  SunMoon,
  Layers,
  LayoutGrid,
  HardDrive,
  BookmarkCheck,
  Link2,
  FolderCog,
  Settings2,
  Plug,
  Power,
  SquareTerminal,
  Eye,
  Eraser,
  Archive,
  Info,
  Trash2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { AgentIcon } from "@/components/common/AgentIcon";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { SettingCard, SettingsSection } from "@/components/common/SettingCard";
import { CommandLineCard } from "@/components/common/CommandLineCard";
import { projectsApi, settingsApi, systemApi } from "@/lib/api";
import {
  useAgents,
  useAppVersion,
  useBackups,
  useDeleteBackup,
  useDeleteHubBackups,
  useHubBackups,
  usePrune,
  useRestoreBackup,
  useSettings,
  useUpdateSettings,
} from "@/hooks/useData";
import { formatBytes } from "@/lib/format";
import type { HubBackup, LinkMode } from "@/types";

type SettingsTab =
  "general" | "directories" | "maintenance" | "servers" | "about";

/**
 * 设置页。上方一条分段标签栏、下方内容区。离开设置走侧栏，顶栏不放返回键。
 */
export function SettingsPage() {
  const { data: settings } = useSettings();
  const { data: agents = [] } = useAgents();
  const { data: backups = [] } = useBackups();
  const { data: version } = useAppVersion();
  const update = useUpdateSettings();
  const visibilityUpdate = useUpdateSettings();
  const restore = useRestoreBackup();
  const deleteBackup = useDeleteBackup();
  const hubBackups = useHubBackups();
  const deleteHubBackups = useDeleteHubBackups();
  const prune = usePrune();

  const [hubDraft, setHubDraft] = useState<string | null>(null);
  const [tab, setTab] = useState<SettingsTab>("general");
  const [restoring, setRestoring] = useState<string | null>(null);
  const [deletingBackup, setDeletingBackup] = useState<string | null>(null);
  const [deletingHub, setDeletingHub] = useState<string[] | null>(null);
  const [configDir, setConfigDir] = useState<string>("");

  // 配置目录只需取一次；放在渲染体里会每次渲染都发起调用
  useEffect(() => {
    void settingsApi.getConfigDir().then(setConfigDir);
  }, []);

  if (!settings) return null;

  const pickAgentDir = async (agentId: string) => {
    const picked = await projectsApi.pickDirectory();
    if (!picked) return;
    update.mutate({
      agentDirOverrides: { ...settings.agentDirOverrides, [agentId]: picked },
    });
  };

  const clearAgentDir = (agentId: string) => {
    const next = { ...settings.agentDirOverrides };
    delete next[agentId];
    update.mutate({ agentDirOverrides: next });
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden pt-4">
      <Tabs
        value={tab}
        onValueChange={(v) => setTab(v as SettingsTab)}
        className="flex min-h-0 flex-1 flex-col"
      >
        <TabsList className="glass mb-5 grid w-full grid-cols-5">
          <TabsTrigger value="general">通用</TabsTrigger>
          <TabsTrigger value="directories">目录</TabsTrigger>
          <TabsTrigger value="maintenance">维护</TabsTrigger>
          <TabsTrigger value="servers">服务器</TabsTrigger>
          <TabsTrigger value="about">关于</TabsTrigger>
        </TabsList>

        <div className="-mx-1 min-h-0 flex-1 overflow-y-auto px-1 pb-8 pt-1">
          <TabsContent value="servers" className="space-y-7">
            <ServerConnectionSetting />
            <ServerSettings />
          </TabsContent>
          <TabsContent value="general" className="space-y-7">
            <SettingsSection title="管理的应用" icon={<Settings2 />}>
              <p className="text-xs text-muted-foreground">
                选择要管理的应用。关闭后隐藏其专属 Skill、MCP
                及来源统计，并退出该应用的 Skill 分组管理。Hub
                托管内容和共享内容继续显示。
              </p>
              <div className="flex flex-wrap gap-2 rounded-xl border border-border-default p-2">
                {agents.map((agent) => {
                  const disabled =
                    (visibilityUpdate.isPending
                      ? visibilityUpdate.variables?.disabledAgents
                      : settings.disabledAgents) ?? [];
                  const enabled = !disabled.includes(agent.id);
                  return (
                    <Button
                      key={agent.id}
                      variant={enabled ? "default" : "ghost"}
                      aria-pressed={enabled}
                      aria-disabled={
                        visibilityUpdate.isPending || update.isPending
                      }
                      onClick={() => {
                        if (visibilityUpdate.isPending || update.isPending)
                          return;
                        visibilityUpdate.mutate({
                          disabledAgents: enabled
                            ? [...disabled, agent.id]
                            : disabled.filter((id) => id !== agent.id),
                        });
                      }}
                      className="gap-2"
                      aria-label={agent.displayName}
                      title={`${agent.displayName}${enabled ? "：正在管理" : "：已关闭管理"}`}
                    >
                      <AgentIcon agentId={agent.id} />
                    </Button>
                  );
                })}
              </div>
            </SettingsSection>
            <SettingsSection title="外观" icon={<Palette />}>
              <SettingCard
                title="主题模式"
                description="选择浅色、深色，或跟随系统外观。"
                icon={<SunMoon />}
              >
                <ThemeToggle />
              </SettingCard>
            </SettingsSection>
            <SettingsSection title="MCP" icon={<Plug />}>
              <SettingCard
                title={<label htmlFor="manage-mcp">启用 MCP 管理</label>}
                description="关闭后隐藏 MCP Hub、Agent 分组和项目 MCP 入口，将当前托管的接入恢复为接管前的 Codex、Claude Code 配置。Hub 记录保留，重新开启时会尝试恢复接入；外部修改不会被覆盖。"
                icon={<Power />}
              >
                <Switch
                  id="manage-mcp"
                  checked={settings.manageMcp ?? true}
                  disabled={update.isPending}
                  onCheckedChange={(value) =>
                    update.mutate({ manageMcp: value })
                  }
                />
              </SettingCard>
              {(settings.manageMcp ?? true) && (
                <SettingCard
                  title={
                    <label htmlFor="show-codex-builtin-mcp">
                      显示 Codex App 内置 MCP
                    </label>
                  }
                  description="在 MCP Hub 中显示 Codex App 内置服务。卡片置灰且只读，不支持托管、编辑或删除。"
                  icon={<Eye />}
                >
                  <Switch
                    id="show-codex-builtin-mcp"
                    checked={settings.showCodexBuiltinMcp ?? false}
                    disabled={update.isPending}
                    onCheckedChange={(value) =>
                      update.mutate({ showCodexBuiltinMcp: value })
                    }
                  />
                </SettingCard>
              )}
            </SettingsSection>
            <SettingsSection
              title="命令行与内置 skill"
              icon={<SquareTerminal />}
            >
              <SettingCard
                title={
                  <label htmlFor="cli-skill-enabled">
                    启用 CLI 与内置 skill
                  </label>
                }
                description="仅影响本机。关闭后普通命令行操作会被拒绝，内置 skill 不再自动同步；未修改、未被使用的内置副本会从 Hub 移除。已修改或仍被引用的副本会保留并提示原因。命令行程序仍随安装包保留，远程 SSH 辅助连接不受影响；此开关不是安全隔离。"
                icon={<SquareTerminal />}
                details={
                  settings.cliSkillWarning && (
                    <p
                      role="status"
                      className="text-xs text-amber-700 dark:text-amber-400"
                    >
                      {settings.cliSkillWarning}
                    </p>
                  )
                }
              >
                <Switch
                  id="cli-skill-enabled"
                  checked={settings.cliSkillEnabled ?? true}
                  disabled={update.isPending}
                  onCheckedChange={(value) =>
                    update.mutate({ cliSkillEnabled: value })
                  }
                />
              </SettingCard>
            </SettingsSection>
            <SettingsSection title="分组与安装" icon={<LayoutGrid />}>
              <SettingCard
                title={
                  <label htmlFor="preserve-manual">
                    是否保留手动安装的 skill
                  </label>
                }
                description="开启后保留手动安装的 skill；关闭后仅当前启用分组中的 skill 生效，组外 skill 暂时停用，文件保留。没有启用分组时同样生效。重新开启只恢复由此策略停用的 skill；退出应用管理时恢复原状态。此策略仅适用于已接入原生启停的 Claude Code、Codex 和 Pi（含共享目录 ~/.agents/skills 里的 skill）；OpenCode、Grok Build 的手动 Skill 始终保留。"
                icon={<BookmarkCheck />}
              >
                <Switch
                  id="preserve-manual"
                  checked={settings.preserveManualSkills ?? true}
                  disabled={update.isPending}
                  onCheckedChange={(value) =>
                    update.mutate({ preserveManualSkills: value })
                  }
                />
              </SettingCard>
              <SettingCard
                title="默认链接方式"
                description="用于 Agent 安装。自动模式优先使用软链，失败时回退为复制；项目使用各自的链接设置。"
                icon={<Link2 />}
                details={
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    软链共享 Hub
                    中的文件，修改后同步生效。复制生成独立文件，源文件更新后需要重新写入。
                  </p>
                }
              >
                <Select
                  value={settings.defaultLinkMode}
                  disabled={update.isPending}
                  onValueChange={(v) =>
                    update.mutate({ defaultLinkMode: v as LinkMode })
                  }
                >
                  <SelectTrigger className="w-44" aria-label="默认链接方式">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="auto">自动（优先软链）</SelectItem>
                    <SelectItem value="symlink">符号链接</SelectItem>
                    <SelectItem value="copy">文件复制</SelectItem>
                  </SelectContent>
                </Select>
              </SettingCard>
            </SettingsSection>
          </TabsContent>

          <TabsContent value="directories" className="space-y-7">
            <SettingsSection title="Skill Studio 存储" icon={<HardDrive />}>
              <SettingCard
                title="Hub 目录"
                icon={<Layers />}
                description="安装、导入和托管的 skill 存放在这里。留空使用默认目录，不能与 Agent 的 skills 目录重叠。目录变更不会迁移文件，当前 Hub 非空时不能切换。"
                details={
                  <div className="flex items-center gap-2">
                    <Input
                      aria-label="Hub 目录"
                      value={hubDraft ?? settings.hubDir ?? ""}
                      placeholder="~/.skill-studio/skills（默认）"
                      onChange={(e) => setHubDraft(e.target.value)}
                    />
                    <Button
                      size="sm"
                      disabled={update.isPending || hubDraft === null}
                      onClick={() =>
                        update.mutate(
                          hubDraft?.trim()
                            ? { hubDir: hubDraft.trim() }
                            : { clearHubDir: true },
                          { onSuccess: () => setHubDraft(null) },
                        )
                      }
                    >
                      保存目录
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={update.isPending}
                      onClick={async () => {
                        const picked = await projectsApi.pickDirectory();
                        if (picked) setHubDraft(picked);
                      }}
                    >
                      <FolderOpen className="h-5 w-5" />
                      选择目录
                    </Button>
                  </div>
                }
              />
              <SettingCard
                title="配置目录"
                icon={<Settings2 />}
                description={
                  <>
                    <span className="block">
                      保存分组、项目、来源记录和配置备份。
                    </span>
                    <span className="mt-1 block break-all font-mono text-[11px]">
                      {configDir || "正在读取…"}
                    </span>
                  </>
                }
              >
                <Button
                  variant="outline"
                  size="sm"
                  disabled={!configDir}
                  onClick={() => void systemApi.revealPath(configDir)}
                >
                  <FolderOpen className="h-5 w-5" />
                  打开
                </Button>
              </SettingCard>
            </SettingsSection>
            <SettingsSection title="Agent 目录覆盖" icon={<FolderCog />}>
              <p className="text-xs leading-relaxed text-muted-foreground">
                默认使用环境变量或标准目录。若 Agent
                安装在其他位置，可在此指定配置目录。
              </p>
              {agents.map((a) => {
                const override = settings.agentDirOverrides[a.id];
                return (
                  <SettingCard
                    key={a.id}
                    icon={<AgentIcon agentId={a.id} />}
                    title={
                      <span className="flex items-center gap-2">
                        {a.displayName}
                        <Badge variant="outline" className="text-[10px]">
                          {override ? "自定义" : "默认"}
                        </Badge>
                      </span>
                    }
                    description={
                      <span className="break-all font-mono text-[11px]">
                        {override ?? a.configDir}
                      </span>
                    }
                  >
                    {override && (
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={update.isPending}
                        onClick={() => clearAgentDir(a.id)}
                      >
                        恢复默认
                      </Button>
                    )}
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={update.isPending}
                      onClick={() => void pickAgentDir(a.id)}
                    >
                      <FolderOpen className="h-5 w-5" />
                      选择目录
                    </Button>
                  </SettingCard>
                );
              })}
            </SettingsSection>
          </TabsContent>

          <TabsContent value="maintenance" className="space-y-7">
            <SettingsSection title="数据维护" icon={<Sparkles />}>
              <SettingCard
                title="清理失效引用"
                icon={<Eraser />}
                description="清理已不存在的 skill 在分组和注册记录中留下的引用。"
              >
                <Button
                  variant="outline"
                  size="sm"
                  disabled={prune.isPending}
                  onClick={() => prune.mutate()}
                >
                  {prune.isPending ? "清理中…" : "清理"}
                </Button>
              </SettingCard>
            </SettingsSection>
            <SettingsSection title="配置备份" icon={<History />}>
              <SettingCard
                title="配置备份保留份数"
                icon={<Archive />}
                description="写入前自动备份 Skill Studio 配置和 MCP 配置，各最多保留这么多份；超出时先删已无法恢复的，再删最旧的。设为 0 则不保留备份。托管完整备份不受此限制。"
              >
                <Input
                  aria-label="配置备份保留份数"
                  type="number"
                  min={0}
                  max={100}
                  value={settings.backupKeep}
                  className="w-20 text-center"
                  onChange={(e) => {
                    const n = Number(e.target.value);
                    if (
                      e.target.value.trim() &&
                      Number.isInteger(n) &&
                      n >= 0 &&
                      n <= 100
                    )
                      update.mutate({ backupKeep: n });
                  }}
                />
              </SettingCard>
              <div className="overflow-hidden rounded-xl border border-border-default bg-card">
                <div className="flex items-center justify-between gap-3 border-b border-border/60 px-5 py-4">
                  <span className="text-sm font-medium">可恢复的备份</span>
                  <Badge variant="outline">{backups.length} 份</Badge>
                </div>
                {backups.length === 0 ? (
                  <p className="px-5 py-8 text-center text-xs text-muted-foreground">
                    暂无备份。配置更新后会自动生成。
                  </p>
                ) : (
                  <div className="divide-y divide-border/60">
                    {backups.slice(0, 10).map((path) => (
                      <div
                        key={path}
                        className="flex items-center gap-3 px-5 py-3"
                      >
                        <History className="h-5 w-5 shrink-0 text-muted-foreground" />
                        <p
                          title={path}
                          className="min-w-0 flex-1 truncate font-mono text-xs"
                        >
                          {path.split(/[/\\]/).pop()}
                        </p>
                        <Button
                          variant="outline"
                          size="sm"
                          disabled={restore.isPending}
                          onClick={() => setRestoring(path)}
                        >
                          恢复
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          aria-label={`删除 ${path.split(/[/\\]/).pop()}`}
                          disabled={deleteBackup.isPending}
                          onClick={() => setDeletingBackup(path)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    ))}
                  </div>
                )}
                {backups.length > 10 && (
                  <p className="border-t px-5 py-3 text-xs text-muted-foreground">
                    显示最近 10 份备份。
                  </p>
                )}
              </div>
              <HubBackupsCard
                backups={hubBackups.data ?? []}
                error={hubBackups.error}
                pending={deleteHubBackups.isPending}
                onDelete={(ids) => setDeletingHub(ids)}
              />
            </SettingsSection>
          </TabsContent>

          <TabsContent value="about" className="space-y-7">
            <SettingsSection title="关于应用" icon={<Info />}>
              <div className="flex flex-wrap items-center justify-between gap-6 rounded-xl border border-border-default bg-card p-6">
                <div className="flex min-w-0 items-center gap-4">
                  <SkillStudioIcon className="h-12 w-12 shrink-0" />
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2.5">
                      <h3 className="text-lg font-semibold text-blue-500">
                        Skill Studio
                      </h3>
                      {version && (
                        <Badge
                          variant="outline"
                          className="font-normal text-muted-foreground"
                        >
                          版本 v{version}
                        </Badge>
                      )}
                    </div>
                    <p className="mt-1 text-sm leading-relaxed text-muted-foreground">
                      集中管理你的 skill，为不同 Agent 组合所需能力。
                    </p>
                  </div>
                </div>
                <div className="ml-auto flex shrink-0 items-center gap-2">
                  {[
                    {
                      label: "GitHub",
                      url: "https://github.com/tarnish233/skill-studio",
                      icon: Github,
                    },
                    {
                      label: "更新日志",
                      url: "https://github.com/tarnish233/skill-studio/releases",
                      icon: ExternalLink,
                    },
                  ].map(({ label, url, icon: Icon }) => (
                    <Button key={label} variant="outline" asChild>
                      <a
                        href={url}
                        target="_blank"
                        rel="noopener noreferrer"
                        onClick={(event) => {
                          event.preventDefault();
                          void openUrl(url).catch((error: unknown) =>
                            toast.error(`打开链接失败：${String(error)}`),
                          );
                        }}
                      >
                        <Icon className="h-5 w-5" />
                        {label}
                      </a>
                    </Button>
                  ))}
                </div>
              </div>
              <SettingCard
                title="Skill Hub"
                icon={<Layers />}
                description="汇总、安装和导入 skill，保留来源信息，统一管理文件。"
              />
              <SettingCard
                title="Agent 与项目"
                icon={<FolderCog />}
                description="在 Agent 中切换 skill 分组，为项目选择独立的 skill 组合。手动安装的 skill 按通用设置保留或暂时停用。"
              />
              <CommandLineCard enabled={settings.cliSkillEnabled ?? true} />
            </SettingsSection>
          </TabsContent>
        </div>
      </Tabs>

      <ConfirmDialog
        open={restoring !== null}
        onOpenChange={(o) => !o && setRestoring(null)}
        variant="info"
        title="从备份恢复配置"
        confirmText="恢复"
        pending={restore.isPending}
        description={
          <>
            会用这份备份替换当前的分组、项目与注册记录。
            <span className="font-medium">
              恢复前会先把当前状态另存一份备份
            </span>
            ，所以这一步可以再退回来。已注册的文件不受影响。
          </>
        }
        onConfirm={() => {
          if (restoring) restore.mutate(restoring);
          setRestoring(null);
        }}
      />
      <ConfirmDialog
        open={deletingBackup !== null}
        onOpenChange={(o) => !o && setDeletingBackup(null)}
        title="删除这份配置备份？"
        confirmText="删除"
        pending={deleteBackup.isPending}
        description={
          <>
            删除后不能再恢复到这份备份。当前配置不受影响。
            <span className="block break-all pt-2 font-mono text-xs">
              {deletingBackup?.split(/[/\\]/).pop()}
            </span>
          </>
        }
        onConfirm={() => {
          if (deletingBackup) deleteBackup.mutate(deletingBackup);
          setDeletingBackup(null);
        }}
      />
      <ConfirmDialog
        open={deletingHub !== null}
        onOpenChange={(o) => !o && setDeletingHub(null)}
        title={
          deletingHub?.length === 1
            ? "删除这份完整备份？"
            : `删除未使用的 ${deletingHub?.length ?? 0} 份完整备份？`
        }
        confirmText="删除"
        pending={deleteHubBackups.isPending}
        description="这些是纳入托管、移出 Hub 时留下的历史快照，当前没有被引用；删除后无法找回。仍被引用的那份会保留。"
        onConfirm={() => {
          if (deletingHub) deleteHubBackups.mutate(deletingHub);
          setDeletingHub(null);
        }}
      />
    </div>
  );
}

/** 纳入托管、移出 Hub 时保存的完整备份。被来源记录引用的那份要用来还原，不能删。 */
function HubBackupsCard({
  backups,
  error,
  pending,
  onDelete,
}: {
  backups: HubBackup[];
  /** 恢复模式下后端拒绝列出：判断不了哪份仍被引用 */
  error: unknown;
  pending: boolean;
  onDelete: (ids: string[]) => void;
}) {
  const unused = backups.filter((backup) => !backup.inUse);
  const total = backups.reduce((sum, backup) => sum + backup.size, 0);
  return (
    <div className="overflow-hidden rounded-xl border border-border-default bg-card">
      <div className="flex items-center justify-between gap-3 border-b border-border/60 px-5 py-4">
        <div className="min-w-0">
          <span className="text-sm font-medium">托管完整备份</span>
          <p className="mt-0.5 text-xs text-muted-foreground">
            纳入托管、移出 Hub 时保存的 skill
            全部文件。被引用的那份在退出应用管理、移出 Hub
            时用来还原，不能删除。
          </p>
        </div>
        {!error && (
          <Badge variant="outline" className="shrink-0">
            {backups.length} 份 · {formatBytes(total)}
          </Badge>
        )}
      </div>
      {error ? (
        <p className="px-5 py-8 text-center text-xs text-muted-foreground">
          无法读取完整备份：{String(error)}
        </p>
      ) : backups.length === 0 ? (
        <p className="px-5 py-8 text-center text-xs text-muted-foreground">
          暂无完整备份。纳入托管时会自动生成。
        </p>
      ) : (
        <div className="divide-y divide-border/60">
          {backups.map((backup) => (
            <div key={backup.id} className="flex items-center gap-3 px-5 py-3">
              <Archive className="h-5 w-5 shrink-0 text-muted-foreground" />
              <div className="min-w-0 flex-1">
                <p className="flex flex-wrap items-center gap-x-2 text-sm">
                  <span className="font-medium">{backup.name}</span>
                  <span className="text-xs text-muted-foreground">
                    {backup.operation === "adopt"
                      ? "纳入托管时"
                      : backup.operation === "release"
                        ? "移出 Hub 时"
                        : "托管时"}
                    {" · "}
                    {new Date(backup.createdAt * 1000).toLocaleString()}
                    {" · "}
                    {formatBytes(backup.size)}
                  </span>
                </p>
                <p
                  title={backup.originalPath}
                  className="truncate font-mono text-xs text-muted-foreground"
                >
                  {backup.originalPath}
                </p>
              </div>
              {backup.inUse ? (
                <Badge
                  variant="outline"
                  className="shrink-0"
                  title="退出应用管理、移出 Hub 时用它还原"
                >
                  使用中
                </Badge>
              ) : (
                <Button
                  variant="ghost"
                  size="sm"
                  aria-label={`删除 ${backup.name} 的完整备份`}
                  disabled={pending}
                  onClick={() => onDelete([backup.id])}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              )}
            </div>
          ))}
        </div>
      )}
      {unused.length > 0 && (
        <div className="flex justify-end border-t border-border/60 px-5 py-3">
          <Button
            variant="outline"
            size="sm"
            disabled={pending}
            onClick={() => onDelete(unused.map((backup) => backup.id))}
          >
            删除未使用的 {unused.length} 份
          </Button>
        </div>
      )}
    </div>
  );
}
