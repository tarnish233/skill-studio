import { useQuery } from "@tanstack/react-query";
import { useTarget } from "@/components/targets/TargetProvider";
import { pluginsApi } from "@/lib/api/plugins";
import { queryKeys } from "@/lib/queryKeys";

export function usePluginInventory() {
  const local = useTarget().id === "local";
  return useQuery({
    queryKey: queryKeys.plugins,
    queryFn: pluginsApi.scan,
    enabled: local,
    staleTime: 30_000,
    refetchInterval: local ? 30_000 : false,
  });
}
export function usePluginCli(enabled: boolean) {
  const local = useTarget().id === "local";
  return useQuery({
    queryKey: queryKeys.pluginCli,
    queryFn: pluginsApi.capabilities,
    enabled: local && enabled,
    staleTime: 60_000,
  });
}
