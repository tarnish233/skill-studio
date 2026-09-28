import { SshHostPicker } from "./SshHostPicker";
import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke as nativeInvoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Check,
  ChevronDown,
  Folder,
  Loader2,
  Monitor,
  Network,
  Power,
  Pencil,
  Plus,
  Server,
  ServerCog,
  Trash2,
  Unplug,
} from "lucide-react";
import { toast } from "sonner";
import { SettingCard, SettingsSection } from "@/components/common/SettingCard";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  getPending,
  getTarget,
  requestRemote,
  ServerProfile,
  setRemoteDialogs,
  setTarget,
  subscribeTransport,
} from "@/lib/api/transport";
import { queryClient } from "@/lib/query/queryClient";
import { useNavigationGuard } from "@/components/common/NavigationGuard";

type Prompt = {
  requestId: string;
  serverId: string;
  prompt: string;
  confirmation: boolean;
};
type Directory = {
  path: string;
  parent: string | null;
  directories: { name: string; path: string }[];
};
type DirectoryRequest = {
  path?: string;
  select: boolean;
  resolve: (path: string | null) => void;
  serverId: string;
};
// Desktop-only preference: independent of the currently managed machine.
const SERVER_CONNECTIONS_KEY = "skill-studio:server-connections-enabled";
function readServerConnectionsEnabled() {
  try {
    return localStorage.getItem(SERVER_CONNECTIONS_KEY) !== "false";
  } catch {
    return true;
  }
}
const Context = createContext<{
  serverConnectionsEnabled: boolean;
  changingConnections: boolean;
  setServerConnectionsEnabled: (enabled: boolean) => void;
  servers: ServerProfile[];
  connecting: string | null;
  stage: string;
  error: string | null;
  failedServer: string | null;
  choose: (profile?: ServerProfile) => Promise<void>;
  edit: (profile?: ServerProfile) => void;
  remove: (profile: ServerProfile) => Promise<void>;
  disconnect: () => Promise<void>;
} | null>(null);
export const useTarget = () =>
  useSyncExternalStore(subscribeTransport, getTarget, getTarget);
export const useTargetBusy = () =>
  useSyncExternalStore(subscribeTransport, getPending, getPending);
export const useTargetConnecting = () => {
  const ctx = useContext(Context);
  return !!ctx?.connecting || !!ctx?.changingConnections;
};

export function TargetProvider({ children }: { children: React.ReactNode }) {
  const active = useTarget();
  const [serverConnectionsEnabled, setServerConnectionsEnabledState] = useState(
    readServerConnectionsEnabled,
  );
  const connectionsAllowed = useRef(serverConnectionsEnabled);
  const [changingConnections, setChangingConnections] = useState(false);
  const changingConnectionsRef = useRef(false);
  const requestNavigation = useNavigationGuard();
  const [servers, setServers] = useState<ServerProfile[]>([]);
  const [connecting, setConnecting] = useState<string | null>(null);
  const [stage, setStage] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [failedServer, setFailedServer] = useState<string | null>(null);
  useEffect(() => {
    if (error) toast.error(error.replace(/^REMOTE_DISCONNECTED:/, ""));
  }, [error]);
  const [editing, setEditing] = useState<ServerProfile | null>(null);
  const isNewServer =
    editing !== null && !servers.some((s) => s.id === editing.id);
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const [answer, setAnswer] = useState("");
  const [directory, setDirectory] = useState<DirectoryRequest | null>(null);
  const [path, setPath] = useState("");
  const [listing, setListing] = useState<Directory | null>(null);
  const [directoryError, setDirectoryError] = useState<string | null>(null);
  const [loadingDirectory, setLoadingDirectory] = useState(false);
  const [showHidden, setShowHidden] = useState(false);
  const clients = useRef(
    new Map<string, QueryClient>([["local", queryClient]]),
  );
  if (!clients.current.has(active.id))
    clients.current.set(
      active.id,
      new QueryClient({
        defaultOptions: {
          queries: { retry: false, refetchOnWindowFocus: false },
          mutations: { retry: false },
        },
      }),
    );

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    if (serverConnectionsEnabled)
      void nativeInvoke<ServerProfile[]>("list_servers")
        .then(setServers)
        .catch((e) => setError(String(e)));
    const subscriptions = [
      listen<Prompt>("ssh-prompt", (e) => {
        setPrompt(e.payload);
        setAnswer("");
      }),
      listen<{ serverId: string; stage: string }>("remote-progress", (e) =>
        setStage(e.payload.stage),
      ),
    ];
    return () => {
      subscriptions.forEach((p) => void p.then((off) => off()));
    };
  }, [serverConnectionsEnabled]);

  useEffect(() => {
    setRemoteDialogs(
      (path) =>
        new Promise((resolve) =>
          setDirectory({
            path,
            select: true,
            resolve,
            serverId: getTarget().id,
          }),
        ),
      (path) =>
        new Promise((resolve) =>
          setDirectory({
            path,
            select: false,
            resolve: () => resolve(),
            serverId: getTarget().id,
          }),
        ),
    );
  }, []);

  const loadDirectory = async (
    nextPath: string,
    serverId = directory?.serverId,
  ) => {
    if (!serverId) return;
    setLoadingDirectory(true);
    setDirectoryError(null);
    try {
      const next = await requestRemote<Directory>(serverId, "list_directory", {
        path: nextPath,
      });
      setListing(next);
      setPath(next.path);
    } catch (e) {
      setDirectoryError(String(e));
    } finally {
      setLoadingDirectory(false);
    }
  };
  useEffect(() => {
    if (directory) {
      setListing(null);
      setPath(directory.path ?? "");
      void loadDirectory(directory.path ?? "", directory.serverId);
    }
  }, [directory]);
  const closeDirectory = (value: string | null) => {
    directory?.resolve(value);
    setDirectory(null);
  };

  const changeServerConnections = async (enabled: boolean) => {
    if (getPending() || connecting || changingConnectionsRef.current) {
      toast.info("正在处理当前目标的操作，请稍候");
      return;
    }
    changingConnectionsRef.current = true;
    setChangingConnections(true);
    try {
      if (!enabled) {
        const current = getTarget();
        const ids = new Set(servers.map((server) => server.id));
        if (current.id !== "local") ids.add(current.id);
        for (const serverId of ids) {
          await nativeInvoke("disconnect_server", { serverId });
          if (getTarget().id === serverId)
            setTarget({ ...getTarget(), connected: false });
        }
        await clients.current.get(current.id)?.cancelQueries();
        setTarget({ id: "local", name: "本机", connected: true });
        setEditing(null);
        directory?.resolve(null);
        setDirectory(null);
        for (const [id, client] of clients.current) {
          if (id !== "local") {
            client.clear();
            clients.current.delete(id);
          }
        }
      }
      localStorage.setItem(SERVER_CONNECTIONS_KEY, String(enabled));
      connectionsAllowed.current = enabled;
      setServerConnectionsEnabledState(enabled);
      setError(null);
      setFailedServer(null);
    } catch (e) {
      setError(String(e));
    } finally {
      changingConnectionsRef.current = false;
      setChangingConnections(false);
    }
  };
  const setServerConnectionsEnabled = (enabled: boolean) => {
    requestNavigation(() => void changeServerConnections(enabled));
  };

  const choose = async (profile?: ServerProfile) => {
    if (profile && !connectionsAllowed.current) return;
    const current = getTarget();
    if (getPending() || connecting || changingConnectionsRef.current) {
      toast.info("正在处理当前目标的操作，请稍候");
      return;
    }
    if (!profile) {
      await clients.current.get(active.id)?.cancelQueries();
      setTarget({ id: "local", name: "本机", connected: true });
      setError(null);
      return;
    }
    if (current.id === profile.id && current.connected) return;
    setConnecting(profile.id);
    setStage("正在连接服务器");
    setError(null);
    try {
      await nativeInvoke("connect_server", { profile });
      await clients.current.get(active.id)?.cancelQueries();
      clients.current.get(profile.id)?.clear();
      setFailedServer(null);
      setTarget({ id: profile.id, name: profile.name, connected: true });
    } catch (e) {
      setFailedServer(profile.id);
      setError(String(e));
    } finally {
      setConnecting(null);
    }
  };
  const disconnect = async () => {
    if (getPending()) {
      toast.info("正在处理服务器操作，请稍候");
      return;
    }
    try {
      await nativeInvoke("disconnect_server", { serverId: active.id });
      setTarget({ ...active, connected: false });
    } catch (e) {
      setError(String(e));
    }
  };
  const edit = (profile?: ServerProfile) => {
    if (!connectionsAllowed.current || changingConnectionsRef.current) return;
    setEditing(
      profile
        ? { ...profile }
        : {
            id: crypto.randomUUID(),
            name: "",
            host: "",
            user: null,
            port: null,
            identityFile: null,
            jumpHost: null,
            passwordAuth: false,
            helperBinary: null,
          },
    );
  };
  const save = () => {
    if (!connectionsAllowed.current || changingConnectionsRef.current) return;
    if (getPending() || connecting) {
      toast.info("正在处理当前目标的操作，请稍候");
      return;
    }
    requestNavigation(() => void saveAndConnect());
  };
  const saveAndConnect = async () => {
    if (
      !editing ||
      !connectionsAllowed.current ||
      changingConnectionsRef.current
    )
      return;
    const next = [...servers.filter((s) => s.id !== editing.id), editing];
    try {
      await nativeInvoke("save_servers", { servers: next });
      setServers(next);
      setEditing(null);
      if (active.id === editing.id) {
        await nativeInvoke("disconnect_server", { serverId: active.id });
        setTarget({ ...active, name: editing.name, connected: false });
      }
      await choose(editing);
    } catch (e) {
      setError(String(e));
    }
  };
  const remove = async (profile: ServerProfile) => {
    if (!connectionsAllowed.current || changingConnectionsRef.current) return;
    if (getPending()) {
      toast.info("正在处理操作，请稍候");
      return;
    }
    try {
      await nativeInvoke("disconnect_server", { serverId: profile.id });
      const next = servers.filter((s) => s.id !== profile.id);
      await nativeInvoke("save_servers", { servers: next });
      setServers(next);
      clients.current.get(profile.id)?.clear();
      if (active.id === profile.id)
        setTarget({ id: "local", name: "本机", connected: true });
    } catch (e) {
      setError(String(e));
    }
  };
  const respond = async (value: string | null) => {
    if (!prompt) return;
    const id = prompt.requestId;
    setPrompt(null);
    setAnswer("");
    await nativeInvoke("answer_ssh_prompt", {
      requestId: id,
      answer: value,
    }).catch((e) => setError(String(e)));
  };
  useEffect(() => {
    if (active.id === "local" || !active.connected) return;
    const timer = setInterval(() => {
      if (!getPending()) void requestRemote(active.id, "hello").catch(() => {});
    }, 15000);
    return () => clearInterval(timer);
  }, [active.id, active.connected]);

  return (
    <Context.Provider
      value={{
        serverConnectionsEnabled,
        changingConnections,
        setServerConnectionsEnabled,
        servers,
        connecting,
        stage,
        error,
        failedServer,
        choose,
        edit,
        remove,
        disconnect,
      }}
    >
      {error && (
        <span role="alert" className="sr-only">
          {error}
        </span>
      )}
      <QueryClientProvider client={clients.current.get(active.id)!}>
        <div className="contents" key={active.id}>
          {children}
        </div>
      </QueryClientProvider>
      <Dialog
        open={editing !== null}
        onOpenChange={(v) => {
          if (!v) setEditing(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {servers.some((s) => s.id === editing?.id)
                ? "编辑服务器"
                : "添加服务器"}
            </DialogTitle>
            <DialogDescription>
              {isNewServer
                ? "从本机 SSH 配置中选择一台服务器。"
                : "修改已保存的连接；留空的选项沿用系统 SSH 配置。"}
            </DialogDescription>
          </DialogHeader>
          {editing && isNewServer && (
            <SshHostPicker
              selected={editing.host}
              onSelect={(host) => setEditing({ ...editing, name: host, host })}
            />
          )}
          {editing && !isNewServer && (
            <div className="min-h-0 space-y-4 overflow-y-auto px-6 py-4">
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <Label htmlFor="server-name">名称</Label>
                  <Input
                    id="server-name"
                    value={editing.name}
                    onChange={(e) =>
                      setEditing({ ...editing, name: e.target.value })
                    }
                    placeholder="开发服务器"
                  />
                </div>
                <div>
                  <Label htmlFor="server-host">SSH Host 或服务器地址</Label>
                  <Input
                    id="server-host"
                    value={editing.host}
                    onChange={(e) =>
                      setEditing({ ...editing, host: e.target.value })
                    }
                    placeholder="192.168.1.2"
                  />
                </div>
              </div>
              <div>
                <Label htmlFor="server-auth">认证方式</Label>
                <select
                  id="server-auth"
                  className="mt-1 h-10 w-full rounded-md border bg-background px-3 text-sm"
                  value={editing.passwordAuth ? "password" : "ssh"}
                  onChange={(e) =>
                    setEditing({
                      ...editing,
                      passwordAuth: e.target.value === "password",
                    })
                  }
                >
                  <option value="ssh">SSH 配置 / 密钥</option>
                  <option value="password">密码 / 交互认证</option>
                </select>
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <Label htmlFor="server-user">
                    用户名（留空沿用 SSH 配置）
                  </Label>
                  <Input
                    id="server-user"
                    value={editing.user ?? ""}
                    onChange={(e) =>
                      setEditing({ ...editing, user: e.target.value || null })
                    }
                  />
                </div>
                <div>
                  <Label htmlFor="server-port">端口</Label>
                  <Input
                    id="server-port"
                    type="number"
                    min={1}
                    max={65535}
                    placeholder="SSH 配置 / 22"
                    value={editing.port ?? ""}
                    onChange={(e) =>
                      setEditing({
                        ...editing,
                        port: e.target.value ? Number(e.target.value) : null,
                      })
                    }
                  />
                </div>
              </div>
              <details className="space-y-3">
                <summary className="cursor-pointer text-sm text-muted-foreground">
                  高级选项
                </summary>
                <div>
                  <Label htmlFor="server-key">私钥文件</Label>
                  <div className="flex gap-2">
                    <Input
                      id="server-key"
                      value={editing.identityFile ?? ""}
                      onChange={(e) =>
                        setEditing({
                          ...editing,
                          identityFile: e.target.value || null,
                        })
                      }
                      placeholder="使用 ssh-agent 或 SSH 配置"
                    />
                    <Button
                      variant="outline"
                      onClick={async () => {
                        const path = await open({ multiple: false });
                        if (typeof path === "string")
                          setEditing({ ...editing, identityFile: path });
                      }}
                    >
                      选择
                    </Button>
                  </div>
                </div>
                <div>
                  <Label htmlFor="server-jump">跳板机</Label>
                  <Input
                    id="server-jump"
                    value={editing.jumpHost ?? ""}
                    onChange={(e) =>
                      setEditing({
                        ...editing,
                        jumpHost: e.target.value || null,
                      })
                    }
                    placeholder="SSH Host 或 user@host:port"
                  />
                </div>
                <div>
                  <Label htmlFor="server-helper">Linux 辅助程序</Label>
                  <div className="flex gap-2">
                    <Input
                      id="server-helper"
                      value={editing.helperBinary ?? ""}
                      onChange={(e) =>
                        setEditing({
                          ...editing,
                          helperBinary: e.target.value || null,
                        })
                      }
                      placeholder="自动使用应用内置组件"
                    />
                    <Button
                      variant="outline"
                      onClick={async () => {
                        const path = await open({ multiple: false });
                        if (typeof path === "string")
                          setEditing({ ...editing, helperBinary: path });
                      }}
                    >
                      选择
                    </Button>
                  </div>
                </div>
              </details>
              {error && (
                <p
                  role="alert"
                  className="break-words text-sm text-destructive"
                >
                  {error}
                </p>
              )}
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditing(null)}>
              取消
            </Button>
            <Button
              disabled={
                !editing?.name.trim() || !editing?.host.trim() || !!connecting
              }
              onClick={() => void save()}
            >
              {isNewServer ? "添加并连接" : "保存并连接"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog
        open={prompt !== null}
        onOpenChange={(v) => {
          if (!v) void respond(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {prompt?.confirmation ? "确认服务器身份" : "SSH 身份验证"}
            </DialogTitle>
            <DialogDescription className="whitespace-pre-wrap break-words">
              {prompt?.prompt}
            </DialogDescription>
          </DialogHeader>
          {!prompt?.confirmation && (
            <div className="px-6 py-4">
              <Input
                autoFocus
                aria-label="SSH 密码或密钥口令"
                type="password"
                autoComplete="off"
                value={answer}
                onChange={(e) => setAnswer(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void respond(answer);
                }}
              />
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => void respond(null)}>
              取消
            </Button>
            <Button
              onClick={() =>
                void respond(prompt?.confirmation ? "yes" : answer)
              }
            >
              {prompt?.confirmation ? "信任并连接" : "继续"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog
        open={directory !== null}
        onOpenChange={(v) => {
          if (!v) closeDirectory(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {directory?.select ? "选择服务器目录" : "浏览服务器目录"} ·{" "}
              {active.name}
            </DialogTitle>
            <DialogDescription>所有路径均位于当前服务器。</DialogDescription>
          </DialogHeader>
          <div className="min-h-0 space-y-3 overflow-y-auto px-6 py-4">
            <form
              className="flex gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                void loadDirectory(path);
              }}
            >
              <Input
                aria-label="服务器目录路径"
                value={path}
                onChange={(e) => setPath(e.target.value)}
              />
              <Button
                type="submit"
                variant="outline"
                disabled={loadingDirectory}
              >
                前往
              </Button>
            </form>
            <div className="flex items-center justify-between">
              <Button
                variant="ghost"
                disabled={!listing?.parent || loadingDirectory}
                onClick={() => void loadDirectory(listing!.parent!)}
              >
                上一级
              </Button>
              <label className="text-sm">
                <input
                  type="checkbox"
                  checked={showHidden}
                  onChange={(e) => setShowHidden(e.target.checked)}
                />{" "}
                显示隐藏目录
              </label>
            </div>
            <div className="max-h-64 min-h-32 space-y-1 overflow-y-auto">
              {loadingDirectory ? (
                <p>正在读取目录…</p>
              ) : (
                listing?.directories
                  .filter((d) => showHidden || !d.name.startsWith("."))
                  .map((d) => (
                    <Button
                      key={d.path}
                      className="w-full justify-start"
                      variant="ghost"
                      onClick={() => void loadDirectory(d.path)}
                    >
                      <Folder className="mr-2 h-4 w-4" />
                      {d.name}
                    </Button>
                  ))
              )}
            </div>
            {directoryError && (
              <p role="alert" className="text-sm text-destructive">
                {directoryError}
              </p>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => closeDirectory(null)}>
              关闭
            </Button>
            {directory?.select && (
              <Button
                disabled={
                  loadingDirectory ||
                  !listing ||
                  !!directoryError ||
                  path !== listing.path
                }
                onClick={() => closeDirectory(listing!.path)}
              >
                选择此目录
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Context.Provider>
  );
}

export function TargetPicker({ compact = false }: { compact?: boolean }) {
  const ctx = useContext(Context);
  const active = useTarget();
  const pending = useTargetBusy();
  const guard = useNavigationGuard();
  if (!ctx || !ctx.serverConnectionsEnabled) return null;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant={compact ? "ghost" : "outline"}
          aria-label={compact ? active.name : undefined}
          className={`${compact ? "h-9 w-9 shrink-0 rounded-xl p-0 text-muted-foreground" : "max-w-48 gap-2"} ${!ctx.connecting && active.id !== "local" ? (active.connected ? "text-emerald-600 hover:text-emerald-600 dark:text-emerald-400 dark:hover:text-emerald-400" : "text-destructive hover:text-destructive") : ""}`}
          title={
            compact
              ? `管理目标：${active.name}`
              : ctx.connecting
                ? ctx.stage
                : active.id === "local"
                  ? "本机"
                  : active.connected
                    ? "已连接"
                    : "已断开，点击重新连接"
          }
          disabled={!!ctx.connecting || ctx.changingConnections || pending > 0}
        >
          {ctx.connecting ? (
            <Loader2 className="h-5 w-5 animate-spin" />
          ) : active.id === "local" ? (
            <Monitor className="h-5 w-5" />
          ) : (
            <Server className="h-5 w-5" />
          )}
          <span className={compact ? "sr-only" : "truncate"}>
            {ctx.connecting ? "连接中…" : active.name}
          </span>
          {!compact && <ChevronDown className="h-4 w-4" />}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        side={compact ? "right" : "bottom"}
        align={compact ? "end" : "start"}
        className="w-72"
      >
        <DropdownMenuItem onSelect={() => guard(() => void ctx.choose())}>
          <Monitor className="mr-2 h-4 w-4" />
          本机{active.id === "local" && <Check className="ml-auto h-4 w-4" />}
        </DropdownMenuItem>
        {ctx.servers.map((s) => (
          <DropdownMenuItem
            key={s.id}
            onSelect={() => guard(() => void ctx.choose(s))}
          >
            <Server className="mr-2 h-4 w-4" />
            <div>
              <div
                className={
                  ctx.failedServer === s.id ||
                  (active.id === s.id && !active.connected)
                    ? "text-destructive"
                    : active.id === s.id && active.connected
                      ? "text-emerald-600 dark:text-emerald-400"
                      : ""
                }
              >
                {s.name}
              </div>
              <div className="text-xs text-muted-foreground">
                {s.user ? `${s.user}@` : ""}
                {s.host}
              </div>
            </div>
            {active.id === s.id && <Check className="ml-auto h-4 w-4" />}
          </DropdownMenuItem>
        ))}
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => ctx.edit()}>
          <Plus className="mr-2 h-4 w-4" />
          添加服务器…
        </DropdownMenuItem>
        {active.id !== "local" && (
          <>
            <DropdownMenuItem
              onSelect={() =>
                ctx.edit(ctx.servers.find((s) => s.id === active.id))
              }
            >
              <Pencil className="mr-2 h-4 w-4" />
              编辑当前连接…
            </DropdownMenuItem>
            <DropdownMenuItem
              onSelect={() =>
                guard(() =>
                  active.connected
                    ? void ctx.disconnect()
                    : void ctx.choose(
                        ctx.servers.find((s) => s.id === active.id),
                      ),
                )
              }
            >
              <Unplug className="mr-2 h-4 w-4" />
              {active.connected ? "断开连接" : "重新连接"}
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export function ServerSettings() {
  const ctx = useContext(Context);
  const guard = useNavigationGuard();
  if (!ctx || !ctx.serverConnectionsEnabled) return null;
  return (
    <SettingsSection
      title="服务器设置"
      icon={<ServerCog />}
      action={
        <Button onClick={() => ctx.edit()}>
          <Plus className="mr-2 h-4 w-4" />
          添加服务器
        </Button>
      }
    >
      <p className="text-xs text-muted-foreground">
        连接记录保存在本机。移除记录不会删除服务器上的内容。
      </p>
      {ctx.servers.map((s) => (
        <SettingCard
          key={s.id}
          title={s.name}
          icon={<Server />}
          description={<span className="break-all">{s.host}</span>}
        >
          <Button
            variant="outline"
            disabled={!!ctx.connecting}
            onClick={() => guard(() => void ctx.choose(s))}
          >
            连接
          </Button>
          <Button
            aria-label={`编辑 ${s.name}`}
            variant="ghost"
            size="icon"
            onClick={() => ctx.edit(s)}
          >
            <Pencil className="h-4 w-4" />
          </Button>
          <Button
            aria-label={`移除 ${s.name}`}
            variant="ghost"
            size="icon"
            onClick={() => guard(() => void ctx.remove(s))}
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </SettingCard>
      ))}
    </SettingsSection>
  );
}

export function ServerConnectionSetting() {
  const ctx = useContext(Context);
  const pending = useTargetBusy();
  if (!ctx) return null;
  return (
    <SettingsSection title="远程管理" icon={<Network />}>
      <SettingCard
        title={
          <label htmlFor="server-connections-enabled">启用服务器连接</label>
        }
        description="关闭后仅管理本机，隐藏主页服务器选择器和下方连接配置；已保存的连接记录会保留。"
        icon={<Power />}
      >
        <Switch
          id="server-connections-enabled"
          checked={ctx.serverConnectionsEnabled}
          disabled={ctx.changingConnections || !!ctx.connecting || pending > 0}
          onCheckedChange={ctx.setServerConnectionsEnabled}
        />
      </SettingCard>
    </SettingsSection>
  );
}
