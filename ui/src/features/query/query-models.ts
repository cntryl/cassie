export type QuerySchemaSectionId = "tables" | "views" | "indexes" | "udfs" | "procedures";

export type QuerySchemaItemKind = "table" | "view" | "index" | "udf" | "procedure";

export interface QuerySchemaItem {
  id: string;
  kind: QuerySchemaItemKind;
  label: string;
  metadata?: string;
}

export interface QuerySchemaSection {
  id: QuerySchemaSectionId;
  label: string;
  items: QuerySchemaItem[];
}

export interface QuerySchema {
  sections: QuerySchemaSection[];
}

export interface QueryExecutionResult {
  command: string;
  columns: string[];
  rows: string[][];
}

export interface QueryValidationResult {
  command: string;
  columns: string[];
  valid: boolean;
}
