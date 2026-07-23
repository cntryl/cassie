import type { MonacoCompletionItem } from "@/components/query/monaco-sql-editor";
import type { QuerySchemaDatabase } from "./query-models";
import { flattenCompletionItems } from "./query-mappers";

const SQL_KEYWORDS = [
  "SELECT",
  "FROM",
  "WHERE",
  "JOIN",
  "INSERT",
  "UPDATE",
  "DELETE",
  "CREATE",
  "ALTER",
  "DROP",
  "GROUP BY",
  "ORDER BY",
  "LIMIT",
] as const;

export function buildSqlCompletionItems(schema: QuerySchemaDatabase[]): MonacoCompletionItem[] {
  return [
    ...SQL_KEYWORDS.map((keyword) => ({
      label: keyword,
      insertText: keyword,
      detail: "SQL keyword",
    })),
    ...flattenCompletionItems(schema),
  ];
}
