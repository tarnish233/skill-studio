import { supportsMcp } from "@/lib/mcpAgents";
import {
  readInitialView,
  initialResource,
  VIEW_STORAGE_KEY,
  type StaticView,
  type ViewId,
} from "@/lib/initialView";
import { AgentMcpGroups } from "@/pages/AgentMcpGroups";
import { McpProjectsPage } from "@/pages/McpProjectsPage";
import {
  TargetPicker,
  useTarget,
  useTargetConnecting,
} from "@/components/targets/TargetProvider";
import { SkillStudioIcon } from "@/components/common/SkillStudioIcon";
import { SidebarAction } from "@/components/common/SidebarAction";
import {
  NavigationGuard,
  useNavigationGuard,
} from "@/components/common/NavigationGuard";
import { InstallSkillsPage } from "@/pages/InstallSkillsPage";
import { useNativePageTools } from "@/hooks/useNativePageTools";
import { PageToolsContext } from "@/components/common/PageTools";
import { useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { motion, useReducedMotion } from "framer-motion";
import {
  Plug,
  ArrowLeft,
  FolderGit2,
  Layers,
  RefreshCw,
  Settings as SettingsIcon,
  TriangleAlert,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AgentIcon } from "@/components/common/AgentIcon";
import { NavSwitcher, type NavSection } from "@/components/common/NavSwitcher";
import { useAgents, useSettings, useSkillsAutoRefresh } from "@/hooks/useData";
import { systemApi } from "@/lib/api";
import { isLinux, isWindows } from "@/lib/platform";
import { AgentPage } from "@/pages/AgentGroupsPage";
import { LibraryPage } from "@/pages/LibraryPage";
import { ProjectsPage } from "@/pages/ProjectsPage";
import { McpPage, mcpEditorTitle, type McpEditorState } from "@/pages/McpPage";
import { SettingsPage } from "@/pages/SettingsPage";

// macOS 把红绿灯悬浮在内容上（titleBarStyle: Overlay），需要让出 28px；
// Windows / Linux 自带或自绘标题栏，不留空。
const DRAG_BAR_HEIGHT = isWindows() || isLinux() ? 0 : 28;

const AGENT_PREFIX = "agent:";

const STATIC_TITLES: Record<StaticView, string> = {
  mcp: "MCP Hub",
  library: "Skill Hub",
  projects: "项目",
  settings: "设置",
  install: "安装 skill",
};

export default function App() {
  return (
    <NavigationGuard>
      <AppContent />
    </NavigationGuard>
  );
}
function AppContent() {
  const reduceMotion = useReducedMotion();
  const target = useTarget();
  const connecting = useTargetConnecting();
  const requestNavigation = useNavigationGuard();
  const { data: allAgents = [], isDetecting, detectionFailed } = useAgents();
  const { data: settings } = useSettings();
  const mcpEnabled = settings ? settings.manageMcp !== false : false;
  const agents = useMemo(
    () => allAgents.filter((a) => !settings?.disabledAgents?.includes(a.id)),
    [allAgents, settings?.disabledAgents],
  );
  const queryClient = useQueryClient();
  useSkillsAutoRefresh();

  const [initError, setInitError] = useState<string | null>(null);
  const [view, setView] = useState<ViewId>(() =>
    readInitialView(settings, target.id),
  );
  const [resource, setResource] = useState<"skills" | "mcp">(() =>
    initialResource(view, settings),
  );
  const activeResource = mcpEnabled ? resource : "skills";
  const remoteMcp = target.id !== "local" && activeResource === "mcp";
  useEffect(() => {
    localStorage.setItem("skill-studio-resource", resource);
  }, [resource]);
  const [pageSlide, setPageSlide] = useState(0);
  const [searchHost, setSearchHost] = useState<HTMLDivElement | null>(null);
  const [addHost, setAddHost] = useState<HTMLDivElement | null>(null);
  const [mcpEditor, setMcpEditor] = useState<McpEditorState | null>(null);
  useEffect(() => setMcpEditor(null), [target.id]);
  const activeMcpEditor =
    mcpEnabled && view === "mcp" && target.id === "local" ? mcpEditor : null;
  const isSettings = view === "settings";
  const isSubpage = isSettings || view === "install" || !!activeMcpEditor;
  const nativeTools = useNativePageTools(
    { search: searchHost, add: addHost },
    !isSubpage && !connecting && !initError,
    `${target.id}:${activeResource}:${view}`,
  );

  // 设置页的返回目标 = 进入设置之前停留的那个视图
  const backTarget = useRef<ViewId>("library");
  useEffect(() => {
    if (settings?.manageMcp !== false) return;
    setResource("skills");
    setMcpEditor(null);
    if (backTarget.current === "mcp") backTarget.current = "library";
    if (view === "mcp") setView("library");
  }, [settings?.manageMcp, view]);
  useEffect(() => {
    if (view !== "settings" && view !== "install") {
      backTarget.current = view;
      localStorage.setItem(VIEW_STORAGE_KEY, view);
    }
  }, [view]);

  useEffect(() => {
    const disabledView = (id: ViewId) =>
      (remoteMcp && (id === "projects" || id.startsWith(AGENT_PREFIX))) ||
      (id.startsWith(AGENT_PREFIX) &&
        (settings?.disabledAgents?.includes(id.slice(AGENT_PREFIX.length)) ||
          (activeResource === "mcp" &&
            !supportsMcp(id.slice(AGENT_PREFIX.length)))));
    const hub = activeResource === "mcp" ? "mcp" : "library";
    if (disabledView(backTarget.current)) backTarget.current = hub;
    if (disabledView(view)) setView(hub);
  }, [settings?.disabledAgents, view, activeResource, remoteMcp]);

  // 启动期错误（例如配置文件坏了）要让用户看见，而不是静默用默认值跑
  useEffect(() => {
    void systemApi
      .getInitError()
      .then(setInitError)
      .catch(() => setInitError(null));
  }, []);

  const titles = useMemo(() => {
    const map: Record<string, string> = { ...STATIC_TITLES };
    for (const a of agents) {
      map[`${AGENT_PREFIX}${a.id}`] = a.displayName;
    }
    return map;
  }, [agents]);

  const sections = useMemo<NavSection<ViewId>[]>(
    () => [
      {
        items: [
          {
            id: "library",
            label: STATIC_TITLES.library,
            icon: <Layers className="h-5 w-5" />,
          },
          ...(mcpEnabled
            ? [
                {
                  id: "mcp" as ViewId,
                  label: "MCP Hub",
                  icon: <Plug className="h-5 w-5" />,
                },
              ]
            : []),
        ],
      },
      {
        label: "AGENT",
        items: agents
          .filter((a) => activeResource === "skills" || supportsMcp(a.id))
          .map((a) => ({
            id: `${AGENT_PREFIX}${a.id}` as ViewId,
            label: a.displayName,
            icon: (
              <AgentIcon
                agentId={a.id}
                className={a.id === "pi" ? "scale-90" : undefined}
              />
            ),
            badge: a.detected
              ? undefined
              : isDetecting
                ? "检测中"
                : detectionFailed
                  ? "检测失败"
                  : "未装",
            disabledReason: remoteMcp
              ? "远程 MCP 暂不支持分组管理，请在 MCP Hub 查看。"
              : undefined,
          })),
      },
      {
        label: "项目",
        items: [
          {
            id: "projects",
            label: STATIC_TITLES.projects,
            icon: <FolderGit2 className="h-5 w-5" />,
            disabledReason: remoteMcp
              ? "远程 MCP 暂不支持项目管理，请在 MCP Hub 查看。"
              : undefined,
          },
        ],
      },
    ],
    [
      agents,
      activeResource,
      mcpEnabled,
      remoteMcp,
      isDetecting,
      detectionFailed,
    ],
  );

  const refresh = () => {
    void queryClient.invalidateQueries();
  };

  const content = () => {
    if (remoteMcp && (view === "projects" || view.startsWith(AGENT_PREFIX)))
      return <McpPage />;
    if (view.startsWith(AGENT_PREFIX)) {
      return activeResource === "mcp" ? (
        <AgentMcpGroups key={view} agentId={view.slice(AGENT_PREFIX.length)} />
      ) : (
        <AgentPage key={view} agentId={view.slice(AGENT_PREFIX.length)} />
      );
    }
    switch (view) {
      case "mcp":
        return mcpEnabled ? (
          <McpPage editor={mcpEditor} onEditorChange={setMcpEditor} />
        ) : (
          <LibraryPage
            onAdd={() => requestNavigation(() => setView("install"))}
          />
        );
      case "projects":
        return activeResource === "mcp" ? (
          <McpProjectsPage />
        ) : (
          <ProjectsPage />
        );
      case "install":
        return <InstallSkillsPage />;
      case "settings":
        return <SettingsPage />;
      default:
        return (
          <LibraryPage
            onAdd={() => requestNavigation(() => setView("install"))}
          />
        );
    }
  };

  const navigate = (next: ViewId) => {
    if (next === view && !activeMcpEditor) return;
    requestNavigation(() => {
      const order: ViewId[] = [
        ...sections.flatMap((section) => section.items.map((item) => item.id)),
        "settings",
      ];
      const from = order.indexOf(view);
      const to = order.indexOf(next);
      setPageSlide(from >= 0 && to >= 0 ? Math.sign(to - from) * 14 : 0);
      if (next === "library" || (next === "mcp" && mcpEnabled)) {
        setResource(next === "mcp" ? "mcp" : "skills");
      }
      setMcpEditor(null);
      setView(next);
    });
  };

  return (
    <PageToolsContext.Provider
      value={{ search: searchHost, add: addHost, native: nativeTools }}
    >
      <TooltipProvider delayDuration={300}>
        <div
          {...(connecting ? { inert: "" } : {})}
          aria-busy={connecting}
          className="app-shell flex h-screen flex-col overflow-hidden text-foreground selection:bg-primary/30"
        >
          <div
            data-tauri-drag-region
            className="fixed left-0 top-0 z-[70] w-[var(--app-chrome-size)]"
            style={{ height: DRAG_BAR_HEIGHT }}
          />
          <div className="flex min-h-0 flex-1">
            <aside
              data-tauri-drag-region="deep"
              aria-label="侧边栏"
              className="app-sidebar flex w-[var(--app-chrome-size)] shrink-0 flex-col items-center pb-3"
              style={{ paddingTop: DRAG_BAR_HEIGHT + 8 }}
            >
              <div className="mb-2 flex h-11 w-full shrink-0 items-center justify-center">
                <SidebarAction
                  label="Skill Studio · 返回 Hub"
                  onClick={() =>
                    navigate(activeResource === "mcp" ? "mcp" : "library")
                  }
                >
                  <SkillStudioIcon className="h-7 w-7" />
                </SidebarAction>
              </div>
              <div
                aria-hidden="true"
                className="mb-2 w-10 shrink-0 border-t border-border/70"
              />
              <nav
                aria-label="主导航"
                className="min-h-0 w-full flex-1 overflow-y-auto px-2 pb-3"
              >
                <NavSwitcher
                  sections={sections}
                  active={view}
                  selected={activeResource === "mcp" ? "mcp" : "library"}
                  onSelect={navigate}
                />
              </nav>
              <div className="flex w-full shrink-0 flex-col items-center gap-1 pt-3">
                <SidebarAction label="重新扫描" onClick={refresh}>
                  <RefreshCw className="h-5 w-5" />
                </SidebarAction>
                <TargetPicker compact />
                <SidebarAction
                  label="设置"
                  active={isSettings}
                  onClick={() => navigate("settings")}
                >
                  <SettingsIcon className="h-5 w-5" />
                </SidebarAction>
              </div>
            </aside>
            <section className="flex min-w-0 flex-1 flex-col overflow-hidden">
              <header
                data-tauri-drag-region="deep"
                className="grid h-[var(--app-toolbar-height)] shrink-0 grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-3 px-6"
              >
                <div className="flex min-w-0 items-center">
                  {isSubpage && (
                    <Button
                      data-tauri-no-drag
                      variant="ghost"
                      size="icon"
                      className="h-9 w-9 shrink-0 rounded-xl text-muted-foreground"
                      title="返回"
                      aria-label="返回"
                      onClick={() =>
                        activeMcpEditor
                          ? requestNavigation(() => setMcpEditor(null))
                          : navigate(backTarget.current)
                      }
                    >
                      <ArrowLeft className="h-5 w-5" />
                    </Button>
                  )}
                </div>
                <div className="flex min-w-0 max-w-sm items-baseline justify-center gap-3">
                  <h1 className="shrink-0 text-xl font-semibold leading-5 tracking-tight">
                    {activeMcpEditor
                      ? mcpEditorTitle(activeMcpEditor)
                      : (titles[view] ?? "Skill Studio")}
                  </h1>
                  {isSettings ? (
                    <p
                      className="truncate text-[11px] leading-none text-muted-foreground"
                      title={`当前管理目标：${target.name} · 目录、管理策略与备份属于此目标；外观与服务器连接属于桌面应用。`}
                    >
                      当前管理目标：{target.name} ·
                      目录、管理策略与备份属于此目标；外观与服务器连接属于桌面应用。
                    </p>
                  ) : (
                    <p className="truncate text-[11px] leading-none text-muted-foreground">
                      {target.name}
                      {view.startsWith(AGENT_PREFIX) || view === "projects"
                        ? ` · ${activeResource === "mcp" ? "MCP" : "Skill"} 管理`
                        : ""}
                    </p>
                  )}
                </div>
                {!isSubpage && (
                  <div
                    className="flex items-center justify-self-end gap-3"
                    // Offset the 24px header padding and half of the 32px add
                    // host so its center has equal top and right insets.
                    style={{
                      marginRight:
                        "calc(var(--app-toolbar-height) / 2 - 2.5rem)",
                    }}
                    data-tauri-drag-region="false"
                    data-tauri-no-drag
                  >
                    <div ref={setSearchHost} className="h-8 w-48 min-w-0" />
                    <div ref={setAddHost} className="h-8 w-8 shrink-0" />
                  </div>
                )}
              </header>
              <main className="app-content flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-background">
                {initError && (
                  <div className="mx-6 mt-4 flex items-start gap-2 rounded-xl border border-amber-500/50 bg-amber-500/10 px-4 py-3">
                    <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />
                    <div className="min-w-0 flex-1 text-xs leading-relaxed">
                      <p className="font-medium">配置未能正常加载</p>
                      <p className="pt-0.5 text-muted-foreground">
                        {initError}
                      </p>
                    </div>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setInitError(null)}
                    >
                      知道了
                    </Button>
                  </div>
                )}
                <fieldset
                  disabled={!target.connected && !isSettings}
                  className={`flex min-h-0 min-w-0 flex-1 flex-col ${!target.connected && !isSettings ? "pointer-events-none opacity-60" : ""}`}
                >
                  <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden px-6">
                    <motion.div
                      key={`${view}:${activeResource}`}
                      className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
                      // Transparent native windows can delay animation frames while
                      // inactive. Keep content visible from its very first paint.
                      initial={reduceMotion ? false : { y: pageSlide }}
                      animate={{ y: 0 }}
                      transition={{
                        duration: reduceMotion ? 0 : 0.22,
                        ease: [0.22, 1, 0.36, 1],
                      }}
                    >
                      {content()}
                    </motion.div>
                  </div>
                </fieldset>
              </main>
            </section>
          </div>
        </div>
      </TooltipProvider>
    </PageToolsContext.Provider>
  );
}
