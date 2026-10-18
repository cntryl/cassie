import type { Plugin } from "vite-plus";

/** Resolve Monaco's vendored sanitizer to its audited npm dependency. */
export function monacoSanitizerPlugin(): Plugin {
  return {
    name: "cassie-monaco-sanitizer",
    enforce: "pre",
    resolveId: {
      order: "pre",
      async handler(source, importer) {
        if (source !== "./dompurify/dompurify.js" || !importer) return null;
        const caller = importer.split("?")[0]?.replace(/\\/g, "/");
        if (!caller?.endsWith("/node_modules/monaco-editor/esm/vs/base/browser/domSanitize.js")) {
          return null;
        }

        const resolved = await this.resolve("dompurify", importer, { skipSelf: true });
        if (!resolved) this.error("Monaco's audited DOMPurify dependency could not be resolved");
        return resolved;
      },
    },
  };
}
