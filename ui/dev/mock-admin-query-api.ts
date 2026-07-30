import type { IncomingMessage, ServerResponse } from "node:http";
import type { QueryExplainResponse, QuerySchemaResponse } from "@/adapters";

const MOCK_USER = "root";
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
  columnDefinitions: Array<[name: string, dataType: string, primaryKey?: boolean]> = [],
) {
  const [database, schema, name] = canonicalName.split(".");
  const columns =
    kind === "table" || kind === "view"
      ? columnDefinitions.map(([columnName, dataType, primaryKey = false]) => ({
          id: `column:${canonicalName}:${columnName}`,
          name: columnName,
          data_type: dataType,
          primary_key: primaryKey,
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

const database1Schema = {
  sections: [
    {
      id: "tables",
      label: "Tables",
      items: [
        schemaItem("table", "Database1.public.customers", "6 columns", [
          ["id", "uuid", true],
          ["email", "text"],
          ["display_name", "text"],
          ["status", "text"],
          ["created_at", "timestamptz"],
          ["updated_at", "timestamptz"],
        ]),
        schemaItem("table", "Database1.public.orders", "7 columns", [
          ["id", "uuid", true],
          ["customer_id", "uuid"],
          ["order_number", "text"],
          ["status", "text"],
          ["subtotal", "numeric"],
          ["tax", "numeric"],
          ["created_at", "timestamptz"],
        ]),
        schemaItem("table", "Database1.public.order_items", "6 columns", [
          ["id", "uuid", true],
          ["order_id", "uuid"],
          ["product_id", "uuid"],
          ["quantity", "integer"],
          ["unit_price", "numeric"],
          ["discount", "numeric"],
        ]),
        schemaItem("table", "Database1.public.products", "6 columns", [
          ["id", "uuid", true],
          ["sku", "text"],
          ["name", "text"],
          ["category", "text"],
          ["price", "numeric"],
          ["active", "boolean"],
        ]),
        schemaItem("table", "Database1.reporting.daily_sales", "5 columns", [
          ["day", "date", true],
          ["order_count", "bigint"],
          ["customer_count", "bigint"],
          ["gross_revenue", "numeric"],
          ["net_revenue", "numeric"],
        ]),
        schemaItem("table", "Database1.audit.change_log", "6 columns", [
          ["id", "uuid", true],
          ["actor_id", "uuid"],
          ["entity_type", "text"],
          ["entity_id", "uuid"],
          ["action", "text"],
          ["occurred_at", "timestamptz"],
        ]),
      ],
    },
    {
      id: "views",
      label: "Views",
      items: [
        schemaItem("view", "Database1.public.active_customers", "3 columns", [
          ["id", "uuid"],
          ["display_name", "text"],
          ["last_order_at", "timestamptz"],
        ]),
        schemaItem("view", "Database1.reporting.monthly_revenue", "3 columns", [
          ["month", "date"],
          ["order_count", "bigint"],
          ["revenue", "numeric"],
        ]),
        schemaItem("view", "Database1.reporting.top_products", "4 columns", [
          ["product_id", "uuid"],
          ["name", "text"],
          ["units_sold", "bigint"],
          ["revenue", "numeric"],
        ]),
      ],
    },
    {
      id: "indexes",
      label: "Indexes",
      items: [
        schemaItem("index", "Database1.public.customers_email_key", "unique on customers(email)"),
        schemaItem("index", "Database1.public.orders_customer_idx", "on orders(customer_id)"),
        schemaItem("index", "Database1.public.orders_created_idx", "on orders(created_at desc)"),
        schemaItem("index", "Database1.public.products_sku_key", "unique on products(sku)"),
      ],
    },
    {
      id: "udfs",
      label: "UDFs",
      items: [
        schemaItem("udf", "Database1.public.calculate_order_total", "(order_id uuid) -> numeric"),
        schemaItem(
          "udf",
          "Database1.reporting.customer_lifetime_value",
          "(customer_id uuid) -> numeric",
        ),
      ],
    },
    {
      id: "procedures",
      label: "Procedures",
      items: [
        schemaItem("procedure", "Database1.public.fulfill_order", "(order_id uuid)"),
        schemaItem("procedure", "Database1.audit.purge_change_log", "(before timestamptz)"),
      ],
    },
  ],
} satisfies QuerySchemaResponse;

const database2Schema = {
  sections: [
    {
      id: "tables",
      label: "Tables",
      items: [
        schemaItem("table", "Database2.inventory.warehouses", "5 columns", [
          ["id", "uuid", true],
          ["code", "text"],
          ["name", "text"],
          ["region", "text"],
          ["active", "boolean"],
        ]),
        schemaItem("table", "Database2.inventory.stock_items", "7 columns", [
          ["id", "uuid", true],
          ["warehouse_id", "uuid"],
          ["sku", "text"],
          ["on_hand", "integer"],
          ["reserved", "integer"],
          ["reorder_point", "integer"],
          ["updated_at", "timestamptz"],
        ]),
        schemaItem("table", "Database2.inventory.stock_movements", "7 columns", [
          ["id", "uuid", true],
          ["stock_item_id", "uuid"],
          ["movement_type", "text"],
          ["quantity", "integer"],
          ["reference", "text"],
          ["actor_id", "uuid"],
          ["occurred_at", "timestamptz"],
        ]),
        schemaItem("table", "Database2.support.accounts", "5 columns", [
          ["id", "uuid", true],
          ["name", "text"],
          ["tier", "text"],
          ["owner_id", "uuid"],
          ["created_at", "timestamptz"],
        ]),
        schemaItem("table", "Database2.support.tickets", "8 columns", [
          ["id", "uuid", true],
          ["account_id", "uuid"],
          ["subject", "text"],
          ["priority", "text"],
          ["status", "text"],
          ["assignee_id", "uuid"],
          ["opened_at", "timestamptz"],
          ["closed_at", "timestamptz"],
        ]),
        schemaItem("table", "Database2.support.ticket_events", "6 columns", [
          ["id", "uuid", true],
          ["ticket_id", "uuid"],
          ["event_type", "text"],
          ["body", "text"],
          ["author_id", "uuid"],
          ["created_at", "timestamptz"],
        ]),
      ],
    },
    {
      id: "views",
      label: "Views",
      items: [
        schemaItem("view", "Database2.inventory.low_stock", "4 columns", [
          ["warehouse", "text"],
          ["sku", "text"],
          ["available", "integer"],
          ["reorder_point", "integer"],
        ]),
        schemaItem("view", "Database2.support.open_ticket_queue", "5 columns", [
          ["ticket_id", "uuid"],
          ["account_name", "text"],
          ["subject", "text"],
          ["priority", "text"],
          ["age_hours", "integer"],
        ]),
        schemaItem("view", "Database2.support.sla_summary", "4 columns", [
          ["account_id", "uuid"],
          ["open_tickets", "bigint"],
          ["breached_tickets", "bigint"],
          ["average_resolution_hours", "numeric"],
        ]),
      ],
    },
    {
      id: "indexes",
      label: "Indexes",
      items: [
        schemaItem("index", "Database2.inventory.stock_items_sku_idx", "on stock_items(sku)"),
        schemaItem(
          "index",
          "Database2.inventory.movements_time_idx",
          "on stock_movements(occurred_at desc)",
        ),
        schemaItem("index", "Database2.support.tickets_status_idx", "on tickets(status, priority)"),
        schemaItem(
          "index",
          "Database2.support.ticket_events_ticket_idx",
          "on ticket_events(ticket_id)",
        ),
      ],
    },
    {
      id: "udfs",
      label: "UDFs",
      items: [
        schemaItem(
          "udf",
          "Database2.inventory.available_quantity",
          "(stock_item_id uuid) -> integer",
        ),
        schemaItem("udf", "Database2.support.sla_deadline", "(ticket_id uuid) -> timestamptz"),
      ],
    },
    {
      id: "procedures",
      label: "Procedures",
      items: [
        schemaItem("procedure", "Database2.inventory.rebalance_stock", "(sku text)"),
        schemaItem("procedure", "Database2.support.escalate_overdue_tickets", "()"),
      ],
    },
  ],
} satisfies QuerySchemaResponse;

function mockSchemaForDatabase(database: string): QuerySchemaResponse {
  if (database === "Database1") return database1Schema;
  if (database === "Database2") return database2Schema;
  return { sections: [] };
}

function mockExecuteResult(database: string, sql: string) {
  if (sql.trim().toUpperCase().startsWith("CREATE DATABASE ")) {
    return { columns: [], command: "CREATE DATABASE", rows: [] };
  }
  return {
    columns: [column("id"), column("name"), column("owner"), column("notes")],
    command: sql.trim().toUpperCase().startsWith("SELECT") ? "SELECT" : "SELECT",
    rows: [
      [
        database === "Database1" ? "order-1" : "ticket-1",
        database === "Database1" ? "Order 1001" : "Login issue",
        "alice",
        null,
      ],
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

function mockExplainResult(database: string): QueryExplainResponse {
  return {
    columns: [column("QUERY PLAN")],
    command: "EXPLAIN",
    plan: {
      ...mockExplainPlan,
      summary: {
        ...mockExplainPlan.summary,
        collection:
          database === "Database1" ? "Database1.public.orders" : "Database2.support.tickets",
      },
    },
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
  const databases = [
    { name: "Database1", description: "Commerce, customers, orders, and reporting" },
    { name: "Database2", description: "Inventory, warehouse, and customer support operations" },
  ];
  const operations = new Map<string, { completed: boolean; cancelled: boolean }>();

  async function queryBody(req: IncomingMessage, res: ServerResponse) {
    const body = JSON.parse((await readBody(req)) || "{}") as {
      database?: string;
      sql?: string;
      operation_id?: string;
    };
    if (!body.database || !databases.some((candidate) => candidate.name === body.database)) {
      sendJson(res, 400, { error: "a valid database is required" });
      return null;
    }
    if (!body.operation_id) {
      sendJson(res, 400, { error: "operation_id is required" });
      return null;
    }
    return body;
  }

  async function completeOperation(operationId: string, res: ServerResponse, result: unknown) {
    const operation = { completed: false, cancelled: false };
    operations.set(operationId, operation);
    await new Promise<void>((resolve) => setTimeout(resolve, 500));
    if (operation.cancelled) {
      if (!res.writableEnded) sendJson(res, 409, { error: "query operation cancelled" });
      return;
    }
    operation.completed = true;
    sendJson(res, 200, result);
  }

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
      sendJson(res, 200, databases);
      return;
    }

    if (req.method === "POST" && path === "/api/v1/admin/databases") {
      let body: { name?: unknown };
      try {
        body = JSON.parse((await readBody(req)) || "{}") as { name?: unknown };
      } catch {
        sendJson(res, 400, { error: "invalid create database request" });
        return;
      }
      const requestedName = typeof body.name === "string" ? body.name.trim() : "";
      if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(requestedName)) {
        sendJson(res, 400, {
          error: "database names must be unqualified SQL identifiers",
        });
        return;
      }
      const name = requestedName.toLowerCase();
      if (databases.some((database) => database.name.toLowerCase() === name)) {
        sendJson(res, 409, { error: `database '${name}' already exists` });
        return;
      }
      databases.push({ name, description: "Created in this mock session" });
      sendJson(res, 201, { name });
      return;
    }

    if (req.method === "GET" && path === "/api/v1/admin/catalog") {
      const database = new URL(req.url ?? "/", "http://cassie.mock").searchParams.get("database");
      if (!database) {
        sendJson(res, 400, { error: "database is required" });
      } else if (!databases.some((candidate) => candidate.name === database)) {
        sendJson(res, 404, { error: `database '${database}' does not exist` });
      } else {
        sendJson(res, 200, mockSchemaForDatabase(database));
      }
      return;
    }

    if (req.method === "DELETE" && path.startsWith("/api/v1/admin/query-operations/")) {
      const parts = path.split("/");
      const operationId = decodeURIComponent(parts[parts.length - 1] ?? "");
      const operation = operations.get(operationId);
      if (!operation) sendJson(res, 404, { error: "query operation not found" });
      else if (operation.completed)
        sendJson(res, 409, { error: "query operation already completed" });
      else {
        operation.cancelled = true;
        sendJson(res, 200, { cancelled: true });
      }
      return;
    }

    if (req.method === "POST" && path === "/api/v1/admin/query-executions") {
      const body = await queryBody(req, res);
      if (body) {
        const createdDatabase = /^CREATE\s+DATABASE\s+([A-Za-z_][A-Za-z0-9_]*)\s*;?$/i.exec(
          body.sql ?? "",
        )?.[1];
        if (createdDatabase && !databases.some((database) => database.name === createdDatabase)) {
          databases.push({ name: createdDatabase, description: "Created in this mock session" });
        }
        await completeOperation(
          body.operation_id!,
          res,
          mockExecuteResult(body.database!, body.sql ?? ""),
        );
      }
      return;
    }

    if (req.method === "POST" && path === "/api/v1/admin/query-validations") {
      const body = await queryBody(req, res);
      if (body)
        await completeOperation(body.operation_id!, res, {
          valid: true,
          command: body.sql?.trim().toUpperCase().startsWith("SELECT") ? "SELECT" : "SELECT",
          columns:
            body.database === "Database1"
              ? [column("order_id"), column("customer_id")]
              : [column("ticket_id"), column("account_id")],
        });
      return;
    }

    if (req.method === "POST" && path === "/api/v1/admin/query-explanations") {
      const body = await queryBody(req, res);
      if (body) await completeOperation(body.operation_id!, res, mockExplainResult(body.database!));
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
