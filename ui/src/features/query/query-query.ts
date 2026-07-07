import { createQuery, queryScope } from "@askrjs/askr/data";
import type { QuerySchema } from "./query-models";
import { queryService } from "./query-service";

const queryQueries = queryScope("query");

export const QUERY_SCHEMA_KEY = queryQueries.key("schema");

function fetchAdminQuerySchema({ signal }: { signal?: AbortSignal }) {
  return queryService.getSchema({ signal });
}

export function createAdminQuerySchemaQuery() {
  return createQuery<QuerySchema>({
    key: QUERY_SCHEMA_KEY,
    fetch: fetchAdminQuerySchema,
  });
}
