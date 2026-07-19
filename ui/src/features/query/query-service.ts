import { apiv1 } from "@/adapters";
import { unwrapResponse, type ServiceRequestOptions } from "@/shared/errors/api";
import {
  mapQueryExplain,
  mapQueryResult,
  mapQueryValidation,
  mapSchemaResponse,
} from "./query-mappers";
import type { QueryExecutionResult, QuerySchema, QueryValidationResult } from "./query-models";

async function getSchema(options: ServiceRequestOptions = {}): Promise<QuerySchema> {
  const response = await apiv1.listAdminCatalog(options);

  return mapSchemaResponse(unwrapResponse(response, "Unable to load query schema"));
}

async function validate(
  sql: string,
  options: ServiceRequestOptions = {},
): Promise<QueryValidationResult> {
  const response = await apiv1.createAdminQueryValidation({ body: { sql }, ...options });

  return mapQueryValidation(unwrapResponse(response, "Unable to validate SQL"));
}

async function execute(
  sql: string,
  options: ServiceRequestOptions = {},
): Promise<QueryExecutionResult> {
  const response = await apiv1.createAdminQueryExecution({ body: { sql }, ...options });

  return mapQueryResult(unwrapResponse(response, "Unable to execute SQL"));
}

async function explain(
  sql: string,
  options: ServiceRequestOptions = {},
): Promise<QueryExecutionResult> {
  const response = await apiv1.createAdminQueryExplanation({ body: { sql }, ...options });

  return mapQueryExplain(unwrapResponse(response, "Unable to explain SQL"));
}

export const queryService = {
  getSchema,
  validate,
  execute,
  explain,
};
