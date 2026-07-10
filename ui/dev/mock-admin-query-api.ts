import type { IncomingMessage, ServerResponse } from "node:http";
import type { ViteDevServer } from "@voidzero-dev/vite-plus-core";
import type { QueryExplainResponse, QuerySchemaResponse } from "@/adapters";

const MOCK_USER = "admin";
const MOCK_PASSWORD = "pwd123";

// Mirrors src/rest/router.rs's parse_rest_credentials/AUTH_TOKEN_PREFIX so the
// mock server exercises the same login flow (and failure mode) as the real
// backend: `Authorization: Bearer <user>:<password>`, or `Bearer <password>`
// alone to use the default mock user.
function isAuthorized(req: IncomingMessage): boolean {
  const raw = req.headers.authorization;
  if (!raw) {
    return false;
  }

  const token = raw.startsWith("Bearer ") ? raw.slice("Bearer ".length).trim() : "";
  if (!token) {
    return false;
  }

  const separatorIndex = token.indexOf(":");
  if (separatorIndex === -1) {
    return token === MOCK_PASSWORD;
  }

  const user = token.slice(0, separatorIndex).trim();
  const password = token.slice(separatorIndex + 1).trim();
  return user === MOCK_USER && password === MOCK_PASSWORD;
}

function sendUnauthorized(res: ServerResponse) {
  res.statusMessage = "Unauthorized";
  sendJson(res, 401, {
    message: "Invalid username or password.",
    status: 401,
  });
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

const mockSchema = {
  sections: [
    {
      id: "tables",
      label: "Tables",
      items: [
        {
          id: "table:postgres.public.documents",
          kind: "table",
          label: "postgres.public.documents",
          metadata: "4 columns",
        },
        {
          id: "table:postgres.public.accounts",
          kind: "table",
          label: "postgres.public.accounts",
          metadata: "6 columns",
        },
        {
          id: "table:postgres.reporting.monthly_totals",
          kind: "table",
          label: "postgres.reporting.monthly_totals",
          metadata: "5 columns",
        },
      ],
    },
    {
      id: "views",
      label: "Views",
      items: [
        {
          id: "view:postgres.public.active_docs",
          kind: "view",
          label: "postgres.public.active_docs",
          metadata: "3 columns",
        },
        {
          id: "view:postgres.public.daily_active_users",
          kind: "view",
          label: "postgres.public.daily_active_users",
          metadata: "2 columns",
        },
      ],
    },
    {
      id: "indexes",
      label: "Indexes",
      items: [
        {
          id: "index:postgres.public.idx_id",
          kind: "index",
          label: "postgres.public.idx_id",
          metadata: "scalar on postgres.public.documents(id)",
        },
        {
          id: "index:postgres.public.idx_email",
          kind: "index",
          label: "postgres.public.idx_email",
          metadata: "unique on postgres.public.accounts(email)",
        },
      ],
    },
    {
      id: "udfs",
      label: "UDFs",
      items: [
        {
          id: "udf:postgres.public.calculate_total",
          kind: "udf",
          label: "postgres.public.calculate_total",
          metadata: "(account_id uuid) -> numeric",
        },
        {
          id: "udf:postgres.public.event_count",
          kind: "udf",
          label: "postgres.public.event_count",
          metadata: "(since timestamptz) -> bigint",
        },
      ],
    },
    {
      id: "procedures",
      label: "Procedures",
      items: [
        {
          id: "procedure:postgres.public.archive_old_documents",
          kind: "procedure",
          label: "postgres.public.archive_old_documents",
          metadata: "()",
        },
      ],
    },
  ],
} satisfies QuerySchemaResponse;

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

/**
 * Serves canned responses for the admin Query page's REST endpoints so the UI
 * can be built and reviewed without a running Cassie backend. Enable with
 * `VITE_MOCK_API=true npm run dev` (see `npm run dev:mock`).
 */
export function mockAdminQueryApiPlugin() {
  return {
    name: "cassie:mock-admin-query-api",
    configureServer(server: ViteDevServer) {
      server.middlewares.use(async (req, res, next) => {
        const url = req.url ?? "";

        if (url.startsWith("/api/v1/admin/") && !isAuthorized(req)) {
          sendUnauthorized(res);
          return;
        }

        if (req.method === "GET" && url.startsWith("/api/v1/admin/catalog")) {
          sendJson(res, 200, mockSchema);
          return;
        }

        if (req.method === "POST" && url.startsWith("/api/v1/admin/query-executions")) {
          const body = await readBody(req);
          const sql = JSON.parse(body || "{}").sql ?? "";
          sendJson(res, 200, mockExecuteResult(sql));
          return;
        }

        if (req.method === "POST" && url.startsWith("/api/v1/admin/query-validations")) {
          const body = await readBody(req);
          const sql = JSON.parse(body || "{}").sql ?? "";
          sendJson(res, 200, {
            valid: true,
            command: sql.trim().toUpperCase().startsWith("SELECT") ? "SELECT" : "SELECT",
            columns: [column("id"), column("name"), column("owner"), column("notes")],
          });
          return;
        }

        if (req.method === "POST" && url.startsWith("/api/v1/admin/query-explanations")) {
          sendJson(res, 200, mockExplainResult());
          return;
        }

        next();
      });
    },
  };
}
