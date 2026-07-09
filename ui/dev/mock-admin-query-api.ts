import type { IncomingMessage, ServerResponse } from "node:http";
import type { ViteDevServer } from "@voidzero-dev/vite-plus-core";

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

function schemaColumn(
  table: string,
  name: string,
  options: { dataType?: string; primaryKey?: boolean } = {},
) {
  return {
    id: `${table}:${name}`,
    name,
    dataType: options.dataType,
    primaryKey: options.primaryKey,
  };
}

const mockSchema = {
  databases: [
    {
      id: "db1",
      label: "Database 1",
      namespaces: [
        {
          id: "public",
          label: "public",
          sections: [
            {
              id: "tables",
              label: "Tables",
              items: [
                {
                  id: "table:documents",
                  kind: "table",
                  label: "documents",
                  metadata: "4 columns",
                  columns: [
                    schemaColumn("documents", "id", { dataType: "uuid", primaryKey: true }),
                    schemaColumn("documents", "title", { dataType: "text" }),
                    schemaColumn("documents", "owner_id", { dataType: "uuid" }),
                    schemaColumn("documents", "created_at", { dataType: "timestamptz" }),
                  ],
                },
                {
                  id: "table:accounts",
                  kind: "table",
                  label: "accounts",
                  metadata: "6 columns",
                  columns: [
                    schemaColumn("accounts", "id", { dataType: "uuid", primaryKey: true }),
                    schemaColumn("accounts", "name", { dataType: "text" }),
                    schemaColumn("accounts", "email", { dataType: "text" }),
                    schemaColumn("accounts", "plan", { dataType: "text" }),
                    schemaColumn("accounts", "created_at", { dataType: "timestamptz" }),
                    schemaColumn("accounts", "updated_at", { dataType: "timestamptz" }),
                  ],
                },
              ],
            },
            {
              id: "views",
              label: "Views",
              items: [
                {
                  id: "view:active_docs",
                  kind: "view",
                  label: "active_docs",
                  metadata: "3 columns",
                  columns: [
                    schemaColumn("active_docs", "id", { dataType: "uuid", primaryKey: true }),
                    schemaColumn("active_docs", "title", { dataType: "text" }),
                    schemaColumn("active_docs", "owner_id", { dataType: "uuid" }),
                  ],
                },
              ],
            },
            {
              id: "indexes",
              label: "Indexes",
              items: [
                {
                  id: "index:documents:idx_id",
                  kind: "index",
                  label: "idx_id",
                  metadata: "scalar on documents(id)",
                },
              ],
            },
            { id: "udfs", label: "UDFs", items: [] },
            { id: "procedures", label: "Procedures", items: [] },
          ],
        },
        {
          id: "reporting",
          label: "reporting",
          sections: [
            {
              id: "tables",
              label: "Tables",
              items: [
                {
                  id: "table:monthly_totals",
                  kind: "table",
                  label: "monthly_totals",
                  metadata: "5 columns",
                },
              ],
            },
            { id: "views", label: "Views", items: [] },
            { id: "indexes", label: "Indexes", items: [] },
            { id: "udfs", label: "UDFs", items: [] },
            { id: "procedures", label: "Procedures", items: [] },
          ],
        },
      ],
    },
    {
      id: "db2",
      label: "Database 2",
      namespaces: [
        {
          id: "public",
          label: "public",
          sections: [
            {
              id: "tables",
              label: "Tables",
              items: [
                {
                  id: "table:events",
                  kind: "table",
                  label: "events",
                  metadata: "8 columns",
                },
              ],
            },
            { id: "views", label: "Views", items: [] },
            { id: "indexes", label: "Indexes", items: [] },
            { id: "udfs", label: "UDFs", items: [] },
            { id: "procedures", label: "Procedures", items: [] },
          ],
        },
      ],
    },
  ],
};

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

function mockExplainResult() {
  return {
    columns: [column("QUERY PLAN")],
    command: "EXPLAIN",
    rows: [
      [
        "Seq Scan on documents  (cost=0.00..12.30 rows=230 width=64)\n" +
          "  Filter: (id IS NOT NULL)\n" +
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

        if (req.method === "GET" && url.startsWith("/api/v1/admin/query/schema")) {
          sendJson(res, 200, mockSchema);
          return;
        }

        if (req.method === "POST" && url.startsWith("/api/v1/admin/query/execute")) {
          const body = await readBody(req);
          const sql = JSON.parse(body || "{}").sql ?? "";
          sendJson(res, 200, mockExecuteResult(sql));
          return;
        }

        if (req.method === "POST" && url.startsWith("/api/v1/admin/query/validate")) {
          const body = await readBody(req);
          const sql = JSON.parse(body || "{}").sql ?? "";
          sendJson(res, 200, {
            valid: true,
            command: sql.trim().toUpperCase().startsWith("SELECT") ? "SELECT" : "SELECT",
            columns: [column("id"), column("name"), column("owner"), column("notes")],
          });
          return;
        }

        if (req.method === "POST" && url.startsWith("/api/v1/admin/query/explain")) {
          sendJson(res, 200, mockExplainResult());
          return;
        }

        next();
      });
    },
  };
}
