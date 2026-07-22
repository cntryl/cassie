import { describe, expect, it } from "vite-plus/test";

import { shouldRedirectToLogin } from "@/adapters/client";

describe("API client authentication redirects", () => {
  it("should_keep_login_stable_given_an_unauthenticated_session_probe", () => {
    // Arrange
    const status = 401;

    // Act
    const shouldRedirect = shouldRedirectToLogin(status, "/api/v1/auth/session", "/login");

    // Assert
    expect(shouldRedirect).toBe(false);
  });

  it("should_redirect_to_login_given_an_expired_session_on_a_protected_route", () => {
    // Arrange
    const status = 401;

    // Act
    const shouldRedirect = shouldRedirectToLogin(status, "/api/v1/admin/query-executions", "/");

    // Assert
    expect(shouldRedirect).toBe(true);
  });
});
