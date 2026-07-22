import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";
import { clearRoutes, getManifest } from "@askrjs/askr/router";

import { registerRootRoutes } from "@/pages/_routes";
import { signIn, signOut } from "@/shared/auth";
import { mockJsonResponse, resetFetchMock } from "./support/mock-fetch";
import { saveQueryWorkspace } from "@/features/query/query-tabs";

function mockCatalogSuccess() {
  mockJsonResponse("/api/v1/admin/catalog", { sections: [] });
  mockJsonResponse("/api/v1/admin/databases", [{ name: "postgres" }]);
  saveQueryWorkspace("admin", {
    version: 1,
    activeTabId: "query-1",
    tabs: [
      {
        id: "query-1",
        ordinal: 1,
        title: "Query 1",
        database: "postgres",
        sql: "SELECT 1 AS ready;",
      },
    ],
  });
}

async function flushUi() {
  await new Promise<void>((resolve) => queueMicrotask(() => resolve()));
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

async function mountAt(path: string) {
  cleanupApp("app");
  // cleanupApp() tears down the registered route manifest along with the
  // mounted component tree, so each test needs a fresh registration —
  // registerRootRoutes() runs the same registerRoutes() call main.tsx's
  // "./pages/_routes" side-effect import triggers once in production.
  clearRoutes();
  registerRootRoutes();

  document.body.innerHTML = '<div id="app"></div>';
  window.history.pushState({}, "", path);

  const root = document.getElementById("app");
  if (!root) {
    throw new Error("Missing test app root");
  }

  // QueryPage is registered behind lazy(), so the dynamic import backing it
  // needs to resolve before the route renders — pre-warm the module cache so
  // lazy()'s synchronous check on first render doesn't race the import.
  await import("@/pages/app/query");

  await createSPA({ root, manifest: getManifest() });
  await flushUi();
  await flushUi();
  return root;
}

afterEach(() => {
  vi.clearAllMocks();
  cleanupApp("app");
  document.body.innerHTML = "";
  signOut();
  resetFetchMock();
});

beforeEach(() => {
  signOut();
});

describe("route-level auth guard", () => {
  it("redirects an unauthenticated visitor from / to /login", async () => {
    const root = await mountAt("/");

    expect(window.location.pathname).toBe("/login");
    expect(root.querySelector("#login-username")).toBeTruthy();
    expect(root.querySelector("[data-query-page]")).toBe(null);
  });

  it("renders the query page for a signed-in visitor at /", async () => {
    mockCatalogSuccess();
    signIn("admin", "secret");

    const root = await mountAt("/");

    expect(window.location.pathname).toBe("/");
    expect(root.querySelector("[data-query-page]")).toBeTruthy();
  });

  it("redirects a signed-in visitor away from /login back to /", async () => {
    mockCatalogSuccess();
    signIn("admin", "secret");

    const root = await mountAt("/login");

    expect(window.location.pathname).toBe("/");
    expect(root.querySelector("[data-query-page]")).toBeTruthy();
  });

  it("keeps an unauthenticated visitor on /login without redirecting", async () => {
    const root = await mountAt("/login");

    expect(window.location.pathname).toBe("/login");
    expect(root.querySelector("#login-username")).toBeTruthy();
  });

  it("should_render_a_recovery_page_given_an_unknown_route", async () => {
    // Arrange / Act
    const root = await mountAt("/missing-page");

    // Assert
    expect(window.location.pathname).toBe("/missing-page");
    expect(root.textContent).toContain("Page not found");
    expect(root.querySelector('a[href="/"]')).toBeTruthy();
  });
});
