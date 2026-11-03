import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { mcpRequest } from "@/lib/api/mcp";
import type { ManagedMcp } from "@/lib/api/mcpManagement";
import { queryKeys } from "@/lib/queryKeys";

/** Debounce compatibility validation, not connection attempts. Bindings are not validation input. */
export function useMcpGatewayCheck(entry: ManagedMcp | null, enabled: boolean) {
  const signature = entry
    ? JSON.stringify({
        id: entry.id,
        name: entry.name,
        mode: entry.mode,
        definition: entry.definition,
        oauth: entry.oauth,
        clientId: entry.clientId,
        scopes: entry.scopes,
      })
    : null;
  const [ready, setReady] = useState(signature);
  useEffect(() => {
    if (signature === ready) return;
    const timer = setTimeout(() => setReady(signature), 350);
    return () => clearTimeout(timer);
  }, [signature, ready]);
  const settling = signature !== ready;
  const query = useQuery({
    queryKey: [...queryKeys.mcpGatewayCheck, ready],
    queryFn: () =>
      mcpRequest<{ issue: string | null }>("gatewayCheck", {
        entry: JSON.parse(ready!),
      }),
    enabled: enabled && !!ready && !settling,
    retry: false,
  });
  // Never let a previous successful check enable saving a new, unchecked definition.
  return { ...query, isPending: enabled && (settling || query.isPending) };
}
