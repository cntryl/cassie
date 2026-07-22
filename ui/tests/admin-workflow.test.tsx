import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";
import { clearRoutes, getManifest } from "@askrjs/askr/router";

import { registerRootRoutes } from "@/pages/_routes";
import { isSignedIn, signOut } from "@/shared/auth";

async function flushUi() {
  await new Promise<void>((resolve) => queueMicrotask(() => resolve()));
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

async function waitFor(root: Element, predicate: () => boolean, label: string) {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    await flushUi();
    if (predicate()) {
      return;
    }
  }

  throw new Error(`Timed out waiting for ${label}. Current text: ${root.textContent ?? ""}`);
}

function typeInto(input: HTMLInputElement, value: string) {
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function buttonByText(root: Element, text: string) {
  const button = Array.from(root.querySelectorAll("button")).find((candidate) =>
    candidate.textContent?.includes(text),
  );
  if (!(button instanceof HTMLButtonElement)) {
    throw new Error(`Missing button with text ${text}`);
  }
  return button;
}

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function installWorkflowApi() {
  const requests: string[] = [];
  const mock = vi.fn(async (request: Request) => {
    const path = new URL(request.url).pathname;
    const key = `${request.method} ${path}`;
    requests.push(key);

    if (key === "GET /api/v1/auth/session") {
      return json({ error: "unauthorized" }, 401);
    }
    if (key === "POST /api/v1/auth/login") {
      return json({ user: "admin", role: "admin" });
    }
    if (key === "GET /api/v1/admin/databases") {
      return json([{ name: "analytics" }, { name: "postgres" }]);
    }
    if (key === "GET /api/v1/admin/catalog") {
      return json({ sections: [] });
    }
    if (key === "POST /api/v1/admin/query-executions") {
      return json({
        columns: [
          {
            name: "ready",
            data_type: "int8",
            type_oid: 20,
            typlen: 8,
            atttypmod: -1,
            format_code: 0,
            nullable: false,
          },
        ],
        rows: [[1]],
        command: "SELECT",
      });
    }
    if (key === "POST /api/v1/auth/logout") {
      return json({ logged_out: true });
    }

    throw new Error(`Unexpected workflow request: ${key}`);
  });
  vi.stubGlobal("fetch", mock);
  return requests;
}

async function mountWorkflow() {
  cleanupApp("app");
  clearRoutes();
  registerRootRoutes();
  document.body.innerHTML = '<div id="app"></div>';
  window.history.pushState({}, "", "/login");
  const root = document.getElementById("app");
  if (!root) {
    throw new Error("Missing test app root");
  }

  await import("@/pages/app/query");
  await createSPA({ root, manifest: getManifest() });
  await flushUi();
  return root;
}

afterEach(() => {
  cleanupApp("app");
  clearRoutes();
  document.body.innerHTML = "";
  signOut();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe("admin UI workflow", () => {
  it("should_complete_login_query_and_logout_through_the_registered_routes", async () => {
    // Arrange
    window.localStorage.clear();
    const requests = installWorkflowApi();
    const root = await mountWorkflow();
    const username = root.querySelector("#login-username") as HTMLInputElement;
    const password = root.querySelector("#login-password") as HTMLInputElement;

    // Act: authenticate, then create a database-owned query tab.
    typeInto(username, "admin");
    typeInto(password, "pwd123");
    buttonByText(root, "Sign in").click();
    await waitFor(
      root,
      () => root.textContent?.includes("Choose a database") === true,
      "empty query page",
    );
    buttonByText(root, "New Query").click();
    await waitFor(
      root,
      () => root.textContent?.includes("analytics") === true,
      "database selector",
    );
    buttonByText(root, "analytics").click();
    await waitFor(root, () => root.querySelector("[data-query-page]") !== null, "query page");

    // Assert
    expect(window.location.pathname).toBe("/");
    expect(root.textContent).toContain("analytics");
    expect(isSignedIn()).toBe(true);

    // Act: run the default query and inspect its result.
    buttonByText(root, "Run").click();
    await waitFor(root, () => root.textContent?.includes("1 row") === true, "query result");

    // Assert
    expect(root.textContent).toContain("SELECT");
    expect(root.textContent).toContain("1 row");

    // Act: revoke the session through the real logout route.
    const logoutLink = root.querySelector('a[aria-label="Sign out"]');
    if (!(logoutLink instanceof HTMLElement)) {
      throw new Error("Missing logout link");
    }
    logoutLink.click();
    await waitFor(root, () => window.location.pathname === "/logout", "logout page");
    buttonByText(root, "Sign out").click();
    await waitFor(root, () => window.location.pathname === "/login", "login page");

    // Assert
    expect(isSignedIn()).toBe(false);
    expect(root.querySelector("#login-username")).toBeTruthy();
    expect(requests).toContain("POST /api/v1/auth/login");
    expect(requests).toContain("GET /api/v1/admin/catalog");
    expect(requests).toContain("POST /api/v1/admin/query-executions");
    expect(requests).toContain("POST /api/v1/auth/logout");
  });
});
