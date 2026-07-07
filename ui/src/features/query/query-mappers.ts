import type {
  QueryResult,
  QueryResultValue,
  QuerySchemaResponse,
  QueryValidateResponse,
} from "@/adapters";
import type {
  QueryExecutionResult,
  QuerySchema,
  QuerySchemaSection,
  QueryValidationResult,
} from "./query-models";

const QUERY_SCHEMA_SECTION_ORDER: QuerySchemaSection["id"][] = [
  "tables",
  "views",
  "indexes",
  "udfs",
  "procedures",
];

function toSchemaId(value: string): QuerySchemaSection["id"] | null {
  if (value === "tables" || value === "views" || value === "indexes" || value === "udfs" || value === "procedures") {
    return value;
  }

  return null;
}

function sortItems(items: QuerySchemaSection["items"]) {
  return [...items].sort((left, right) => left.label.localeCompare(right.label, undefined, { sensitivity: "base" }));
}

function stringForQueryValue(value: QueryResultValue) {
  if (value === null) {
    return "NULL";
  }

  if (typeof value === "string") {
    return value;
  }

  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }

  return JSON.stringify(value);
}

export function mapSchemaResponse(dto: QuerySchemaResponse): QuerySchema {
  const sectionsById = new Map<string, QuerySchemaSection>(
    dto.sections
      .map((section) => {
        const sectionId = toSchemaId(section.id);
        if (sectionId === null) {
          return null;
        }

        return [
          sectionId,
          {
            ...section,
            id: sectionId,
            items: sortItems(section.items),
          },
        ] as [string, QuerySchemaSection];
      })
      .filter((entry): entry is [string, QuerySchemaSection] => entry !== null),
  );

  const sections = QUERY_SCHEMA_SECTION_ORDER.map((sectionId) =>
    sectionsById.get(sectionId) ?? {
      id: sectionId,
      items: [],
      label: sectionId,
    },
  );

  return { sections };
}

export function mapQueryResult(dto: QueryResult): QueryExecutionResult {
  return {
    command: dto.command,
    columns: dto.columns.map((column) => column.name),
    rows: dto.rows.map((row) => row.map((columnValue) => stringForQueryValue(columnValue))),
  };
}

export function mapQueryValidation(dto: QueryValidateResponse): QueryValidationResult {
  return {
    command: dto.command,
    columns: dto.columns?.map((column) => column.name) ?? [],
    valid: dto.valid,
  };
}
