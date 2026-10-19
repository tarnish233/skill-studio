import { useEffect } from "react";
import { useTarget } from "@/components/targets/TargetProvider";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  agentsApi,
  groupsApi,
  projectsApi,
  settingsApi,
  skillsApi,
  systemApi,
} from "@/lib/api";
import { queryKeys, NAVIGATION_STALE_TIME } from "@/lib/queryKeys";
import { toastLinkReport } from "@/lib/linkReport";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import type { LinkMode, SettingsPatch } from "@/types";

/* ─────────────────── 查询 ─────────────────── */

export function useAgentInventory() {
  return useQuery({
    queryKey: queryKeys.agents,
    queryFn: () => agentsApi.list(true),
    staleTime: 60_000,
  });
}

export function useAgents() {
  const inventory = useAgentInventory();
  const probe = useQuery({
    queryKey: queryKeys.agentProbe,
    queryFn: () => agentsApi.list(false),
    enabled: inventory.data !== undefined,
    // Version detection enriches the inventory; it never blocks navigation.
    staleTime: 60_000,
  });
  const details = new Map(probe.data?.map((agent) => [agent.id, agent]));
  return {
    ...inventory,
    data: inventory.data?.map((agent) => {
      const detail = details.get(agent.id);
      return detail
        ? {
            ...agent,
            detected: agent.detected || detail.detected,
            cliAvailable: detail.cliAvailable,
            cliBroken: detail.cliBroken,
            cliVersion: detail.cliVersion,
          }
        : agent;
    }),
    isDetecting: probe.data === undefined && !probe.isError,
    detectionFailed: probe.isError,
  };
}

export function useSkills() {
  return useQuery({
    queryKey: queryKeys.skills,
    queryFn: () => skillsApi.scan(),
    // scan_skills 在后台线程遍历全库并占用配置锁；全局默认的
    // staleTime: 0 + refetchOnWindowFocus 会让纯导航和切回窗口都白扫一次。
    // 敢留这个窗口是因为失效不靠过期驱动：watcher 的 skills-changed、写操作后的
    // invalidate、手动刷新都是 invalidateQueries，对活跃查询立即重取、不看 staleTime。
    staleTime: 20_000,
  });
}

export function useGroups() {
  return useQuery({
    queryKey: queryKeys.groups,
    queryFn: () => groupsApi.list(),
    staleTime: NAVIGATION_STALE_TIME,
  });
}

export function useProjects() {
  return useQuery({
    queryKey: queryKeys.projects,
    queryFn: () => projectsApi.list(),
    staleTime: NAVIGATION_STALE_TIME,
  });
}

export function useSettings() {
  return useQuery({
    queryKey: queryKeys.settings,
    queryFn: () => settingsApi.get(),
    staleTime: NAVIGATION_STALE_TIME,
  });
}

export function useBackups() {
  return useQuery({
    queryKey: queryKeys.backups,
    queryFn: () => settingsApi.listBackups(),
  });
}

/** 删除一份 Skill Studio 配置备份 */
export function useDeleteBackup() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (path: string) => settingsApi.deleteBackup(path),
    onSuccess: () => toast.success("配置备份已删除"),
    onError: (e: unknown) => toast.error(String(e)),
    onSettled: () => void qc.invalidateQueries({ queryKey: queryKeys.backups }),
  });
}

/** 纳入托管、移出 Hub 时保存的完整备份 */
export function useHubBackups() {
  return useQuery({
    queryKey: queryKeys.hubBackups,
    queryFn: () => skillsApi.listHubBackups(),
  });
}

export function useDeleteHubBackups() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (ids: string[]) => skillsApi.deleteHubBackups(ids),
    onSuccess: (count) => toast.success(`已删除 ${count} 份完整备份`),
    onError: (e: unknown) => toast.error(String(e)),
    onSettled: () =>
      void qc.invalidateQueries({ queryKey: queryKeys.hubBackups }),
  });
}

export function useAppVersion() {
  return useQuery({
    queryKey: queryKeys.version,
    queryFn: () => systemApi.getVersion(),
    staleTime: Infinity,
  });
}

/**
 * 用户在 Studio 之外改动了 skill 目录时刷新。
 * 后端 watcher 已做 400ms 去抖，这里直接失效即可。
 */
export function useSkillsAutoRefresh() {
  const target = useTarget();
  const qc = useQueryClient();
  useTauriEvent("skills-changed", () => {
    if (target.id !== "local") return;
    void qc.invalidateQueries({ queryKey: queryKeys.skills });
  });
  useEffect(() => {
    if (target.id === "local" || !target.connected) return;
    const timer = setInterval(() => void qc.invalidateQueries(), 30000);
    return () => clearInterval(timer);
  }, [target.id, target.connected, qc]);
}

/* ─────────────────── 变更 ─────────────────── */

/** 任何会改动文件或配置的操作都要连带刷新 skill 状态 */
function useInvalidateAfterWrite() {
  const qc = useQueryClient();
  return () => {
    void qc.invalidateQueries({ queryKey: queryKeys.skills });
    void qc.invalidateQueries({ queryKey: queryKeys.groups });
    void qc.invalidateQueries({ queryKey: queryKeys.projects });
    void qc.invalidateQueries({ queryKey: queryKeys.backups });
    void qc.invalidateQueries({ queryKey: queryKeys.config });
    void qc.invalidateQueries({ queryKey: queryKeys.skillBackups });
    void qc.invalidateQueries({ queryKey: queryKeys.hubBackups });
  };
}

export function useRegisterSkills() {
  const invalidate = useInvalidateAfterWrite();
  return useMutation({
    mutationFn: (v: {
      skillIds: string[];
      agentIds: string[];
      mode?: LinkMode;
      force?: boolean;
    }) => skillsApi.register(v.skillIds, v.agentIds, v.mode, v.force ?? false),
    onSuccess: (report) => toastLinkReport(report, "注册"),
    onError: (e: unknown) => toast.error(String(e)),
    onSettled: invalidate,
  });
}

export function useSetSkillEnabled() {
  const invalidate = useInvalidateAfterWrite();
  return useMutation({
    mutationFn: (v: { skillId: string; agentId: string; enabled: boolean }) =>
      skillsApi.setEnabled(v.skillId, v.agentId, v.enabled),
    onSuccess: (_r, v) =>
      toast.success(v.enabled ? "已启用 skill" : "已停用 skill"),
    onError: (e: unknown) => toast.error(String(e)),
    onSettled: invalidate,
  });
}

export function useAdoptToHub() {
  const invalidate = useInvalidateAfterWrite();
  return useMutation({
    mutationFn: (skillId: string) => skillsApi.adoptToHub(skillId),
    onSuccess: (skill) => toast.success(`已托管到 Hub：${skill.name}`),
    onError: (e: unknown) => toast.error(String(e)),
    onSettled: invalidate,
  });
}

export function usePrune() {
  const invalidate = useInvalidateAfterWrite();
  return useMutation({
    mutationFn: () => skillsApi.prune(),
    onSuccess: (n) =>
      n > 0
        ? toast.success(`已清理 ${n} 条失效引用`)
        : toast.info("没有失效引用"),
    onError: (e: unknown) => toast.error(String(e)),
    onSettled: invalidate,
  });
}

export function useCreateProject() {
  const invalidate = useInvalidateAfterWrite();
  return useMutation({
    mutationFn: (v: { name: string; root: string }) =>
      projectsApi.create(v.name, v.root),
    onSuccess: (p) => toast.success(`已添加项目：${p.name}`),
    onError: (e: unknown) => toast.error(String(e)),
    onSettled: invalidate,
  });
}

export function useDeleteProject() {
  const invalidate = useInvalidateAfterWrite();
  return useMutation({
    mutationFn: (projectId: string) => projectsApi.remove(projectId),
    onSuccess: () => toast.success("项目已移除（项目目录里的文件未变动）"),
    onError: (e: unknown) => toast.error(String(e)),
    onSettled: invalidate,
  });
}

export function useApplyProject() {
  const invalidate = useInvalidateAfterWrite();
  return useMutation({
    mutationFn: (v: {
      projectId: string;
      selection: {
        agentIds: string[];
        skillIds: string[];
        groupIds: string[];
        linkMode: LinkMode;
      };
    }) => projectsApi.apply(v.projectId, v.selection),
    onSuccess: () => toast.success("项目选择已保存并写入"),
    onError: (e: unknown) => toast.error(String(e)),
    onSettled: invalidate,
  });
}

export function useWriteProjectGitignore() {
  return useMutation({
    mutationFn: (projectId: string) => projectsApi.writeGitignore(projectId),
    onSuccess: (added) =>
      added
        ? toast.success("已写入 .gitignore")
        : toast.info(".gitignore 里已有该条目"),
    onError: (e: unknown) => toast.error(String(e)),
  });
}

export function useUpdateSettings() {
  const qc = useQueryClient();
  return useMutation({
    scope: { id: "update-settings" },
    mutationFn: (patch: SettingsPatch) => settingsApi.update(patch),
    onSuccess: (settings, patch) => {
      qc.setQueryData(queryKeys.settings, settings);
      // 每次保存都会先备份当前配置，并按保留份数轮转
      void qc.invalidateQueries({ queryKey: queryKeys.backups });
      const visibilityOnly = patch.disabledAgents !== undefined;
      // Leaving Agent management may stop a Skill group and restore manual policy.
      // Mark these snapshots stale, but avoid scans/CLI probes while editing settings.
      const refetchType = visibilityOnly
        ? ("none" as const)
        : ("active" as const);
      void qc.invalidateQueries({ queryKey: queryKeys.config, refetchType });
      if (
        patch.manageMcp !== undefined ||
        patch.agentDirOverrides !== undefined ||
        // 份数变了，后端已按新份数清理 MCP 备份（MCP Hub 的「已备份 N」）
        patch.backupKeep !== undefined
      ) {
        void qc.invalidateQueries({ queryKey: ["mcp", "local"] });
      }
      if (
        visibilityOnly ||
        patch.manageMcp !== undefined ||
        patch.agentDirOverrides !== undefined ||
        patch.showCodexBuiltinMcp !== undefined
      ) {
        void qc.invalidateQueries({
          queryKey: ["mcp", "inventory"],
          refetchType,
        });
      }
      const pathsChanged =
        patch.agentDirOverrides !== undefined ||
        patch.hubDir !== undefined ||
        patch.clearHubDir;
      if (
        visibilityOnly ||
        pathsChanged ||
        patch.preserveManualSkills !== undefined
      ) {
        void qc.invalidateQueries({ queryKey: queryKeys.groups, refetchType });
        void qc.invalidateQueries({ queryKey: queryKeys.skills, refetchType });
      }
      if (pathsChanged)
        void qc.invalidateQueries({ queryKey: queryKeys.agents });
    },
    onError: (e: unknown) => toast.error(String(e)),
  });
}

export function useRestoreBackup() {
  const invalidate = useInvalidateAfterWrite();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (path: string) => settingsApi.restoreBackup(path),
    onSuccess: () => {
      toast.success("已从备份恢复");
      void qc.invalidateQueries({ queryKey: queryKeys.agents });
      void qc.invalidateQueries({ queryKey: queryKeys.settings });
    },
    onError: (e: unknown) => toast.error(String(e)),
    onSettled: invalidate,
  });
}
