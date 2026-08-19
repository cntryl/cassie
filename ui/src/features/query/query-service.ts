import { apiv1 } from "@/adapters";
import { unwrapResponse, type ServiceRequestOptions } from "@/shared/errors/api";
import {
  mapQueryExplain,
  mapQueryResult,
  mapQueryValidation,
  mapSchemaResponse,
} from "./query-mappers";
import type { QueryExecutionResult, QuerySchema, QueryValidationResult } from "./query-models";

async function getSchema(
  database: string,
  options: ServiceRequestOptions = {},
): Promise<QuerySchema> {
  const response = await apiv1.listAdminCatalog({ query: { database }, ...options });
  return mapSchemaResponse(unwrapResponse(response, "Unable to load query schema"), database);
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

export const queryService = {
  getSchema,
  validate,
  execute,
  explain,
};
