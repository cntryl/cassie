import { askr } from "@askrjs/vite";
import autoprefixer from "autoprefixer";
import { defineConfig } from "vite-plus";

import { mockAdminQueryApiPlugin } from "./dev/mock-admin-query-api";

const useMockApi = process.env.VITE_MOCK_API === "true";

function fileUrlPath(path: string) {
  return decodeURIComponent(new URL(path, import.meta.url).pathname).replace(
    /^\/([A-Za-z]:\/)/,
    "$1",
  );
}

const srcDir = fileUrlPath("./src");

const cassieBackend = "http://localhost:8080";

const config = {
  fmt: {},
  lint: {
    ignorePatterns: ["dist/**", "coverage/**"],
    options: {
      typeAware: true,
      typeCheck: true,
    },
  },
  plugins: [
    askr({
      optimizeTemplates: true,
    }),
    ...(useMockApi ? [mockAdminQueryApiPlugin()] : []),
  ],
  css: {
    transformer: "postcss" as const,
    postcss: {
      plugins: [autoprefixer()],
    },
  },
  base: "/",
  define: {
    "process.env": {},
  },
  server: {
    port: 5174,
    open: true,
    proxy: {
      "/api": {
        target: cassieBackend,
        // Preserve the browser-facing Host header so Cassie's same-origin
        // state-change check sees the same authority in Host and Origin.
        changeOrigin: false,
      },
      "/health": {
        target: cassieBackend,
        changeOrigin: true,
      },
      "/liveness": {
        target: cassieBackend,
        changeOrigin: true,
      },
      "/targetz": {
        target: cassieBackend,
        changeOrigin: true,
      },
      "/metrics": {
        target: cassieBackend,
        changeOrigin: true,
      },
    },
  },
  resolve: {
    dedupe: ["@askrjs/askr", "@askrjs/ui"],
    alias: [
      {
        find: "@",
        replacement: srcDir,
      },
    ],
  },
  build: {
    sourcemap: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./tests/support/test-environment.ts"],
    exclude: ["**/dist/**", "**/node_modules/**", "**/tests/e2e/**", "**/tests/e2e-mock/**"],
    globals: true,
    coverage: {
      reporter: ["text", "json", "html"],
    },
  },
};

export default defineConfig(config);
