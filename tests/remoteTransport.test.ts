import { afterEach, describe, expect, it } from "vitest";
import {
  invoke,
  setTarget,
  getTarget,
  getPending,
  uploadLocalSkill,
} from "@/lib/api/transport";
import { calls, handlers, defaultSettings } from "./mocks/tauri";

afterEach(() => setTarget({ id: "local", name: "本机", connected: true }));

describe("remote target routing", () => {
  it("routes every project, Hub, group and backup operation to the chosen server", async () => {
    setTarget({ id: "node2", name: "node2", connected: true });
    handlers.set("remote_request", ({ serverId, method }) => ({
      serverId,
      method,
    }));
    for (const method of [
      "list_projects",
      "create_project",
      "apply_project",
      "scan_skills",
      "list_groups",
      "list_backups",
      "restore_backup",
      "delete_backup",
      "list_hub_backups",
      "delete_hub_backups",
      "read_skill_document",
      "register_skills",
      "set_agent_group",
      "reapply_agent_groups",
      "set_project_agent",
      "reapply_project",
      "save_project",
    ]) {
      expect(await invoke(method, { sample: 1 })).toEqual({
        serverId: "node2",
        method,
      });
    }
    expect(calls.every((c) => c.command === "remote_request")).toBe(true);
  });
  it("keeps in-flight operations bound to their original server", async () => {
    let resolve!: (value: string) => void;
    handlers.set(
      "remote_request",
      () =>
        new Promise((r) => {
          resolve = r;
        }),
    );
    setTarget({ id: "a", name: "A", connected: true });
    const response = invoke("create_project", {
      name: "test",
      root: "/tmp/test",
    });
    expect(getPending()).toBe(1);
    setTarget({ id: "b", name: "B", connected: true });
    resolve("created-on-a");
    expect(await response).toBe("created-on-a");
    expect(calls[0].args).toMatchObject({ serverId: "a" });
    expect(getPending()).toBe(0);
  });
  it("never falls back to local operations after disconnection", async () => {
    setTarget({ id: "node2", name: "node2", connected: false });
    await expect(invoke("delete_project", { projectId: "x" })).rejects.toThrow(
      "断开",
    );
    expect(calls).toHaveLength(0);
    setTarget({ id: "node2", name: "node2", connected: true });
    handlers.set("remote_request", () => {
      throw new Error("REMOTE_DISCONNECTED:network lost");
    });
    await expect(invoke("scan_skills")).rejects.toBeTruthy();
    expect(getTarget().connected).toBe(false);
  });
  it("keeps appearance local while management settings remain remote", async () => {
    setTarget({ id: "node2", name: "node2", connected: true });
    handlers.set("get_settings", () => ({
      ...defaultSettings(),
      theme: "dark",
      language: "zh",
    }));
    handlers.set("remote_request", ({ method }) =>
      method === "get_settings"
        ? { theme: "light", language: "en", hubDir: "/remote/hub" }
        : null,
    );
    expect(await invoke("get_settings")).toMatchObject({
      theme: "dark",
      language: "zh",
      hubDir: "/remote/hub",
    });
    await invoke("update_settings", { patch: { theme: "system" } });
    expect(calls.find((c) => c.command === "update_settings")?.args).toEqual({
      patch: { theme: "system" },
    });
    expect(
      calls
        .filter((c) => c.command === "remote_request")
        .some(
          (c) => (c.args as { method: string }).method === "update_settings",
        ),
    ).toBe(false);
    expect(getTarget().id).toBe("node2");
  });
});

it("cancels desktop upload when the target changed while choosing files", async () => {
  setTarget({ id: "a", name: "A", connected: true });
  let resolve!: (path: string) => void;
  const operation = uploadLocalSkill(
    () =>
      new Promise<string>((r) => {
        resolve = r;
      }),
  );
  setTarget({ id: "local", name: "本机", connected: true });
  resolve("/test/skill");
  await expect(operation).rejects.toThrow("目标已变化");
  expect(calls).toHaveLength(0);
  expect(getPending()).toBe(0);
});
