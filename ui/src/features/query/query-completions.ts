import type {
  MonacoCompletionContext,
  MonacoCompletionItem,
} from "@/components/query/monaco-sql-editor";
import type { QuerySchemaDatabase, QuerySchemaItem } from "./query-models";

const START_KEYWORDS = ["SELECT", "INSERT INTO", "UPDATE", "DELETE FROM", "CREATE", "CALL"];
const RELATION_KEYWORDS = ["WHERE", "JOIN", "ON", "GROUP BY", "ORDER BY", "LIMIT"];
const EXPRESSION_KEYWORDS = ["FROM", "WHERE", "GROUP BY", "ORDER BY", "LIMIT", "AS"];
const ALIAS_STOP_WORDS = new Set([
  "WHERE",
  "JOIN",
  "ON",
  "GROUP",
  "ORDER",
  "LIMIT",
  "SET",
  "VALUES",
  "RETURNING",
  "LEFT",
  "RIGHT",
  "INNER",
  "OUTER",
  "FULL",
  "CROSS",
]);

type CompletionMode = "start" | "relation" | "expression" | "procedure" | "index";
type RelationReference = { item: QuerySchemaItem; qualifier: string };

function quoteIdentifier(identifier: string) {
  return /^[a-z_][a-z0-9_$]*$/i.test(identifier)
    ? identifier
    : `"${identifier.replace(/"/g, '""')}"`;
}

function keywordItems(keywords: readonly string[]): MonacoCompletionItem[] {
  return keywords.map((keyword, index) => ({
    label: keyword,
    insertText: keyword,
    detail: "SQL keyword",
    kind: "keyword",
    sortText: `9-${String(index).padStart(2, "0")}-${keyword}`,
  }));
}

function currentDatabaseSchema(schema: QuerySchemaDatabase[], databaseName: string) {
  return schema.find(
    (database) => database.id.localeCompare(databaseName, undefined, { sensitivity: "base" }) === 0,
  );
}

function itemsForDatabase(schema: QuerySchemaDatabase[], databaseName: string) {
  const database = currentDatabaseSchema(schema, databaseName);
  if (!database) return [];

  return database.namespaces.flatMap((namespace) =>
    namespace.sections.flatMap((section) => section.items),
  );
}

function executableObjectName(item: QuerySchemaItem) {
  return `${quoteIdentifier(item.schema)}.${quoteIdentifier(item.name)}`;
}

function objectCompletion(item: QuerySchemaItem): MonacoCompletionItem {
  return {
    label: item.name,
    insertText: executableObjectName(item),
    filterText: `${item.name} ${item.schema}.${item.name} ${item.label}`,
    detail: `${item.kind} · ${item.database}.${item.schema}${item.metadata ? ` · ${item.metadata}` : ""}`,
    kind:
      item.kind === "udf"
        ? "function"
        : item.kind === "procedure"
          ? "method"
          : item.kind === "index"
            ? "reference"
            : "class",
    sortText: `1-${item.schema}-${item.name}`,
  };
}

function sqlBeforeCursor(context?: MonacoCompletionContext) {
  if (!context) return "";
  return context.sql.slice(0, context.offset);
}

function scrubSql(sql: string) {
  return sql
    .replace(/--[^\n]*/g, " ")
    .replace(/\/\*[\s\S]*?\*\//g, " ")
    .replace(/'(?:''|[^'])*'/g, "''");
}

function completionMode(sql: string): CompletionMode {
  const scrubbed = scrubSql(sql);
  const normalized = scrubbed.trimEnd();
  if (normalized.length === 0) return "start";
  if (!/\s$/.test(scrubbed) && /(?:^|;)\s*[a-z_]*$/i.test(normalized)) return "start";
  if (/\bCALL\s+(?:[\w".]*)$/i.test(normalized)) return "procedure";
  if (/\b(?:DROP|ALTER)\s+INDEX\s+(?:[\w".]*)$/i.test(normalized)) return "index";
  if (
    /\b(?:FROM|JOIN|UPDATE|INTO)\s+(?:[\w".]*)$/i.test(normalized) ||
    /\bDELETE\s+FROM\s+(?:[\w".]*)$/i.test(normalized)
  ) {
    return "relation";
  }
  return "expression";
}

function relationReferences(sql: string, relations: QuerySchemaItem[]): RelationReference[] {
  const byName = new Map(relations.map((item) => [item.name.toLowerCase(), item]));
  const references: RelationReference[] = [];
  const expression = /\b(?:FROM|JOIN|UPDATE|INTO)\s+([\w".]+)(?:\s+(?:AS\s+)?([a-z_][\w$]*))?/gi;

  for (const match of scrubSql(sql).matchAll(expression)) {
    const relationParts = match[1]?.replace(/"/g, "").split(".") ?? [];
    const relationName = relationParts[relationParts.length - 1]?.toLowerCase();
    if (!relationName) continue;
    const item = byName.get(relationName);
    if (!item) continue;
    const possibleAlias = match[2];
    const alias =
      possibleAlias && !ALIAS_STOP_WORDS.has(possibleAlias.toUpperCase())
        ? possibleAlias
        : item.name;
    references.push({ item, qualifier: quoteIdentifier(alias) });
  }

  return references;
}

function qualifierBeforeCursor(sql: string) {
  return sql.match(/([a-z_][\w$]*|"(?:""|[^"])+")\.\s*[\w$]*$/i)?.[1]?.replace(/"/g, "");
}

function columnItems(
  references: RelationReference[],
  requestedQualifier?: string,
): MonacoCompletionItem[] {
  const selected = requestedQualifier
    ? references.filter(
        (reference) =>
          reference.qualifier.replace(/"/g, "").toLowerCase() ===
            requestedQualifier.toLowerCase() ||
          reference.item.name.toLowerCase() === requestedQualifier.toLowerCase(),
      )
    : references;
  const unqualifiedCounts = new Map<string, number>();
  for (const { item } of selected) {
    for (const column of item.columns) {
      unqualifiedCounts.set(column.name, (unqualifiedCounts.get(column.name) ?? 0) + 1);
    }
  }

  return selected.flatMap(({ item, qualifier }) =>
    item.columns.map((column) => {
      const quotedColumn = quoteIdentifier(column.name);
      const needsQualifier =
        requestedQualifier === undefined && (unqualifiedCounts.get(column.name) ?? 0) > 1;
      const insertText = requestedQualifier
        ? quotedColumn
        : needsQualifier
          ? `${qualifier}.${quotedColumn}`
          : quotedColumn;
      return {
        label: requestedQualifier
          ? column.name
          : needsQualifier
            ? `${qualifier}.${column.name}`
            : column.name,
        insertText,
        filterText: `${column.name} ${qualifier}.${column.name}`,
        detail: `${column.dataType}${column.primaryKey ? " · primary key" : ""} · ${item.schema}.${item.name}`,
        kind: "field" as const,
        sortText: `${column.primaryKey ? "0" : "1"}-${column.name}`,
      };
    }),
  );
}

function deduplicate(items: MonacoCompletionItem[]) {
  const unique = new Map<string, MonacoCompletionItem>();
  for (const item of items) {
    const key = `${item.kind ?? ""}:${item.label}:${item.insertText}`;
    if (!unique.has(key)) unique.set(key, item);
  }
  return [...unique.values()];
}

export function buildSqlCompletionItems(
  schema: QuerySchemaDatabase[],
  databaseName: string,
  context?: MonacoCompletionContext,
): MonacoCompletionItem[] {
  const sql = sqlBeforeCursor(context);
  const documentSql = context?.sql ?? sql;
  const mode = completionMode(sql);
  const objects = itemsForDatabase(schema, databaseName);
  const relations = objects.filter((item) => item.kind === "table" || item.kind === "view");
  const routines = objects.filter((item) => item.kind === "udf");

  if (mode === "start") return keywordItems(START_KEYWORDS);
  if (mode === "procedure") {
    return objects.filter((item) => item.kind === "procedure").map(objectCompletion);
  }
  if (mode === "index") {
    return objects.filter((item) => item.kind === "index").map(objectCompletion);
  }
  if (mode === "relation") {
    return deduplicate([...relations.map(objectCompletion), ...keywordItems(RELATION_KEYWORDS)]);
  }

  const references = relationReferences(documentSql, relations);
  const qualifier = qualifierBeforeCursor(sql);
  if (qualifier) return deduplicate(columnItems(references, qualifier));

  return deduplicate([
    ...columnItems(references),
    ...routines.map(objectCompletion),
    ...keywordItems(EXPRESSION_KEYWORDS),
  ]);
}
