import { describe, expect, it } from "vite-plus/test";

import { buildSqlCompletionItems } from "@/features/query/query-completions";
import type { QuerySchemaDatabase } from "@/features/query/query-models";

const schema: QuerySchemaDatabase[] = [
  {
    id: "analytics",
    label: "analytics",
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
                id: "table:analytics.public.events",
                kind: "table",
                label: "analytics.public.events",
                database: "analytics",
                schema: "public",
                name: "events",
                metadata: "3 columns",
                columns: [
                  { id: "events:id", name: "id", dataType: "uuid", primaryKey: true },
                  { id: "events:name", name: "name", dataType: "text", primaryKey: false },
                  {
                    id: "events:occurred",
                    name: "occurred at",
                    dataType: "timestamp",
                    primaryKey: false,
                  },
                ],
              },
              {
                id: "table:analytics.reporting.accounts",
                kind: "table",
                label: "analytics.reporting.accounts",
                database: "analytics",
                schema: "reporting",
                name: "accounts",
                columns: [{ id: "accounts:id", name: "id", dataType: "uuid", primaryKey: true }],
              },
            ],
          },
          {
            id: "views",
            label: "Views",
            items: [
              {
                id: "view:analytics.public.recent_events",
                kind: "view",
                label: "analytics.public.recent_events",
                database: "analytics",
                schema: "public",
                name: "recent_events",
                columns: [{ id: "recent:id", name: "id", dataType: "uuid", primaryKey: false }],
              },
            ],
          },
          { id: "indexes", label: "Indexes", items: [] },
          {
            id: "udfs",
            label: "UDFs",
            items: [
              {
                id: "udf:analytics.public.event_count",
                kind: "udf",
                label: "analytics.public.event_count",
                database: "analytics",
                schema: "public",
                name: "event_count",
                metadata: "(since timestamp) -> bigint",
                columns: [],
              },
            ],
          },
          {
            id: "procedures",
            label: "Procedures",
            items: [
              {
                id: "procedure:analytics.public.archive_events",
                kind: "procedure",
                label: "analytics.public.archive_events",
                database: "analytics",
                schema: "public",
                name: "archive_events",
                columns: [],
              },
            ],
          },
        ],
      },
    ],
  },
  { id: "postgres", label: "postgres", namespaces: [] },
];

function complete(sql: string) {
  return buildSqlCompletionItems(schema, "analytics", {
    sql,
    offset: sql.length,
    word: sql.match(/[\w$]*$/)?.[0] ?? "",
  });
}

describe("SQL completions", () => {
  it("should_offer_executable_relations_after_from", () => {
    // Arrange / Act
    const items = complete("SELECT * FROM eve");

    // Assert
    expect(items.find((item) => item.label === "events")).toMatchObject({
      insertText: "public.events",
      kind: "class",
    });
    expect(items.some((item) => item.label === "event_count")).toBe(false);
    expect(items.some((item) => item.label.includes("analytics.public"))).toBe(false);
  });

  it("should_offer_columns_from_referenced_aliases", () => {
    // Arrange
    const sql = "SELECT e. FROM public.events AS e";

    // Act
    const items = buildSqlCompletionItems(schema, "analytics", {
      sql,
      offset: "SELECT e.".length,
      word: "",
    });

    // Assert
    expect(items.find((item) => item.label === "id")).toMatchObject({
      insertText: "id",
      detail: "uuid · primary key · public.events",
      kind: "field",
    });
    expect(items.find((item) => item.label === "occurred at")?.insertText).toBe('"occurred at"');
    expect(items.some((item) => item.label === "accounts")).toBe(false);
  });

  it("should_qualify_ambiguous_columns_in_joins", () => {
    // Arrange
    const sql = "SELECT  FROM public.events e JOIN reporting.accounts a ON e.id = a.id";

    // Act
    const items = buildSqlCompletionItems(schema, "analytics", {
      sql,
      offset: "SELECT ".length,
      word: "",
    });

    // Assert
    expect(
      items.filter((item) => item.label.endsWith(".id")).map((item) => item.insertText),
    ).toEqual(["e.id", "a.id"]);
  });

  it("should_offer_only_procedures_after_call", () => {
    // Arrange / Act
    const items = complete("CALL arch");

    // Assert
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({
      label: "archive_events",
      insertText: "public.archive_events",
      kind: "method",
    });
  });

  it("should_keep_other_database_objects_out_of_scope", () => {
    // Arrange / Act
    const items = buildSqlCompletionItems(schema, "postgres", {
      sql: "SELECT * FROM ",
      offset: 14,
      word: "",
    });

    // Assert
    expect(items.some((item) => item.kind === "class")).toBe(false);
  });
});
