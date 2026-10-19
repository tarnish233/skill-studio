import { useState } from "react";
import {
  useMutation,
  useQueryClient,
  type QueryKey,
} from "@tanstack/react-query";
import { toast } from "sonner";
import { useUnsavedProject } from "@/components/common/NavigationGuard";

/**
 * 分组页的写操作（启停、重新应用、保存、删除）共用的执行方式：同一时间只做一件事，
 * 进行中不能离开页面；完成后先刷新出真实状态，再提示结果。多步操作可能已部分生效
 * （依次重新应用时后一个 Agent 失败），所以失败时同样刷新。
 */
export function useGroupRunner(refresh: () => Promise<unknown>) {
  const [busy, setBusy] = useState(false);
  useUnsavedProject(false, busy);
  async function run<T>(
    action: () => Promise<T>,
    options: {
      success?: string;
      /** 按结果补一句需要用户留意的话（如原样留下的副本）；有就改用警告提示 */
      notice?: (result: T) => string | undefined;
      onStart?: () => void;
      /** 刷新完成后、提示结果前 */
      onSettled?: () => void;
      onDone?: () => void;
      /** 由调用方展示错误（如弹窗里），不再弹提示 */
      onError?: (message: string) => void;
    } = {},
  ) {
    setBusy(true);
    options.onStart?.();
    let failure: string | null = null;
    let result: T | undefined;
    try {
      result = await action();
    } catch (e) {
      failure = String(e);
    }
    try {
      await refresh();
    } catch {
      // 刷新失败由页面的读取状态说明
    }
    options.onSettled?.();
    setBusy(false);
    if (failure === null) {
      options.onDone?.();
      // 动作已经成功：提醒只决定用哪种提示，不会把成功改成失败
      const notice = options.notice?.(result as T);
      if (notice)
        toast.warning(`${options.success ?? "已完成"}。${notice}。`, {
          duration: 8000,
        });
      else if (options.success) toast.success(options.success);
    } else if (options.onError) options.onError(failure);
    else toast.error(failure);
    return failure === null;
  }
  return { busy, run };
}

/**
 * 拖拽排序：松手立即按新顺序显示，后台保存；失败时退回原顺序并提示，成功不另外提示。
 */
export function useGroupReorder<T>({
  queryKey,
  reorder,
  save,
}: {
  queryKey: QueryKey;
  /** 把缓存里的数据换成新顺序 */
  reorder: (data: T, ids: string[]) => T;
  save: (ids: string[]) => Promise<unknown>;
}) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: save,
    onMutate: async (ids: string[]) => {
      await client.cancelQueries({ queryKey });
      const previous = client.getQueryData<T>(queryKey);
      if (previous !== undefined)
        client.setQueryData<T>(queryKey, reorder(previous, ids));
      return { previous };
    },
    onError: (e, _, context) => {
      if (context?.previous !== undefined)
        client.setQueryData(queryKey, context.previous);
      toast.error(`排序保存失败：${String(e)}`);
    },
    onSettled: () => client.invalidateQueries({ queryKey }),
  });
}

/** 按 id 顺序重排，并把 sortOrder 改成新位置 */
export function sortedBy<G extends { id: string; sortOrder: number }>(
  groups: G[],
  ids: string[],
): G[] {
  return ids.flatMap((id, sortOrder) => {
    const group = groups.find((g) => g.id === id);
    return group ? [{ ...group, sortOrder }] : [];
  });
}
