import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import path from "node:path";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { "@": path.resolve(__dirname, "src") },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./tests/setupGlobals.ts", "./tests/setupTests.ts"],
    include: ["tests/**/*.test.{ts,tsx}"],
    // 默认 5s 对首次渲染偏紧，给一点余量。
    // Radix 浮层过去在 jsdom 里要卡几秒到几十秒，根因与绕过办法见 tests/setupGlobals.ts。
    testTimeout: 10_000,
    coverage: { reporter: ["text", "lcov"] },
  },
});
