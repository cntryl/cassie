export type QuerySchemaSectionId = "tables" | "views" | "indexes" | "udfs" | "procedures";

export type QuerySchemaItemKind = "table" | "view" | "index" | "udf" | "procedure";

export interface QuerySchemaColumn {
  id: string;
  name: string;
  dataType?: string;
  primaryKey?: boolean;
}

export interface QuerySchemaItem {
  id: string;
  kind: QuerySchemaItemKind;
  label: string;
  metadata?: string;
  columns?: QuerySchemaColumn[];
}

export interface QuerySchemaSection {
  id: QuerySchemaSectionId;
  label: string;
  items: QuerySchemaItem[];
}

export interface QuerySchemaNamespace {
  id: string;
  label: string;
  sections: QuerySchemaSection[];
}

export interface QuerySchemaDatabase {
  id: string;
  label: string;
  namespaces: QuerySchemaNamespace[];
}

export interface QuerySchema {
  databases: QuerySchemaDatabase[];
}

export interface QueryExecutionResult {
  command: string;
  columns: string[];
  rows: Array<Array<string | null>>;
}

export interface QueryValidationResult {
  command: string;
  columns: string[];
  valid: boolean;
}
