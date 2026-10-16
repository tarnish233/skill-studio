import { useEffect, useState, type ReactNode } from "react";
import { useQueries } from "@tanstack/react-query";
import { useAgentInventory, useSettings } from "@/hooks/useData";
import { useTarget } from "@/components/targets/TargetProvider";
import { groupsApi, projectsApi, settingsApi, skillsApi } from "@/lib/api";
import { managementApi } from "@/lib/api/mcpManagement";
import { invoke, requestRemote } from "@/lib/api/transport";
import { initialResource, readInitialView } from "@/lib/initialView";
import { NAVIGATION_STALE_TIME, queryKeys } from "@/lib/queryKeys";
import { SkillStudioIcon } from "./SkillStudioIcon";
import { Button } from "@/components/ui/button";

/** Only coordinate the first frame of this target, never background refreshes. */
export function StartupBoundary({ children }: { children: ReactNode }) {
  const target = useTarget();
  const settings = useSettings();
  const agents = useAgentInventory();
  const [opened, setOpened] = useState(false);
  const view = readInitialView(settings.data, target.id);
  const mcp = initialResource(settings.data) === "mcp";
  const definitions: {
    queryKey: readonly string[];
    queryFn: () => Promise<unknown>;
    staleTime?: number;
  }[] = [];
  if (settings.data && !opened) {
    if (mcp) {
      definitions.push(
        target.id === "local"
          ? { queryKey: ["mcp", "local"], queryFn: managementApi.list }
          : {
              queryKey: ["mcp", "inventory", target.id],
              queryFn: () => requestRemote(target.id, "scan_mcp"),
            },
      );
    } else {
      definitions.push({
        queryKey: queryKeys.skillBackups,
        queryFn: () => invoke("list_skill_backups"),
      });
      definitions.push({
        queryKey: queryKeys.skills,
        queryFn: skillsApi.scan,
        staleTime: 20_000,
      });
      if (view === "projects" || view === "groups") {
        definitions.push({
          queryKey: queryKeys.groups,
          queryFn: groupsApi.list,
        });
      }
      // 分组卡片的启用状态来自配置里的 activeGroups
      if (view === "groups") {
        definitions.push({
          queryKey: queryKeys.config,
          queryFn: settingsApi.getConfig,
        });
      }
    }
    if (view === "projects")
      definitions.push({
        queryKey: queryKeys.projects,
        queryFn: projectsApi.list,
      });
  }
  const page = useQueries({
    queries: definitions.map((query) => ({
      staleTime: NAVIGATION_STALE_TIME,
      ...query,
    })),
  });
  const required = [settings, agents, ...page];
  const errors = required.filter(
    (query) => query.isError && query.data === undefined,
  );
  const ready = required.every((query) => query.data !== undefined);
  useEffect(() => {
    if (ready) setOpened(true);
  }, [ready]);

  if (opened || ready) return children;
  return (
    <div
      className="flex h-screen flex-col bg-background text-foreground"
      aria-busy={!errors.length}
    >
      <div data-tauri-drag-region className="h-9 shrink-0" />
      <div className="flex flex-1 flex-col items-center justify-center gap-4 px-6">
        <SkillStudioIcon className="h-8 w-8" />
        {errors.length ? (
          <div role="alert" className="max-w-lg space-y-3 text-center text-sm">
            <p>启动数据加载失败</p>
            <p className="break-words text-muted-foreground">
              {String(errors[0].error)}
            </p>
            <div className="flex justify-center gap-2">
              <Button
                onClick={() => {
                  for (const query of errors) void query.refetch();
                }}
              >
                重试
              </Button>
              <Button variant="outline" onClick={() => setOpened(true)}>
                继续打开应用
              </Button>
            </div>
          </div>
        ) : (
          <div
            role="status"
            aria-label="正在准备工作区"
            className="text-center text-sm text-muted-foreground"
          >
            正在准备工作区…
          </div>
        )}
      </div>
    </div>
  );
}
