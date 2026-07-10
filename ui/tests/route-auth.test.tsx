import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";
import { clearRoutes, getManifest } from "@askrjs/askr/router";
import type { FetchResponse } from "@fgrzl/fetch";

import { registerRootRoutes } from "@/pages/_routes";
import { apiv1, type QuerySchemaResponse } from "@/adapters";
import { signIn, signOut } from "@/shared/auth";

function mockCatalogSuccess() {
  const response: FetchResponse<QuerySchemaResponse> = {
    ok: true,
    status: 200,
    statusText: "OK",
    headers: new Headers(),
    url: "/api/v1/admin/catalog",
    data: { sections: [] },
    error: null,
  };

  vi.spyOn(apiv1, "listAdminCatalog").mockResolvedValue(response);
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
});
