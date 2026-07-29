import { afterEach, describe, expect, it } from "vite-plus/test";
import { createServer, type Server } from "node:http";

import { createMockAdminQueryMiddleware } from "../dev/mock-admin-query-api";

let server: Server | null = null;

async function startMockApi() {
  const middleware = createMockAdminQueryMiddleware();
  server = createServer((request, response) => {
    void middleware(request, response, () => {
      response.statusCode = 404;
      response.end();
    });
  });
  await new Promise<void>((resolve) => server?.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (address === null || typeof address === "string") {
    throw new Error("Missing mock API address");
  }
  return `http://127.0.0.1:${address.port}`;
}

function sessionCookie(response: Response) {
  const cookie = response.headers.get("set-cookie")?.split(";", 1)[0];
  if (!cookie) {
    throw new Error("Missing session cookie");
  }

  return cookie;
}

afterEach(async () => {
  if (server !== null) {
    await new Promise<void>((resolve, reject) => {
      server?.close((error) => (error ? reject(error) : resolve()));
    });
    server = null;
  }
});

describe("mock admin query API", () => {
  async function createSession(baseUrl: string) {
    const response = await fetch(`${baseUrl}/api/v1/auth/login`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ username: "root", password: "pwd123", database: "analytics" }),
    });

    if (!response.ok) {
      throw new Error(`Login failed: ${response.status}`);
    }

    return sessionCookie(response);
  }

  it("should_complete_the_cookie_session_query_and_logout_workflow", async () => {
    // Arrange
    const baseUrl = await startMockApi();
    const cookie = await createSession(baseUrl);

    // Act
    const session = await fetch(`${baseUrl}/api/v1/auth/session`, {
      headers: { cookie },
    });
    const catalog = await fetch(`${baseUrl}/api/v1/admin/catalog?database=analytics`, {
      headers: { cookie },
    });
    const execute = await fetch(`${baseUrl}/api/v1/admin/query-executions`, {
      method: "POST",
      headers: { "content-type": "application/json", cookie },
      body: JSON.stringify({
        database: "analytics",
        sql: "SELECT 1 AS ready;",
        operation_id: "operation-1",
      }),
    });
    const logout = await fetch(`${baseUrl}/api/v1/auth/logout`, {
      method: "POST",
      headers: { cookie },
    });
    const restoredAfterLogout = await fetch(`${baseUrl}/api/v1/auth/session`, {
      headers: { cookie },
    });

    // Assert
    expect(session.status).toBe(200);
    expect(await session.json()).toEqual({
      user: "root",
      role: "admin",
    });
    expect(catalog.status).toBe(200);
    expect((await execute.json()).command).toBe("SELECT");
    expect(logout.status).toBe(200);
    expect(restoredAfterLogout.status).toBe(401);
  });

  it("should_keep_newly_created_databases_queryable_for_all_client_workflows", async () => {
    // Arrange
    const baseUrl = await startMockApi();
    const cookie = await createSession(baseUrl);

    // Act
    const createDatabase = await fetch(`${baseUrl}/api/v1/admin/query-executions`, {
      method: "POST",
      headers: { "content-type": "application/json", cookie },
      body: JSON.stringify({
        database: "analytics",
        sql: "CREATE DATABASE reporting",
        operation_id: "op-create",
      }),
    });
    const validation = await fetch(`${baseUrl}/api/v1/admin/query-validations`, {
      method: "POST",
      headers: { "content-type": "application/json", cookie },
      body: JSON.stringify({
        database: "reporting",
        sql: "SELECT 1 AS ready;",
        operation_id: "op-validate",
      }),
    });
    const execution = await fetch(`${baseUrl}/api/v1/admin/query-executions`, {
      method: "POST",
      headers: { "content-type": "application/json", cookie },
      body: JSON.stringify({
        database: "reporting",
        sql: "SELECT 1 AS ready;",
        operation_id: "op-execute",
      }),
    });
    const explain = await fetch(`${baseUrl}/api/v1/admin/query-explanations`, {
      method: "POST",
      headers: { "content-type": "application/json", cookie },
      body: JSON.stringify({
        database: "reporting",
        sql: "SELECT 1 AS ready;",
        operation_id: "op-explain",
      }),
    });
    const catalog = await fetch(`${baseUrl}/api/v1/admin/catalog?database=reporting`, {
      headers: { cookie },
    });
    const databaseList = await fetch(`${baseUrl}/api/v1/admin/databases`, {
      headers: { cookie },
    });

    // Assert
    expect(createDatabase.status).toBe(200);
    expect((await createDatabase.json()).command).toBe("CREATE DATABASE");
    expect(validation.status).toBe(200);
    const validationPayload = await validation.json();
    expect(validationPayload.command).toBe("SELECT");
    expect(validationPayload.valid).toBe(true);
    expect(execution.status).toBe(200);
    expect((await execution.json()).command).toBe("SELECT");
    expect(explain.status).toBe(200);
    expect((await explain.json()).command).toBe("EXPLAIN");
    expect(catalog.status).toBe(200);
    expect(databaseList.status).toBe(200);
    expect(await databaseList.json()).toEqual(
      expect.arrayContaining([{ name: "reporting", description: "Created in this mock session" }]),
    );
  });

  it("should_reject_invalid_credentials_without_a_session_cookie", async () => {
    // Arrange
    const baseUrl = await startMockApi();

    // Act
    const response = await fetch(`${baseUrl}/api/v1/auth/login`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ username: "root", password: "wrong" }),
    });

    // Assert
    expect(response.status).toBe(401);
    expect(response.headers.get("set-cookie")).toBe(null);
  });
});
