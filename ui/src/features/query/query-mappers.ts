import type {
  QueryResult,
  QueryResultValue,
  QuerySchemaResponse,
  QueryValidateResponse,
} from "@/adapters";
import type {
  QueryExecutionResult,
  QuerySchema,
  QuerySchemaDatabase,
  QuerySchemaNamespace,
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

// The backend doesn't yet expose multiple databases/namespaces — the real
// `/api/v1/admin/query/schema` endpoint still returns a flat `{ sections }`
// shape (see src/rest/query.rs). These are placeholder labels (not a real
// database/namespace name — Cassie is not Postgres, it only speaks the pg
// wire protocol) so the tree has somewhere to nest today's data. Delete once
// the backend response actually carries real database/namespace names.
const DEFAULT_DATABASE_ID = "default";
const DEFAULT_DATABASE_LABEL = "default";
const DEFAULT_NAMESPACE_ID = "public";
const DEFAULT_NAMESPACE_LABEL = "public";

// Not yet part of the generated `QuerySchemaResponse` type — a forward-compat
// shim for when the backend ships real multi-database/namespace grouping.
interface WireSchemaNamespace {
  id: string;
  label: string;
  sections: QuerySchemaResponse["sections"];
}

interface WireSchemaDatabase {
  id: string;
  label: string;
  namespaces: WireSchemaNamespace[];
}

type WireSchemaResponse = QuerySchemaResponse & { databases?: WireSchemaDatabase[] };

function toSchemaId(value: string): QuerySchemaSection["id"] | null {
  if (
    value === "tables" ||
    value === "views" ||
    value === "indexes" ||
    value === "udfs" ||
    value === "procedures"
  ) {
    return value;
  }

  return null;
}

function sortItems(items: QuerySchemaSection["items"]) {
  return [...items].sort((left, right) =>
    left.label.localeCompare(right.label, undefined, { sensitivity: "base" }),
  );
}

// Items pass through unchanged (sortItems only reorders), so a forward-compat
// `columns` array on a wire item (not yet part of the generated type — see
// WireSchemaDatabase above) survives here and is visible to the tree/item
// components even though this function's declared item type doesn't list it.
function normalizeSections(sections: QuerySchemaResponse["sections"]): QuerySchemaSection[] {
  const sectionsById = new Map<string, QuerySchemaSection>(
    sections
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

  return QUERY_SCHEMA_SECTION_ORDER.map(
    (sectionId) =>
      sectionsById.get(sectionId) ?? {
        id: sectionId,
        items: [],
        label: sectionId,
      },
  );
}

function formatQueryValue(value: QueryResultValue): string | null {
  if (value === null) {
    return null;
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
  const wire = dto as WireSchemaResponse;

  if (wire.databases && wire.databases.length > 0) {
    const databases: QuerySchemaDatabase[] = wire.databases.map((database) => ({
      id: database.id,
      label: database.label,
      namespaces: database.namespaces.map(
        (namespace): QuerySchemaNamespace => ({
          id: namespace.id,
          label: namespace.label,
          sections: normalizeSections(namespace.sections),
        }),
      ),
    }));

    return { databases };
  }

  return {
    databases: [
      {
        id: DEFAULT_DATABASE_ID,
        label: DEFAULT_DATABASE_LABEL,
        namespaces: [
          {
            id: DEFAULT_NAMESPACE_ID,
            label: DEFAULT_NAMESPACE_LABEL,
            sections: normalizeSections(dto.sections),
          },
        ],
      },
    ],
  };
}

export function mapQueryResult(dto: QueryResult): QueryExecutionResult {
  return {
    command: dto.command,
    columns: dto.columns.map((column) => column.name),
    rows: dto.rows.map((row) => row.map((columnValue) => formatQueryValue(columnValue))),
  };
}

export function mapQueryValidation(dto: QueryValidateResponse): QueryValidationResult {
  return {
    command: dto.command,
    columns: dto.columns?.map((column) => column.name) ?? [],
    valid: dto.valid,
  };
}
