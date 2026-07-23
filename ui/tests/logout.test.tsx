import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";

import LogoutPage from "@/pages/logout";
import { isSignedIn, signIn, signOut } from "@/shared/auth";
import { fetchMock, mockJsonResponse, resetFetchMock } from "./support/mock-fetch";

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

async function mountLogout() {
  cleanupApp("app");
  document.body.innerHTML = '<div id="app"></div>';
  window.history.pushState({}, "", "/logout");
  const root = document.getElementById("app");
  if (!root) {
    throw new Error("Missing test app root");
  }

  await createSPA({
    root,
    routes: [
      { path: "/logout", handler: () => <LogoutPage /> },
      { path: "/login", handler: () => <div>signed out</div> },
    ],
  });
  await flushUi();
  return root;
}

function signOutButton(root: Element) {
  const button = Array.from(root.querySelectorAll("button")).find((candidate) =>
    candidate.textContent?.includes("Sign out"),
  );
  if (!(button instanceof HTMLButtonElement)) {
    throw new Error("Missing sign out button");
  }
  return button;
}

afterEach(() => {
  vi.clearAllMocks();
  cleanupApp("app");
  document.body.innerHTML = "";
  signOut();
  resetFetchMock();
});

describe("logout page", () => {
  it("should_match_the_login_page_card_layout", async () => {
    // Arrange
    signIn("admin", "password");

    // Act
    const root = await mountLogout();

    // Assert
    expect(root.querySelector("main.cassie-login-page")).not.toBeNull();
    expect(root.querySelector(".cassie-login-panel")).not.toBeNull();
    expect(root.querySelector(".cassie-login-card")).not.toBeNull();
    expect(root.querySelector(".cassie-brand-logo")).not.toBeNull();
    expect(root.querySelector("h1")?.textContent).toBe("Sign out of Cassie Admin?");
    expect(root.textContent).toContain("You’re signed in as admin.");
    expect(root.textContent).not.toContain("server-backed session");
    expect(root.textContent).not.toContain("clears its cookie");
    expect(root.querySelector('[data-slot="card-footer"]')).toBeNull();
  });

  it("should_revoke_the_session_before_returning_to_login", async () => {
    // Arrange
    signIn("admin", "password");
    mockJsonResponse("/api/v1/auth/logout", { logged_out: true }, { method: "POST" });
    const root = await mountLogout();

    // Act
    signOutButton(root).click();
    await waitForText(root, "signed out");

    // Assert
    expect(fetchMock).toHaveBeenCalledOnce();
    expect(window.location.pathname).toBe("/login");
    expect(isSignedIn()).toBe(false);
  });

  it("should_keep_the_session_available_for_retry_given_a_failed_revoke", async () => {
    // Arrange
    signIn("admin", "password");
    mockJsonResponse(
      "/api/v1/auth/logout",
      { error: "authentication unavailable" },
      { method: "POST", status: 503 },
    );
    const root = await mountLogout();

    // Act
    signOutButton(root).click();
    await waitForText(root, "Sign out failed");

    // Assert
    expect(window.location.pathname).toBe("/logout");
    expect(isSignedIn()).toBe(true);
    expect(root.textContent).toContain("authentication unavailable");
    expect(signOutButton(root).disabled).toBe(false);
  });

  it("should_finish_local_sign_out_given_an_already_expired_server_session", async () => {
    // Arrange
    signIn("admin", "password");
    mockJsonResponse(
      "/api/v1/auth/logout",
      { error: "unauthorized" },
      { method: "POST", status: 401 },
    );
    const root = await mountLogout();

    // Act
    signOutButton(root).click();
    await waitForText(root, "signed out");

    // Assert
    expect(window.location.pathname).toBe("/login");
    expect(isSignedIn()).toBe(false);
  });
});
