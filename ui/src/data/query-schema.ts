export type QuerySchemaObjectKind = "table" | "view" | "index" | "udf" | "procedure";

export interface QuerySchemaObject {
  id: string;
  kind: QuerySchemaObjectKind;
  label: string;
  metadata?: string;
}

export interface QuerySchemaSection {
  id: string;
  label: string;
  items: QuerySchemaObject[];
}

export const querySchema: QuerySchemaSection[] = [
  {
    id: "tables",
    label: "Tables",
    items: [
      { id: "users", kind: "table", label: "users", metadata: "5 columns" },
      { id: "documents", kind: "table", label: "documents", metadata: "14 columns" },
      { id: "sessions", kind: "table", label: "sessions", metadata: "4 columns" },
    ],
  },
  {
    id: "views",
    label: "Views",
    items: [{ id: "recent_documents", kind: "view", label: "recent_documents", metadata: "Materialized: no" }],
  },
  {
    id: "indexes",
    label: "Indexes",
    items: [
      { id: "idx_users_email", kind: "index", label: "idx_users_email", metadata: "ON users(email)" },
      { id: "idx_documents_type", kind: "index", label: "idx_documents_type", metadata: "ON documents(type)" },
    ],
  },
  {
    id: "udfs",
    label: "UDFs",
    items: [
      { id: "json_extract", kind: "udf", label: "json_extract", metadata: "language: SQL" },
      { id: "embedding_distance", kind: "udf", label: "embedding_distance", metadata: "language: Rust" },
    ],
  },
  {
    id: "procedures",
    label: "Procedures",
    items: [{ id: "refresh_analytics", kind: "procedure", label: "refresh_analytics" }],
  },
];
