import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";

import LoginPage from "@/pages/login";
import { isSignedIn, signOut } from "@/shared/auth";
import { fetchMock, mockJsonResponse, resetFetchMock } from "./support/mock-fetch";

function mockLoginSuccess() {
  mockJsonResponse("/api/v1/auth/login", { user: "admin", role: "admin" }, { method: "POST" });
}

function mockLoginUnauthorized() {
  mockJsonResponse(
    "/api/v1/auth/login",
    { error: "Invalid username or password." },
    { method: "POST", status: 401 },
  );
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
  resetFetchMock();
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
    mockLoginSuccess();
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

    expect(fetchMock).toHaveBeenCalledOnce();
    const request = fetchMock.mock.calls[0]?.[0];
    expect(await request?.json()).toEqual({ username: "admin", password: "pwd123" });
    expect(window.location.pathname).toBe("/");
    expect(isSignedIn()).toBe(true);
  });

  it("should_not_offer_a_database_control", async () => {
    const root = await mountLogin();
    expect(root.querySelector("#login-database")).toBeNull();
    expect(root.textContent).not.toContain("Database (optional)");
  });

  it("should_show_the_destroyer_style_inline_error_given_rejected_credentials", async () => {
    // Arrange
    mockLoginUnauthorized();
    const root = await mountLogin();
    const usernameInput = root.querySelector("#login-username") as HTMLInputElement;
    const passwordInput = root.querySelector("#login-password") as HTMLInputElement;
    const submitButton = root.querySelector('button[type="submit"]') as HTMLButtonElement;

    // Act
    typeInto(usernameInput, "admin");
    typeInto(passwordInput, "wrong");
    await flushUi();
    submitButton.click();

    await waitForText(root, "The username or password is incorrect.");

    // Assert
    expect(root.querySelector('[data-slot="field-error"]')?.textContent).toBe(
      "The username or password is incorrect.",
    );
    expect(root.querySelector('[data-slot="card-footer"]')).toBeNull();
    expect(window.location.pathname).toBe("/login");
    expect(isSignedIn()).toBe(false);
  });

  it("should_send_identity_credentials_only", async () => {
    mockLoginSuccess();
    const root = await mountLogin();
    typeInto(root.querySelector("#login-username") as HTMLInputElement, "admin");
    typeInto(root.querySelector("#login-password") as HTMLInputElement, "pwd123");
    (root.querySelector('button[type="submit"]') as HTMLButtonElement).click();
    await flushUi();
    expect(await fetchMock.mock.calls[0]?.[0].json()).toEqual({
      username: "admin",
      password: "pwd123",
    });
  });
});
