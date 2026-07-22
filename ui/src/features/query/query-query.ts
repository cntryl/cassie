import { createQuery, queryScope } from "@askrjs/askr/data";
import type { QuerySchema } from "./query-models";
import { queryService } from "./query-service";

const queryQueries = queryScope("query");
const schemaFetchers = new Map<
  string,
  ({ signal }: { signal?: AbortSignal }) => Promise<QuerySchema>
>();

function schemaFetcher(database: string) {
  let fetcher = schemaFetchers.get(database);
  if (!fetcher) {
    fetcher = ({ signal }) => queryService.getSchema(database, { signal });
    schemaFetchers.set(database, fetcher);
  }
  return fetcher;
}

export function createAdminQuerySchemaQuery(database: string) {
  return createQuery<QuerySchema>({
    key: queryQueries.key(`schema:${database}`),
    fetch: schemaFetcher(database),
  });
}
