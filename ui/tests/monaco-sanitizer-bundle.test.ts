// @vitest-environment node
import { fileURLToPath } from "node:url";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { build, optimizeDeps, resolveConfig, type Plugin } from "vite-plus";
import { expect, it } from "vite-plus/test";

import config from "../vite.config";

function assertQualifiedSanitizer(modules: string[], chunks: string[]) {
  expect(modules.some((id) => id.endsWith("/dompurify/dist/purify.es.mjs"))).toBe(true);
  expect(modules.some((id) => id.endsWith("/browser/dompurify/dompurify.js"))).toBe(false);
  const versions = [
    ...chunks.join("\n").matchAll(/\.version\s*=\s*["'`]([0-9]+\.[0-9]+\.[0-9]+)["'`]/g),
  ].map((match) => match[1]);
  expect(versions).toContain("3.4.17");
  expect(versions).not.toContain("3.4.8");
}

it("should_bundle_the_qualified_sanitizer_given_monaco_vendoring_when_production_builds", async () => {
  // Arrange
  const modules: string[] = [];
  const code: string[] = [];
  const witness: Plugin = {
    name: "sanitizer-bundle-witness",
    generateBundle(_options, bundle) {
      modules.push(...this.getModuleIds());
      for (const output of Object.values(bundle)) {
        if (output.type === "chunk") code.push(output.code);
      }
    },
  };

  // Act
  await build({
    ...config,
    configFile: false,
    publicDir: false,
    logLevel: "silent",
    plugins: [...(config.plugins ?? []), witness],
    build: {
      ...config.build,
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL(
            "../node_modules/monaco-editor/esm/vs/base/browser/domSanitize.js",
            import.meta.url,
          ),
        ),
        formats: ["es"],
      },
    },
  });

  // Assert
  assertQualifiedSanitizer(modules, code);
}, 15_000);

it("should_preserve_application_sanitizers_given_mirrored_paths_when_callers_are_not_monaco", async () => {
  // Arrange
  const fixture = await mkdtemp(join(tmpdir(), "cassie-sanitizer-isolation-"));
  const caller = join(fixture, "src/monaco-editor/esm/vs/base/browser");
  const modules: string[] = [];
  const code: string[] = [];
  const witness: Plugin = {
    name: "sanitizer-isolation-witness",
    generateBundle(_options, bundle) {
      modules.push(...this.getModuleIds());
      for (const output of Object.values(bundle)) {
        if (output.type === "chunk") code.push(output.code);
      }
    },
  };
  try {
    await mkdir(join(caller, "dompurify"), { recursive: true });
    await writeFile(
      join(caller, "dompurify/dompurify.js"),
      'export default { version: "fixture-owned-sanitizer" };',
    );
    await writeFile(
      join(caller, "domSanitize.js"),
      'import sanitizer from "./dompurify/dompurify.js"; export const version = sanitizer.version;',
    );

    // Act
    await build({
      ...config,
      configFile: false,
      publicDir: false,
      logLevel: "silent",
      plugins: [...(config.plugins ?? []), witness],
      build: {
        write: false,
        minify: false,
        lib: { entry: join(caller, "domSanitize.js"), formats: ["es"] },
      },
    });

    // Assert
    expect(code.join("\n")).toContain("fixture-owned-sanitizer");
    expect(modules.some((id) => id.endsWith("/dompurify/dist/purify.es.mjs"))).toBe(false);
  } finally {
    await rm(fixture, { recursive: true, force: true });
  }
}, 15_000);

it("should_reject_missing_audited_dependencies_given_monaco_vendoring_when_resolution_fails", async () => {
  // Arrange
  const fixture = await mkdtemp(join(tmpdir(), "cassie-sanitizer-missing-"));
  try {
    const sanitizer = fileURLToPath(
      new URL("../node_modules/monaco-editor/esm/vs/base/browser/domSanitize.js", import.meta.url),
    );

    // Act
    const result = build({
      ...config,
      configFile: false,
      publicDir: false,
      logLevel: "silent",
      resolve: {
        ...config.resolve,
        alias: [{ find: "dompurify", replacement: join(fixture, "missing-sanitizer.mjs") }],
      },
      build: { write: false, minify: false, lib: { entry: sanitizer, formats: ["es"] } },
    });

    // Assert
    await expect(result).rejects.toThrow(/missing-sanitizer\.mjs|audited DOMPurify/);
  } finally {
    await rm(fixture, { recursive: true, force: true });
  }
}, 15_000);

it("should_optimize_the_qualified_sanitizer_given_monaco_vendoring_when_development_starts", async () => {
  // Arrange
  const cache = await mkdtemp(join(tmpdir(), "cassie-sanitizer-optimizer-"));
  const modules: string[] = [];
  const witness: Plugin = {
    name: "sanitizer-optimizer-witness",
    generateBundle() {
      modules.push(...this.getModuleIds());
    },
  };
  try {
    const resolved = await resolveConfig(
      {
        ...config,
        configFile: false,
        cacheDir: cache,
        logLevel: "silent",
        optimizeDeps: {
          ...config.optimizeDeps,
          noDiscovery: true,
          rolldownOptions: {
            ...config.optimizeDeps?.rolldownOptions,
            plugins: [config.optimizeDeps?.rolldownOptions?.plugins, witness],
          },
          include: ["monaco-editor/esm/vs/base/browser/domSanitize.js"],
        },
      },
      "serve",
    );

    // Act
    const optimized = await optimizeDeps(resolved, true);
    const files = [...Object.values(optimized.optimized), ...Object.values(optimized.chunks)];
    const code = (await Promise.all(files.map((entry) => readFile(entry.file, "utf8")))).join("\n");

    // Assert
    assertQualifiedSanitizer(modules, [code]);
  } finally {
    await rm(cache, { recursive: true, force: true });
  }
}, 15_000);

it("should_bundle_the_qualified_sanitizer_given_monaco_vendoring_when_workers_build", async () => {
  // Arrange
  const fixture = await mkdtemp(join(tmpdir(), "cassie-sanitizer-worker-"));
  const modules: string[] = [];
  const code: string[] = [];
  const witness: Plugin = {
    name: "sanitizer-worker-witness",
    generateBundle(_options, bundle) {
      modules.push(...this.getModuleIds());
      for (const output of Object.values(bundle)) {
        if (output.type === "chunk") code.push(output.code);
      }
    },
  };
  try {
    const sanitizer = fileURLToPath(
      new URL("../node_modules/monaco-editor/esm/vs/base/browser/domSanitize.js", import.meta.url),
    );
    await writeFile(
      join(fixture, "worker.js"),
      `import { sanitizeHtml } from ${JSON.stringify(sanitizer)}; globalThis.fixtureSanitize = sanitizeHtml;`,
    );
    await writeFile(
      join(fixture, "entry.js"),
      'export function create() { return new Worker(new URL("./worker.js", import.meta.url), {type:"module"}); }',
    );

    // Act
    await build({
      ...config,
      configFile: false,
      publicDir: false,
      logLevel: "silent",
      worker: {
        ...config.worker,
        plugins: () => [...(config.worker?.plugins?.() ?? []), witness],
      },
      build: {
        ...config.build,
        write: false,
        minify: false,
        lib: { entry: join(fixture, "entry.js"), formats: ["es"] },
      },
    });

    // Assert
    assertQualifiedSanitizer(modules, code);
  } finally {
    await rm(fixture, { recursive: true, force: true });
  }
}, 15_000);
