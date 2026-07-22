import type {
  QueryExplainResponse,
  QueryResult,
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

function mapSchemaItem(item: QuerySchemaResponse["sections"][number]["items"][number]) {
  return {
    id: item.id,
    kind: item.kind,
    label: item.label,
    database: item.database,
    schema: item.schema,
    name: item.name,
    metadata: item.metadata,
    columns: item.columns.map((column) => ({
      id: column.id,
      name: column.name,
      dataType: column.data_type,
      primaryKey: column.primary_key,
    })),
  };
}

// Items pass through unchanged (sortItems only reorders), so extra fields on a
// raw test fixture survive here even when the generated transport type does not
// declare them.
function normalizeSections(sections: QuerySchemaSection[]): QuerySchemaSection[] {
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
  defaultDatabase: string,
): QuerySchemaDatabase[] {
  const scopedBuckets = new Map<string, ScopedSectionBucket>();

  for (const section of sections) {
    const sectionId = toSchemaId(section.id);
    if (sectionId === null) {
      continue;
    }

    for (const item of section.items) {
      const bucket = getScopedBucket(scopedBuckets, sections, {
        database: item.database,
        namespace: item.schema,
      });
      const scopedSection =
        bucket.sections.get(sectionId) ??
        ({
          ...section,
          id: sectionId,
          items: [],
        } satisfies QuerySchemaSection);

      scopedSection.items.push(mapSchemaItem(item));
      bucket.sections.set(sectionId, scopedSection);
    }
  }

  if (scopedBuckets.size === 0) {
    getScopedBucket(scopedBuckets, sections, {
      database: defaultDatabase,
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

export function mapSchemaResponse(
  dto: QuerySchemaResponse,
  defaultDatabase = DEFAULT_DATABASE_ID,
): QuerySchema {
  const currentDatabase = defaultDatabase.trim() || DEFAULT_DATABASE_ID;
  return {
    databases: groupFlatSectionsByCatalogScope(dto.sections, currentDatabase),
  };
}

export function mapQueryResult(dto: QueryResult): QueryExecutionResult {
  return {
    command: dto.command,
    columns: dto.columns.map((column) => column.name),
    rows: dto.rows,
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
