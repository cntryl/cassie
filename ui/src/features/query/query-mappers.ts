import type {
  QueryExplainResponse,
  QueryResult,
  QueryResultValue,
  QuerySchemaResponse,
  QueryValidateResponse,
} from "@/adapters";
import type {
  QueryExecutionResult,
  QuerySchema,
  QuerySchemaDatabase,
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

// The catalog endpoint returns flat sections. It scopes current objects through
// canonical `database.schema.name` labels, so the UI derives tree grouping from
// those labels while preserving the wire item payloads.
const DEFAULT_DATABASE_ID = "postgres";
const DEFAULT_NAMESPACE_ID = "public";

type CatalogScope = { database: string; namespace: string };
type ScopedSectionBucket = {
  database: string;
  namespace: string;
  sections: Map<QuerySchemaSection["id"], QuerySchemaSection>;
};

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

function parseCanonicalScope(value: string): CatalogScope | null {
  const parts = value.split(".");
  if (parts.length !== 3 || parts.some((part) => part.trim().length === 0)) {
    return null;
  }

  return {
    database: parts[0],
    namespace: parts[1],
  };
}

function scopeForItem(item: QuerySchemaSection["items"][number]): CatalogScope {
  return (
    parseCanonicalScope(item.label) ??
    parseCanonicalScope(item.id.split(":").slice(1).join(":")) ?? {
      database: DEFAULT_DATABASE_ID,
      namespace: DEFAULT_NAMESPACE_ID,
    }
  );
}

// Items pass through unchanged (sortItems only reorders), so extra fields on a
// raw test fixture survive here even when the generated transport type does not
// declare them.
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

function sectionBucketsForScope(
  sections: QuerySchemaResponse["sections"],
): Map<QuerySchemaSection["id"], QuerySchemaSection> {
  const buckets = new Map<QuerySchemaSection["id"], QuerySchemaSection>();

  for (const section of sections) {
    const sectionId = toSchemaId(section.id);
    if (sectionId === null) {
      continue;
    }

    buckets.set(sectionId, {
      ...section,
      id: sectionId,
      items: [],
    });
  }

  return buckets;
}

function bucketKey(scope: CatalogScope) {
  return `${scope.database}::${scope.namespace}`;
}

function getScopedBucket(
  buckets: Map<string, ScopedSectionBucket>,
  sections: QuerySchemaResponse["sections"],
  scope: CatalogScope,
) {
  const key = bucketKey(scope);
  let bucket = buckets.get(key);

  if (!bucket) {
    bucket = {
      database: scope.database,
      namespace: scope.namespace,
      sections: sectionBucketsForScope(sections),
    };
    buckets.set(key, bucket);
  }

  return bucket;
}

function groupFlatSectionsByCatalogScope(
  sections: QuerySchemaResponse["sections"],
): QuerySchemaDatabase[] {
  const scopedBuckets = new Map<string, ScopedSectionBucket>();

  for (const section of sections) {
    const sectionId = toSchemaId(section.id);
    if (sectionId === null) {
      continue;
    }

    for (const item of section.items) {
      const bucket = getScopedBucket(scopedBuckets, sections, scopeForItem(item));
      const scopedSection =
        bucket.sections.get(sectionId) ??
        ({
          ...section,
          id: sectionId,
          items: [],
        } satisfies QuerySchemaSection);

      scopedSection.items.push(item);
      bucket.sections.set(sectionId, scopedSection);
    }
  }

  if (scopedBuckets.size === 0) {
    getScopedBucket(scopedBuckets, sections, {
      database: DEFAULT_DATABASE_ID,
      namespace: DEFAULT_NAMESPACE_ID,
    });
  }

  const databases = new Map<string, QuerySchemaDatabase>();
  for (const bucket of scopedBuckets.values()) {
    let database = databases.get(bucket.database);
    if (!database) {
      database = {
        id: bucket.database,
        label: bucket.database,
        namespaces: [],
      };
      databases.set(bucket.database, database);
    }

    database.namespaces.push({
      id: bucket.namespace,
      label: bucket.namespace,
      sections: normalizeSections([...bucket.sections.values()]),
    });
  }

  return [...databases.values()]
    .map((database) => ({
      ...database,
      namespaces: database.namespaces.sort((left, right) =>
        left.label.localeCompare(right.label, undefined, { sensitivity: "base" }),
      ),
    }))
    .sort((left, right) =>
      left.label.localeCompare(right.label, undefined, { sensitivity: "base" }),
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
  return {
    databases: groupFlatSectionsByCatalogScope(dto.sections),
  };
}

export function mapQueryResult(dto: QueryResult): QueryExecutionResult {
  return {
    command: dto.command,
    columns: dto.columns.map((column) => column.name),
    rows: dto.rows.map((row) => row.map((columnValue) => formatQueryValue(columnValue))),
  };
}

export function mapQueryExplain(dto: QueryExplainResponse): QueryExecutionResult {
  return {
    ...mapQueryResult(dto),
    plan: dto.plan,
  };
}

export function mapQueryValidation(dto: QueryValidateResponse): QueryValidationResult {
  return {
    command: dto.command,
    columns: dto.columns?.map((column) => column.name) ?? [],
    valid: dto.valid,
  };
}

export function flattenCompletionItems(schema: QuerySchemaDatabase[]) {
  return schema.flatMap((database) =>
    database.namespaces.flatMap((namespace) =>
      namespace.sections.flatMap((section) =>
        section.items.map((item) => ({
          label: item.label,
          insertText: item.label,
          detail: `${item.kind}${item.metadata ? ` · ${item.metadata}` : ""}`,
        })),
      ),
    ),
  );
}
