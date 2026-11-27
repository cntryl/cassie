import { expect, test } from "@playwright/test";

const methodsPath = "**/api/v1/auth/methods";
const startPath = "**/api/v1/auth/oidc/start?*";
const returnTo = "/?database=analytics#results";
const redirectParams = new URLSearchParams({ next: returnTo });

test("should_automatically_start_oidc_as_a_full_browser_navigation", async ({ page }) => {
  // Arrange
  await page.route(methodsPath, (route) => route.fulfill({ json: { oidc: true } }));
  await page.route(startPath, (route) =>
    route.fulfill({ contentType: "text/html", body: "<h1>Identity provider</h1>" }),
  );
  const start = page.waitForRequest(startPath);

  // Act
  await page.goto(`/login?${redirectParams}`);

  // Assert
  const request = await start;
  expect(request.isNavigationRequest()).toBe(true);
  expect(new URL(request.url()).searchParams.get("next")).toBe(returnTo);
  await expect(page.getByRole("heading", { name: "Identity provider" })).toBeVisible();
});

for (const stopped of [
  { query: "oidc_error=1", heading: "Sign in failed", action: "Try again" },
  { query: "signed_out=1", heading: "Signed out", action: "Sign in" },
]) {
  test(`should_require_an_explicit_oidc_restart_after_${stopped.query}`, async ({ page }) => {
    // Arrange
    let starts = 0;
    await page.route(methodsPath, (route) => route.fulfill({ json: { oidc: true } }));
    await page.route(startPath, (route) => {
      starts += 1;
      return route.fulfill({ contentType: "text/html", body: "<h1>Identity provider</h1>" });
    });

    // Act
    await page.goto(`/login?${stopped.query}&${redirectParams}`);

    // Assert
    await expect(page.getByRole("heading", { name: stopped.heading })).toBeVisible();
    await expect(page.getByLabel("Password")).toHaveCount(0);
    expect(starts).toBe(0);

    const start = page.waitForRequest(startPath);
    await page.getByRole("button", { name: stopped.action, exact: true }).click();
    const request = await start;
    expect(request.isNavigationRequest()).toBe(true);
    expect(new URL(request.url()).searchParams.get("next")).toBe(returnTo);
    await expect(page.getByRole("heading", { name: "Identity provider" })).toBeVisible();
  });
}

test("should_restore_an_oidc_query_session_without_restarting_sign_in", async ({ page }) => {
  // Arrange
  let starts = 0;
  await page.route(methodsPath, (route) => route.fulfill({ json: { oidc: true } }));
  await page.route(startPath, (route) => {
    starts += 1;
    return route.abort();
  });
  await page.route("**/api/v1/auth/session", (route) =>
    route.fulfill({ json: { user: "oidc:test-subject", role: "oidc_reader" } }),
  );
  await page.route("**/api/v1/admin/databases", (route) =>
    route.fulfill({ json: [{ name: "analytics" }] }),
  );

  // Act
  await page.goto("/login");
  await page.getByRole("button", { name: "New Query" }).first().click();
  const databaseChoice = page.getByRole("dialog").getByRole("button", { name: "Database" });
  await databaseChoice.click();

  // Assert
  await expect(page.getByRole("option", { name: "analytics" })).toBeVisible();
  await expect(page.getByLabel("Password")).toHaveCount(0);
  expect(starts).toBe(0);
});

test("should_retry_session_lookup_without_restarting_oidc_when_it_is_unavailable", async ({
  page,
}) => {
  // Arrange
  let unavailable = true;
  let starts = 0;
  await page.route(methodsPath, (route) => route.fulfill({ json: { oidc: true } }));
  await page.route(startPath, (route) => {
    starts += 1;
    return route.abort();
  });
  await page.route("**/api/v1/auth/session", (route) =>
    unavailable
      ? route.fulfill({ status: 503, json: { error: "session lookup unavailable" } })
      : route.fulfill({ json: { user: "oidc:test-subject", role: "oidc_reader" } }),
  );
  await page.route("**/api/v1/admin/databases", (route) =>
    route.fulfill({ json: [{ name: "analytics" }] }),
  );
  await page.goto("/login");
  await expect(page.getByRole("heading", { name: "Could not verify your session" })).toBeVisible();
  unavailable = false;

  // Act
  await page.getByRole("button", { name: "Retry", exact: true }).click();

  // Assert
  await expect(page.getByText("Choose a database to open a query workspace.")).toBeVisible();
  expect(starts).toBe(0);
});
