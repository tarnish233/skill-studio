import {
  SummaryBar,
  SummaryEnd,
  SummaryFilter,
  SummaryStat,
  SummaryToggle,
  agentOption,
  type SummaryOption,
} from "@/components/common/SummaryBar";
import { SkillVariantDetails } from "@/components/common/SkillVariantDetails";
import { useSkillPreview } from "@/components/common/SkillPreview";
import {
  SkillBackups,
  DeleteSkillButton,
} from "@/components/common/SkillBackups";
import { SkillStudioIcon } from "@/components/common/SkillStudioIcon";
import { InstallSkillsPage } from "@/pages/InstallSkillsPage";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { PageTools } from "@/components/common/PageTools";
import { useEffect, useMemo, useState } from "react";
import { FolderOpen, PackagePlus, PackageMinus, Link2 } from "lucide-react";
import { AgentIcon } from "@/components/common/AgentIcon";
import {
  SOURCE_LABELS,
  SOURCE_TITLES,
  SourceMark,
  sourceIcon,
} from "@/components/common/SourceMark";
import {
  hubCatalog,
  isSkillVisible,
  hubSourceIds,
  sourceAgentIds,
  type HubCard,
} from "@/lib/hubCatalog";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { EmptyState } from "@/components/common/EmptyState";
import {
  ListContainer,
  ListItemRow,
  RowActions,
} from "@/components/common/ListItemRow";
import {
  useAdoptToHub,
  useAgents,
  useSkills,
  useSettings,
} from "@/hooks/useData";
import { skillsApi, systemApi } from "@/lib/api";
import type { SkillView } from "@/types";
import { formatTokens, sumTokens, tokenIndex, tokenTitle } from "@/lib/tokens";

export function LibraryPage({ onAdd }: { onAdd?: () => void } = {}) {
  const { data: skills = [], isLoading } = useSkills();
  const { data: agents = [] } = useAgents();
  const { data: settings } = useSettings();
  const visibleAgents = agents.filter(
    (a) => !settings?.disabledAgents?.includes(a.id),
  );
  const client = useQueryClient();
  const { previewSkill, previewDialog } = useSkillPreview();
  const [releaseTarget, setReleaseTarget] = useState<SkillView | null>(null);
  /** 左侧筛选：某个来源；null 是「全部」 */
  const [filter, setFilter] = useState<string | null>(null);
  const activeFilter =
    filter && !settings?.disabledAgents?.includes(filter) ? filter : null;
  useEffect(() => {
    if (filter && settings?.disabledAgents?.includes(filter)) setFilter(null);
  }, [filter, settings?.disabledAgents]);
  /** 右侧开关：只看已托管的，和左侧来源叠加 */
  const [hubOnly, setHubOnly] = useState(false);
  const [adding, setAdding] = useState(false);
  const [query, setQuery] = useState("");
  const [adoptTarget, setAdoptTarget] = useState<SkillView | null>(null);
  const [sourceCard, setSourceCard] = useState<HubCard | null>(null);

  /*
   * 托管 / 还原都会改动 skill 的归属，于是它可能从当前筛选里掉出去：文件还在
   * 磁盘上，卡片却整个消失、计数减一，用户没法区分"被筛掉了"和"被删了"。
   * 所以下面两个动作成功后都要把会藏住它的筛选清掉，把结果留在眼前。
   */
  const release = useMutation({
    mutationFn: skillsApi.releaseFromHub,
    onSuccess: () => {
      setReleaseTarget(null);
      setSourceCard(null);
      // 还原后它不再是 Hub 托管，「已托管」会藏住它；来源归属也可能跟着变
      setHubOnly(false);
      setFilter(null);
      toast.success("已移出 Hub 并还原到原始目录");
    },
    onError: (e) => toast.error(String(e)),
    onSettled: () => client.invalidateQueries(),
  });
  const adopt = useAdoptToHub();
  const adoptAndKeepVisible = (skillId: string) =>
    // 托管后它的真身必定在 Hub，「已托管」不会藏它；但来源归属可能变（旧后端
    // 认不出已托管内容的原始来源），所以只清来源筛选。失败了就别动用户的筛选
    adopt.mutate(skillId, { onSuccess: () => setFilter(null) });

  const catalog = useMemo(
    () =>
      hubCatalog(
        skills.filter((skill) =>
          isSkillVisible(skill, settings?.disabledAgents),
        ),
      ),
    [skills, settings?.disabledAgents],
  );
  const selectedSourceCard = sourceCard
    ? (catalog.find((c) => c.key === sourceCard.key) ?? null)
    : null;
  const matches = (card: HubCard) =>
    card.sources.some((s) =>
      `${s.name} ${s.displayName ?? ""} ${s.description ?? ""} ${s.sourcePath}`
        .toLowerCase()
        .includes(query.trim().toLowerCase()),
    );
  const available = catalog.filter((c) => !c.unavailable);
  const tokens = useMemo(() => tokenIndex(skills), [skills]);
  // 与「全部」的数量一致：合并的多来源 skill 只计一次，筛选不改变总量。
  const totalTokens = sumTokens(
    tokens,
    available.map((c) => c.skill.id),
  );
  /** 真身已经搬进 Hub 目录（已托管），与卡片上那枚「托管中」徽标同一判定 */
  const isHubManaged = (card: HubCard) =>
    card.sources.some((s) => s.origin.kind === "hub");
  const hubManaged = available.filter(isHubManaged);
  // 左侧的数字必须和列表同一口径：开着「已托管」时，一枚写着 3 的胶囊
  // 点下去只剩 0 条，等于刚承诺完就打自己的脸
  const counted = hubOnly ? hubManaged : available;
  const passes = (card: HubCard, id: string | null) =>
    !id || hubSourceIds(card).includes(id);
  const filtered = counted.filter((c) => matches(c) && passes(c, activeFilter));
  const invalid = catalog.filter(
    (c) => c.unavailable && matches(c) && (!hubOnly || isHubManaged(c)),
  );

  const count = (id: string) => counted.filter((c) => passes(c, id)).length;
  const sourceLabel = (id: string) =>
    SOURCE_LABELS[id] ?? agentName(agents, id);
  const options: SummaryOption[] = [
    ...visibleAgents.map((a) =>
      agentOption({
        id: a.id,
        label: a.displayName,
        count: count(a.id),
        title: `筛选 ${a.displayName} 来源`,
      }),
    ),
    ...(["studio", "builtin", "agent", "unknown", "external"] as const)
      // 待确认与外部来源只在确实有（或正筛着它）时出现
      .filter(
        (id) =>
          id === "studio" ||
          id === "agent" ||
          activeFilter === id ||
          count(id) > 0,
      )
      .map((id) => ({
        id,
        label: SOURCE_LABELS[id],
        icon: sourceIcon(id),
        title: SOURCE_TITLES[id],
        count: count(id),
      })),
  ];
  const summary = (
    <SummaryBar>
      <SummaryFilter
        label="筛选 skill"
        total={counted.length}
        allTitle={`显示全部 ${counted.length} 个${hubOnly ? "已托管" : "已安装"} skill`}
        options={options}
        selected={activeFilter}
        onSelect={setFilter}
      />
      <SummaryEnd>
        <SummaryToggle
          pressed={hubOnly}
          onClick={() => setHubOnly((v) => !v)}
          title={`已托管：真身已搬进 Hub 目录集中托管的 skill，共 ${hubManaged.length} 个（其余仍在各 Agent 原处）。点击只看这些，可与左侧的来源叠加。`}
        >
          已托管 {hubManaged.length}
        </SummaryToggle>
        <SkillBackups scope="hub" />
        <SummaryStat
          title={tokenTitle(
            totalTokens,
            "当前显示的已安装 skill（多来源去重）",
          )}
        >
          合计 ≈ {formatTokens(totalTokens.total)} tokens
        </SummaryStat>
      </SummaryEnd>
    </SummaryBar>
  );

  const tools = (
    <>
      <PageTools
        query={query}
        onQueryChange={setQuery}
        placeholder="按名称或描述搜索…"
        createLabel="新增 skill"
        onCreate={onAdd ?? (() => setAdding(true))}
      />
    </>
  );
  if (adding) return <InstallSkillsPage onBack={() => setAdding(false)} />;
  if (isLoading) {
    return (
      <div className="space-y-3 py-6">
        {tools}
        <p
          role="status"
          className="py-8 text-center text-sm text-muted-foreground"
        >
          正在加载 skill…
        </p>
      </div>
    );
  }

  if (skills.length === 0) {
    return (
      <>
        {tools}
        {summary}
        <EmptyState
          icon={SkillStudioIcon}
          title="Skill Hub 还没有发现 skill"
          description={`已扫描各 agent 的全局 skill 目录与 Hub，都是空的。在 ${
            agents[0]?.globalSkillDirs[0] ?? "~/.claude/skills"
          } 下建一个含 SKILL.md 的文件夹就会出现在这里。`}
        />
      </>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {tools}
      {summary}

      {previewDialog}
      <div className="min-h-0 flex-1 overflow-y-auto pb-6">
        <ListContainer cards>
          {filtered.map((card) => {
            const skill = card.skill;
            const origins = hubSourceIds(card).filter(
              (id) => !settings?.disabledAgents?.includes(id),
            );
            const rowTokens = sumTokens(tokens, [skill.id]);
            return (
              <ListItemRow
                key={card.key}
                card
                onPreview={() => previewSkill(skill.name, skill.sourcePath)}
                previewLabel={`预览 ${skill.name}`}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="truncate text-sm font-medium">
                      {skill.name}
                    </span>
                    {origins.map((id) => {
                      const title = `${sourceLabel(id)}\n${card.sources
                        .filter((s) =>
                          hubSourceIds({ ...card, sources: [s] }).includes(id),
                        )
                        .map((s) => s.sourcePath)
                        .join("\n")}`;
                      // 与顶部筛选同一规则：认得出的用图标，其余写出名称
                      return (
                        <SourceMark
                          key={id}
                          id={id}
                          label={sourceLabel(id)}
                          title={title}
                        />
                      );
                    })}
                    <Badge
                      variant="outline"
                      title={`${tokenTitle(rowTokens, skill.name)} 整个目录文本合计 ≈ ${formatTokens(rowTokens.total)} tokens。`}
                    >
                      ≈ {formatTokens(rowTokens.skillMd)} tokens
                    </Badge>
                    {isHubManaged(card) && (
                      <Badge
                        variant="outline"
                        className="h-4 border-blue-200 bg-blue-50 px-1.5 text-[10px] text-blue-600 dark:border-blue-800 dark:bg-blue-950/50 dark:text-blue-400"
                      >
                        托管中
                      </Badge>
                    )}
                    {!hubSourceIds(card).length && !isHubManaged(card) && (
                      <Badge variant="outline">外部来源</Badge>
                    )}
                    {card.sources.length > 1 && (
                      <button
                        className="text-xs text-muted-foreground hover:text-foreground"
                        onClick={() => setSourceCard(card)}
                      >
                        {card.sources.length} 个来源
                      </button>
                    )}
                    {catalog.filter(
                      (c) => !c.unavailable && c.skill.name === skill.name,
                    ).length > 1 && (
                      <Badge variant="warning">同名 · 内容不同</Badge>
                    )}
                    {skill.malformedFrontmatter && (
                      <span
                        className="text-xs text-red-600"
                        title={skill.frontmatterError ?? undefined}
                      >
                        YAML 格式错误
                      </span>
                    )}
                    {skill.diagnostics?.map((message) => (
                      <span key={message} className="text-xs text-red-600">
                        {message}
                      </span>
                    ))}
                    <SkillVariantDetails skill={skill} />
                    {skill.frontmatterExtra.length > 0 && (
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <span className="inline-flex cursor-default items-center rounded-md bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
                            扩展字段
                          </span>
                        </TooltipTrigger>
                        <TooltipContent>
                          <p className="font-medium">
                            包含扩展 frontmatter 字段
                          </p>
                          <p className="pt-0.5 font-mono text-[10px]">
                            {skill.frontmatterExtra.join(", ")}
                          </p>
                          <p className="pt-1 text-muted-foreground">
                            不同 Agent 对这些字段的支持可能不同。此提示不表示
                            Skill 正文不可用，也不代表已安装完整插件。
                          </p>
                        </TooltipContent>
                      </Tooltip>
                    )}
                  </div>
                  {skill.description && (
                    <p className="truncate pt-0.5 text-xs text-muted-foreground">
                      {skill.description}
                    </p>
                  )}
                </div>

                <RowActions>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8"
                    title={
                      card.sources.length > 1 ||
                      skill.origin.kind === "hub" ||
                      !!skill.provenance
                        ? "查看来源与托管记录"
                        : "打开所在目录"
                    }
                    aria-label={
                      card.sources.length > 1 ||
                      skill.origin.kind === "hub" ||
                      !!skill.provenance
                        ? `查看 ${skill.name} 的来源`
                        : `打开 ${skill.name} 所在目录`
                    }
                    onClick={() =>
                      card.sources.length > 1 ||
                      skill.origin.kind === "hub" ||
                      !!skill.provenance
                        ? setSourceCard(card)
                        : void systemApi.revealPath(skill.sourcePath)
                    }
                  >
                    <FolderOpen className="h-4 w-4" />
                  </Button>
                  {card.sources.length === 1 &&
                    skill.origin.kind === "hub" &&
                    skill.provenance && (
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8"
                        title="移出 Hub 并还原"
                        aria-label={`还原 ${skill.name} 到原位置`}
                        disabled={release.isPending}
                        onClick={() => setReleaseTarget(skill)}
                      >
                        <PackageMinus className="h-4 w-4" />
                      </Button>
                    )}
                  {card.sources.length === 1 &&
                    skill.origin.kind === "inPlace" && (
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8"
                        title="托管到 Hub"
                        aria-label={`托管 ${skill.name} 到 Hub`}
                        disabled={adopt.isPending}
                        onClick={() => setAdoptTarget(skill)}
                      >
                        <PackagePlus className="h-4 w-4" />
                      </Button>
                    )}
                  {card.sources.map(
                    (source) =>
                      source.origin.kind !== "external" && (
                        <DeleteSkillButton
                          key={source.id}
                          scope={
                            source.origin.kind === "hub"
                              ? "hub"
                              : `agent:${source.origin.ownerAgent}`
                          }
                          path={source.sourcePath}
                          name={source.name}
                        />
                      ),
                  )}
                </RowActions>
              </ListItemRow>
            );
          })}
        </ListContainer>
        {!filtered.length && (
          <p className="py-6 text-center text-sm text-muted-foreground">
            {query || activeFilter || hubOnly
              ? "没有匹配的可用 skill"
              : !catalog.length && skills.length
                ? "当前 Agent 选择下没有可显示的 skill，可在设置中开启其他 Agent。"
                : "暂无可用 skill"}
          </p>
        )}
        {invalid.length > 0 && (
          <details className="mt-5 rounded-xl border border-amber-500/30 bg-amber-500/5 p-4">
            <summary className="cursor-pointer text-sm font-medium">
              失效来源（{invalid.length}）
            </summary>
            <p className="mt-2 text-xs text-muted-foreground">
              这些入口暂时不可用，已保留原链接。恢复目标目录后会重新扫描显示。
            </p>
            <div className="mt-3 space-y-3">
              {invalid.map((card) => (
                <div
                  key={card.key}
                  className="flex items-center gap-3 rounded-lg border bg-background p-3"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium">
                        {card.skill.name}
                      </span>
                      {sourceAgentIds(card.skill)
                        .filter((id) => !settings?.disabledAgents?.includes(id))
                        .map((id) => (
                          <span
                            key={id}
                            role="img"
                            aria-label={`来源：${agentName(agents, id)}`}
                            title={agentName(agents, id)}
                          >
                            <AgentIcon agentId={id} size="sm" />
                          </span>
                        ))}
                    </div>
                    <p className="mt-1 text-xs text-amber-700 dark:text-amber-400">
                      {card.skill.diagnostics?.join("；")}
                    </p>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setSourceCard(card)}
                  >
                    <Link2 className="h-4 w-4" />
                    查看来源
                  </Button>
                </div>
              ))}
            </div>
          </details>
        )}
      </div>

      <Dialog
        open={selectedSourceCard !== null}
        onOpenChange={(open) => {
          if (!open) setSourceCard(null);
        }}
      >
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{selectedSourceCard?.skill.name} · 来源</DialogTitle>
            <DialogDescription>
              {selectedSourceCard?.unavailable
                ? "查看失效入口与目标路径；不会自动删除链接。"
                : "相同内容合并展示，各来源文件与分组引用保持独立。"}
            </DialogDescription>
          </DialogHeader>
          <div className="min-h-0 space-y-3 overflow-y-auto px-6 py-4">
            {selectedSourceCard?.sources.map((source) => (
              <div key={source.id} className="rounded-xl border p-4">
                <div className="mb-2 flex items-center gap-2">
                  {sourceAgentIds(source).map((id) => (
                    <span
                      key={id}
                      className="inline-flex items-center gap-1 text-xs"
                    >
                      <AgentIcon agentId={id} size="sm" />
                      {agentName(agents, id)}
                    </span>
                  ))}
                  {source.origin.kind === "hub" && (
                    <Badge
                      variant="outline"
                      className="border-blue-200 bg-blue-50 text-blue-600 dark:border-blue-800 dark:bg-blue-950/50 dark:text-blue-400"
                    >
                      托管中
                    </Badge>
                  )}
                </div>
                <p className="break-all font-mono text-xs">
                  {source.sourcePath}
                </p>
                {sourceAgentIds(source).flatMap((id) =>
                  (source.agents[id]?.entryPaths ?? [])
                    .filter((p) => p !== source.sourcePath)
                    .map((p) => (
                      <p
                        key={`${id}:${p}`}
                        className="mt-2 break-all text-xs text-muted-foreground"
                      >
                        {agentName(agents, id)} 入口：{p}
                      </p>
                    )),
                )}
                {source.installation && (
                  <p className="mt-3 break-all text-xs text-muted-foreground">
                    由 Skill Studio{" "}
                    {source.installation.source.startsWith("local:")
                      ? "导入"
                      : "安装"}{" "}
                    · {source.installation.source.replace(/^local:/, "")} ·{" "}
                    {source.installation.repositoryPath || "/"}
                  </p>
                )}
                {source.provenance && (
                  <div className="mt-3 space-y-2 text-xs text-muted-foreground">
                    <p className="break-all">
                      托管前位置：{source.provenance.originalPath}
                    </p>
                    <p className="break-all">
                      原始备份：{source.provenance.backupPath}
                    </p>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() =>
                        void systemApi.revealPath(source.provenance!.backupPath)
                      }
                    >
                      打开备份目录
                    </Button>
                  </div>
                )}
                {source.origin.kind === "hub" &&
                  !source.provenance &&
                  !source.installation && (
                    <p className="mt-2 text-xs text-amber-700">
                      缺少历史托管记录，无法确认原始来源和还原路径。不会自动迁移或删除。
                    </p>
                  )}
                {source.diagnostics?.map((message) => (
                  <p key={message} className="mt-2 text-xs text-amber-700">
                    {message}
                  </p>
                ))}
                <div className="mt-3 flex justify-end gap-2">
                  {selectedSourceCard.unavailable ? (
                    sourceAgentIds(source).flatMap((id) =>
                      (source.agents[id]?.entryPaths ?? []).map((path) => (
                        <Button
                          key={`${id}:${path}`}
                          variant="outline"
                          size="sm"
                          title={path}
                          onClick={() =>
                            void systemApi.revealPath(
                              path.replace(/[/\\][^/\\]+$/, ""),
                            )
                          }
                        >
                          <FolderOpen className="h-4 w-4" />
                          打开 {agentName(agents, id)} 入口目录
                        </Button>
                      )),
                    )
                  ) : (
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() =>
                        void systemApi.revealPath(source.sourcePath)
                      }
                    >
                      <FolderOpen className="h-4 w-4" />
                      打开来源目录
                    </Button>
                  )}
                  {source.origin.kind === "hub" && source.provenance && (
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={release.isPending}
                      onClick={() => setReleaseTarget(source)}
                    >
                      <PackageMinus className="h-4 w-4" />
                      移出 Hub 并还原
                    </Button>
                  )}
                  {!selectedSourceCard.unavailable &&
                    source.origin.kind === "inPlace" && (
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={adopt.isPending}
                        onClick={() => {
                          setSourceCard(null);
                          setAdoptTarget(source);
                        }}
                      >
                        <PackagePlus className="h-4 w-4" />
                        托管此来源
                      </Button>
                    )}
                </div>
              </div>
            ))}
          </div>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={releaseTarget !== null}
        onOpenChange={(open) => {
          if (!open && !release.isPending) setReleaseTarget(null);
        }}
        variant="info"
        title="移出 Hub 并还原"
        confirmText="还原到原位置"
        pending={release.isPending}
        description={
          <>
            <span>
              将当前 Hub
              内容还原到托管前的位置，保留托管前和本次还原前的完整备份，并更新分组、项目与链接引用。原位置有冲突或副本被修改时会停止，不覆盖文件。
            </span>
            <span className="mt-2 block break-all font-mono text-xs">
              {releaseTarget?.provenance?.originalPath}
            </span>
          </>
        }
        onConfirm={() => {
          if (releaseTarget) release.mutate(releaseTarget.id);
        }}
      />
      <ConfirmDialog
        open={adoptTarget !== null}
        onOpenChange={(o) => !o && setAdoptTarget(null)}
        variant="info"
        title="托管到 Hub"
        confirmText="托管"
        pending={adopt.isPending}
        description={
          <>
            会把 <span className="font-mono">{adoptTarget?.name}</span> 的真身
            移动到 Hub 目录集中托管，原位置按默认方式保留链接或副本，
            <span className="font-medium">该 agent 仍可正常使用</span>。
            移动前会保存完整备份及原始来源，来源统计保持不变，可从 Hub 还原。
            <span className="mt-2 block break-all font-mono text-xs">
              {adoptTarget?.sourcePath}
            </span>
          </>
        }
        onConfirm={() => {
          if (adoptTarget) adoptAndKeepVisible(adoptTarget.id);
          setAdoptTarget(null);
        }}
      />
    </div>
  );
}

function agentName(
  agents: { id: string; displayName: string }[],
  id: string,
): string {
  return agents.find((a) => a.id === id)?.displayName ?? id;
}
