import { apiv1 } from "@/adapters";
import { unwrapResponse, type ServiceRequestOptions } from "@/shared/errors/api";
import {
  mapQueryExplain,
  mapQueryResult,
  mapQueryValidation,
  mapSchemaResponse,
} from "./query-mappers";
import type { QueryExecutionResult, QuerySchema, QueryValidationResult } from "./query-models";
const schemaRequests = new Map<string, Promise<QuerySchema>>();
const schemaCache = new Map<string, QuerySchema>();

async function getSchema(
  database: string,
  options: ServiceRequestOptions = {},
): Promise<QuerySchema> {
  const cached = schemaCache.get(database);
  if (cached) return cached;
  const pending = schemaRequests.get(database);
  if (pending) return pending;
  const request = (async () => {
    const response = await apiv1.listAdminCatalog({ query: { database }, ...options });
    const schema = mapSchemaResponse(
      unwrapResponse(response, "Unable to load query schema"),
      database,
    );
    schemaCache.set(database, schema);
    return schema;
  })();
  schemaRequests.set(database, request);
  try {
    return await request;
  } finally {
    if (schemaRequests.get(database) === request) schemaRequests.delete(database);
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
    schemaCache.delete(database);
  },
  getSchema,
  validate,
  execute,
  explain,
  cancel,
};
