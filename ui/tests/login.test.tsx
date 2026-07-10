import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";
import type { FetchResponse } from "@fgrzl/fetch";

import LoginPage from "@/pages/login";
import { apiv1, type QuerySchemaResponse } from "@/adapters";
import { isSignedIn, signOut } from "@/shared/auth";

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

function mockCatalogUnauthorized() {
  const response: FetchResponse<QuerySchemaResponse> = {
    ok: false,
    status: 401,
    statusText: "Unauthorized",
    headers: new Headers(),
    url: "/api/v1/admin/catalog",
    data: null,
    error: { message: "Invalid username or password.", status: 401, statusText: "Unauthorized", url: "" },
  };

  vi.spyOn(apiv1, "listAdminCatalog").mockResolvedValue(response);
}

async function flushUi() {
  await new Promise<void>((resolve) => queueMicrotask(() => resolve()));
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

async function waitForText(root: Element, text: string) {
  for (let attempt = 0; attempt < 10; attempt += 1) {
    await flushUi();
    if (root.textContent?.includes(text)) {
      return;
    }
  }

  throw new Error(`Timed out waiting for text ${text}. Current text: ${root.textContent ?? ""}`);
}

function typeInto(input: HTMLInputElement, value: string) {
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

async function mountLogin() {
  cleanupApp("app");
  document.body.innerHTML = '<div id="app"></div>';
  window.history.pushState({}, "", "/login");

  const root = document.getElementById("app");
  if (!root) {
    throw new Error("Missing test app root");
  }

  await createSPA({
    root,
    routes: [
      { path: "/login", handler: () => <LoginPage /> },
      { path: "/", handler: () => <div data-testid="home-stub">home</div> },
    ],
  });
  await flushUi();
  return root;
}

afterEach(() => {
  vi.clearAllMocks();
  cleanupApp("app");
  document.body.innerHTML = "";
  signOut();
});

describe("login page", () => {
  it("keeps typed characters in the username and password fields", async () => {
    const root = await mountLogin();
    const usernameInput = root.querySelector("#login-username") as HTMLInputElement;
    const passwordInput = root.querySelector("#login-password") as HTMLInputElement;

    typeInto(usernameInput, "admin");
    typeInto(passwordInput, "pwd123");
    await flushUi();

    expect(usernameInput.value).toBe("admin");
    expect(passwordInput.value).toBe("pwd123");
  });

  it("signs in and navigates to / when the credential is accepted", async () => {
    mockCatalogSuccess();
    const root = await mountLogin();
    const usernameInput = root.querySelector("#login-username") as HTMLInputElement;
    const passwordInput = root.querySelector("#login-password") as HTMLInputElement;
    const submitButton = root.querySelector('button[type="submit"]') as HTMLButtonElement;

    typeInto(usernameInput, "admin");
    typeInto(passwordInput, "pwd123");
    await flushUi();
    submitButton.click();

    await flushUi();
    await flushUi();

    expect(apiv1.listAdminCatalog).toHaveBeenCalled();
    expect(window.location.pathname).toBe("/");
    expect(isSignedIn()).toBe(true);
  });

  it("shows an inline error and stays on /login when the credential is rejected", async () => {
    mockCatalogUnauthorized();
    const root = await mountLogin();
    const usernameInput = root.querySelector("#login-username") as HTMLInputElement;
    const passwordInput = root.querySelector("#login-password") as HTMLInputElement;
    const submitButton = root.querySelector('button[type="submit"]') as HTMLButtonElement;

    typeInto(usernameInput, "admin");
    typeInto(passwordInput, "wrong");
    await flushUi();
    submitButton.click();

    await waitForText(root, "Sign in failed");

    expect(window.location.pathname).toBe("/login");
    expect(isSignedIn()).toBe(false);
  });
});
