import type { IncomingMessage, ServerResponse } from "node:http";
import type { QueryExplainResponse, QuerySchemaResponse } from "@/adapters";

const MOCK_USER = "admin";
const MOCK_PASSWORD = "pwd123";
const MOCK_SESSION_COOKIE = "cassie_session";
const MOCK_SESSION_TOKEN = "mock-session";

interface MockSession {
  user: string;
  role: string;
}

function sendUnauthorized(res: ServerResponse) {
  res.statusMessage = "Unauthorized";
  sendJson(res, 401, { error: "unauthorized" });
}

function requestCookie(req: IncomingMessage, name: string) {
  const raw = req.headers.cookie;
  if (!raw) {
    return null;
  }

  for (const part of raw.split(";")) {
    const [cookieName, ...valueParts] = part.trim().split("=");
    if (cookieName === name) {
      return valueParts.join("=");
    }
  }

  return null;
}

function hasMockSession(req: IncomingMessage, session: MockSession | null) {
  return session !== null && requestCookie(req, MOCK_SESSION_COOKIE) === MOCK_SESSION_TOKEN;
}

function setSessionCookie(res: ServerResponse, clear: boolean) {
  res.setHeader(
    "set-cookie",
    clear
      ? `${MOCK_SESSION_COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict`
      : `${MOCK_SESSION_COOKIE}=${MOCK_SESSION_TOKEN}; Path=/; HttpOnly; SameSite=Strict`,
  );
}

function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    let data = "";
    req.on("data", (chunk) => {
      data += chunk;
    });
    req.on("end", () => resolve(data));
    req.on("error", reject);
  });
}

function sendJson(res: ServerResponse, status: number, body: unknown) {
  res.statusCode = status;
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(body));
}

function requestPath(req: IncomingMessage) {
  return new URL(req.url ?? "/", "http://cassie.mock").pathname;
}

function column(name: string) {
  return {
    name,
    data_type: "text",
    type_oid: 25,
    typlen: -1,
    atttypmod: -1,
    format_code: 0,
    nullable: true,
  };
}

function schemaItem(
  kind: "table" | "view" | "index" | "udf" | "procedure",
  canonicalName: string,
  metadata: string,
) {
  const [database, schema, name] = canonicalName.split(".");
  const columns =
    kind === "table" || kind === "view"
      ? ["id", "name"].map((columnName) => ({
          id: `column:${canonicalName}:${columnName}`,
          name: columnName,
          data_type: columnName === "id" ? "uuid" : "text",
          primary_key: kind === "table" && columnName === "id",
        }))
      : [];
  return {
    id: `${kind}:${canonicalName}`,
    kind,
    label: canonicalName,
    database,
    schema,
    name,
    columns,
    metadata,
  };
}

const mockSchema = {
  sections: [
    {
      id: "tables",
      label: "Tables",
      items: [
        schemaItem("table", "postgres.public.documents", "4 columns"),
        schemaItem("table", "postgres.public.accounts", "6 columns"),
        schemaItem("table", "postgres.reporting.monthly_totals", "5 columns"),
      ],
    },
    {
      id: "views",
      label: "Views",
      items: [
        schemaItem("view", "postgres.public.active_docs", "3 columns"),
        schemaItem("view", "postgres.public.daily_active_users", "2 columns"),
      ],
    },
    {
      id: "indexes",
      label: "Indexes",
      items: [
        schemaItem("index", "postgres.public.idx_id", "scalar on postgres.public.documents(id)"),
        schemaItem(
          "index",
          "postgres.public.idx_email",
          "unique on postgres.public.accounts(email)",
        ),
      ],
    },
    {
      id: "udfs",
      label: "UDFs",
      items: [
        schemaItem("udf", "postgres.public.calculate_total", "(account_id uuid) -> numeric"),
        schemaItem("udf", "postgres.public.event_count", "(since timestamptz) -> bigint"),
      ],
    },
    {
      id: "procedures",
      label: "Procedures",
      items: [schemaItem("procedure", "postgres.public.archive_old_documents", "()")],
    },
  ],
} satisfies QuerySchemaResponse;

function mockSchemaForDatabase(database: string): QuerySchemaResponse {
  return {
    sections: mockSchema.sections.map((section) => ({
      ...section,
      items: section.items.map((item) => {
        const label = item.label.replace(/^postgres\./, `${database}.`);
        return {
          ...item,
          id: item.id.replace(/:postgres\./, `:${database}.`),
          label,
          database,
          columns: item.columns.map((column) => ({
            ...column,
            id: column.id.replace(/:postgres\./, `:${database}.`),
          })),
        };
      }),
    })),
  };
}

function mockExecuteResult(sql: string) {
  return {
    columns: [column("id"), column("name"), column("owner"), column("notes")],
    command: sql.trim().toUpperCase().startsWith("SELECT") ? "SELECT" : "SELECT",
    rows: [
      ["doc-1", "Document One", "alice", null],
      ["doc-2", "Document Two", null, "NULL"],
      ["doc-3", "Document Three", "carol", "reviewed"],
      ["doc-4", "Document Four", "dave", "pending"],
    ],
  };
}

const mockExplainPlan = {
  format_version: 1,
  summary: {
    collection: "postgres.public.documents",
    root_operator: "Select",
    access_path: "index_seek",
    selected_index: "postgres.public.idx_id",
    selected_cost: 4,
    estimated_rows: 1,
    storage_mode: "row",
  },
  nodes: [
    {
      id: "read",
      label: "Read with idx_id",
      kind: "read",
      detail: "postgres.public.documents via index_seek",
      status: "optimized",
      badges: ["index:idx_id", "predicate pushdown", "projection pruning"],
      metrics: [
        { label: "estimated rows", value: "1" },
        { label: "selected cost", value: "4" },
        { label: "storage", value: "row" },
      ],
    },
    {
      id: "project",
      label: "Project rows",
      kind: "project",
      detail: "narrow",
      status: "active",
      badges: ["field:id", "field:name"],
      metrics: [{ label: "scan fields", value: "id, name" }],
    },
  ],
  attributes: [
    { label: "Access path", value: "index_seek", intent: "success" },
    { label: "Index", value: "idx_id", intent: "success" },
    { label: "Top K", value: "disabled", intent: "neutral" },
    { label: "Storage", value: "row", intent: "neutral" },
  ],
  estimates: {
    scan_rows: 230,
    index_rows: 1,
    join_rows: 0,
    search_rows: 0,
    vector_rows: 0,
    aggregate_rows: 0,
    scan_cost: 230,
    index_cost: 4,
    selected_cost: 4,
    cost_source: "mock",
    rejected_alternatives: ["full_scan"],
  },
  features: [
    {
      id: "predicate_pushdown",
      label: "Predicate pushdown",
      enabled: true,
      intent: "success",
      detail: "Filters applied before rows leave storage",
      node_id: "read",
    },
    {
      id: "projection_pruning",
      label: "Projection pruning",
      enabled: true,
      intent: "success",
      detail: "Read path narrows scanned fields when possible",
      node_id: "read",
    },
    {
      id: "covered_index",
      label: "Covered index",
      enabled: false,
      intent: "neutral",
      detail: "Selected index satisfies the requested projection",
      node_id: "read",
    },
    {
      id: "top_k",
      label: "Top K",
      enabled: false,
      intent: "neutral",
      detail: "Ordering and limit can stop early",
      node_id: "top_k",
    },
  ],
  diagnostics: {
    access_path_reason: "scalar-index-seek",
    fallback_reason: "none",
    pagination_strategy: "none",
    early_stop: "none",
    projection_shape: "narrow",
    operator_feedback_state: "stable",
    operator_feedback_reason: "mock data",
    adaptive_enabled: false,
    adaptive_decision_point: "none",
    adaptive_candidates: [],
    adaptive_selected_alternative: "none",
    adaptive_reason: "none",
    join_strategy: "none",
    join_fallback_reason: "none",
    rollup_rewrite: "none",
    projection_freshness: "current",
  },
} satisfies QueryExplainResponse["plan"];

function mockExplainResult(): QueryExplainResponse {
  return {
    columns: [column("QUERY PLAN")],
    command: "EXPLAIN",
    plan: mockExplainPlan,
    rows: [
      [
        "Index Scan using idx_id on documents  (cost=0.00..4.00 rows=1 width=64)\n" +
          "  Index Cond: (id IS NOT NULL)\n" +
          "  Planning Time: 0.045 ms\n" +
          "  Execution Time: 0.612 ms",
      ],
    ],
  };
}

export type MockAdminQueryMiddleware = (
  req: IncomingMessage,
  res: ServerResponse,
  next: () => void,
) => Promise<void>;

interface MockDevServer {
  middlewares: {
    use(middleware: MockAdminQueryMiddleware): void;
  };
}

/** Cookie-authenticated mock of the same REST workflow used by the built UI. */
export function createMockAdminQueryMiddleware(): MockAdminQueryMiddleware {
  let session: MockSession | null = null;

  return async (req, res, next) => {
    const path = requestPath(req);

    if (req.method === "POST" && path === "/api/v1/auth/login") {
      let credentials: Record<string, unknown>;
      try {
        credentials = JSON.parse((await readBody(req)) || "{}") as Record<string, unknown>;
      } catch {
        sendJson(res, 400, { error: "invalid login request" });
        return;
      }

      if (credentials.username !== MOCK_USER || credentials.password !== MOCK_PASSWORD) {
        sendUnauthorized(res);
        return;
      }

      session = {
        user: MOCK_USER,
        role: "admin",
      };
      setSessionCookie(res, false);
      sendJson(res, 200, session);
      return;
    }

    if (req.method === "GET" && path === "/api/v1/auth/session") {
      if (!hasMockSession(req, session)) {
        sendUnauthorized(res);
        return;
      }

      sendJson(res, 200, session);
      return;
    }

    if (req.method === "POST" && path === "/api/v1/auth/logout") {
      if (!hasMockSession(req, session)) {
        sendUnauthorized(res);
        return;
      }

      session = null;
      setSessionCookie(res, true);
      sendJson(res, 200, { logged_out: true });
      return;
    }

    if (path.startsWith("/api/v1/admin/") && !hasMockSession(req, session)) {
      sendUnauthorized(res);
      return;
    }

    if (req.method === "GET" && path === "/api/v1/admin/databases") {
      sendJson(res, 200, [
        { name: "analytics", description: "Analytics and reporting" },
        { name: "postgres", description: "Primary application database" },
      ]);
      return;
    }

    if (req.method === "GET" && path === "/api/v1/admin/catalog") {
      const database = new URL(req.url ?? "/", "http://cassie.mock").searchParams.get("database");
      if (!database) {
        sendJson(res, 400, { error: "database is required" });
      } else if (!["analytics", "postgres"].includes(database)) {
        sendJson(res, 404, { error: `database '${database}' does not exist` });
      } else {
        sendJson(res, 200, mockSchemaForDatabase(database));
      }
      return;
    }

    if (req.method === "DELETE" && path.startsWith("/api/v1/admin/query-operations/")) {
      sendJson(res, 409, { error: "query operation already completed" });
      return;
    }

    if (req.method === "POST" && path === "/api/v1/admin/query-executions") {
      const body = await readBody(req);
      const sql = (JSON.parse(body || "{}") as { sql?: string }).sql ?? "";
      sendJson(res, 200, mockExecuteResult(sql));
      return;
    }

    if (req.method === "POST" && path === "/api/v1/admin/query-validations") {
      const body = await readBody(req);
      const sql = (JSON.parse(body || "{}") as { sql?: string }).sql ?? "";
      sendJson(res, 200, {
        valid: true,
        command: sql.trim().toUpperCase().startsWith("SELECT") ? "SELECT" : "SELECT",
        columns: [column("id"), column("name"), column("owner"), column("notes")],
      });
      return;
    }

    if (req.method === "POST" && path === "/api/v1/admin/query-explanations") {
      sendJson(res, 200, mockExplainResult());
      return;
    }

    next();
  };
}

/**
 * Serves canned responses for the admin Query page's REST endpoints so the UI
 * can be built and reviewed without a running Cassie backend. Enable with
 * `VITE_MOCK_API=true npm run dev` (see `npm run dev:mock`).
 */
export function mockAdminQueryApiPlugin() {
  return {
    name: "cassie:mock-admin-query-api",
    configureServer(server: MockDevServer) {
      server.middlewares.use(createMockAdminQueryMiddleware());
    },
  };
}
