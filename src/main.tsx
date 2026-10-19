import { TargetProvider } from "@/components/targets/TargetProvider";
import { NavigationGuard } from "@/components/common/NavigationGuard";
import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { Toaster } from "@/components/ui/sonner";
import { queryClient } from "@/lib/query/queryClient";
import { ThemeProvider } from "@/components/theme-provider";
import App from "@/App";
import { StartupBoundary } from "@/components/common/StartupBoundary";
import "@/index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <NavigationGuard>
          <TargetProvider>
            <StartupBoundary>
              <App />
            </StartupBoundary>
          </TargetProvider>
        </NavigationGuard>
        <Toaster />
      </ThemeProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
