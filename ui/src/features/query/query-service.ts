import { apiv1 } from "@/adapters";
import { unwrapResponse, type ServiceRequestOptions } from "@/shared/errors/api";
import {
  mapQueryExplain,
  mapQueryResult,
  mapQueryValidation,
  mapSchemaResponse,
} from "./query-mappers";
import type { QueryExecutionResult, QuerySchema, QueryValidationResult } from "./query-models";
interface SchemaCacheEntry {
  generation: number;
  data?: QuerySchema;
  pending?: Promise<QuerySchema>;
}

const schemaCache = new Map<string, SchemaCacheEntry>();

function waitForConsumer<T>(request: Promise<T>, signal?: AbortSignal): Promise<T> {
  if (!signal) return request;
  if (signal.aborted) return Promise.reject(new DOMException("Aborted", "AbortError"));
  return new Promise((resolve, reject) => {
    const abort = () => reject(new DOMException("Aborted", "AbortError"));
    signal.addEventListener("abort", abort, { once: true });
    void request.then(resolve, reject).finally(() => signal.removeEventListener("abort", abort));
  });
}

async function getSchema(
  database: string,
  options: ServiceRequestOptions = {},
): Promise<QuerySchema> {
  const entry = schemaCache.get(database) ?? { generation: 0 };
  schemaCache.set(database, entry);
  if (entry.data) return entry.data;
  if (entry.pending) return waitForConsumer(entry.pending, options.signal);
  const requestGeneration = entry.generation;
  const request = (async () => {
    const response = await apiv1.listAdminCatalog({ query: { database } });
    const schema = mapSchemaResponse(
      unwrapResponse(response, "Unable to load query schema"),
      database,
    );
    const current = schemaCache.get(database);
    if (current?.generation === requestGeneration) current.data = schema;
    return schema;
  })();
  entry.pending = request;
  try {
    return await waitForConsumer(request, options.signal);
  } finally {
    const current = schemaCache.get(database);
    if (current?.pending === request) current.pending = undefined;
  }
}

async function validate(
  database: string,
  sql: string,
  operationId: string,
  options: ServiceRequestOptions = {},
): Promise<QueryValidationResult> {
  const response = await apiv1.createAdminQueryValidation({
    body: { database, sql, operation_id: operationId },
    ...options,
  });

  return mapQueryValidation(unwrapResponse(response, "Unable to validate SQL"));
}

async function execute(
  database: string,
  sql: string,
  operationId: string,
  options: ServiceRequestOptions = {},
): Promise<QueryExecutionResult> {
  const response = await apiv1.createAdminQueryExecution({
    body: { database, sql, operation_id: operationId },
    ...options,
  });

  return mapQueryResult(unwrapResponse(response, "Unable to execute SQL"));
}

async function explain(
  database: string,
  sql: string,
  operationId: string,
  options: ServiceRequestOptions = {},
): Promise<QueryExecutionResult> {
  const response = await apiv1.createAdminQueryExplanation({
    body: { database, sql, operation_id: operationId },
    ...options,
  });

  return mapQueryExplain(unwrapResponse(response, "Unable to explain SQL"));
}

async function cancel(operationId: string) {
  const response = await apiv1.cancelAdminQueryOperation({
    params: { operation_id: operationId },
  });
  return unwrapResponse(response, "Unable to stop query operation");
}

export const queryService = {
  invalidateSchema(database: string) {
    const entry = schemaCache.get(database) ?? { generation: 0 };
    entry.generation += 1;
    entry.data = undefined;
    entry.pending = undefined;
    schemaCache.set(database, entry);
  },
  getSchema,
  validate,
  execute,
  explain,
  cancel,
};
