import { askr } from "@askrjs/vite";
import autoprefixer from "autoprefixer";
import { defineConfig } from "vite-plus";
import type { UserConfig } from "@voidzero-dev/vite-plus-core";

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
  ],
  css: {
    transformer: "postcss",
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
      "/v1": {
        target: cassieBackend,
        changeOrigin: true,
      },
      "/health": {
        target: cassieBackend,
        changeOrigin: true,
      },
      "/liveness": {
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
    exclude: ["**/dist/**", "**/node_modules/**"],
    globals: true,
    coverage: {
      reporter: ["text", "json", "html"],
    },
  },
} as UserConfig;

export default defineConfig(config);
