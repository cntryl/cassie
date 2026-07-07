import { apiv1 } from "@/adapters";
import { unwrapResponse, type ServiceRequestOptions } from "@/shared/errors/api";
import { mapQueryResult, mapQueryValidation, mapSchemaResponse } from "./query-mappers";
import type { QueryExecutionResult, QuerySchema, QueryValidationResult } from "./query-models";

async function getSchema(options: ServiceRequestOptions = {}): Promise<QuerySchema> {
  const response = await apiv1.getAdminQuerySchema(options);

  return mapSchemaResponse(unwrapResponse(response, "Unable to load query schema"));
}

async function validate(
  sql: string,
  options: ServiceRequestOptions = {},
): Promise<QueryValidationResult> {
  const response = await apiv1.validateAdminQuery({ sql }, options);

  return mapQueryValidation(unwrapResponse(response, "Unable to validate SQL"));
}

async function execute(
  sql: string,
  options: ServiceRequestOptions = {},
): Promise<QueryExecutionResult> {
  const response = await apiv1.executeAdminQuery({ sql }, options);

  return mapQueryResult(unwrapResponse(response, "Unable to execute SQL"));
}

async function explain(
  sql: string,
  options: ServiceRequestOptions = {},
): Promise<QueryExecutionResult> {
  const response = await apiv1.explainAdminQuery({ sql }, options);

  return mapQueryResult(unwrapResponse(response, "Unable to explain SQL"));
}

export const queryService = {
  getSchema,
  validate,
  execute,
  explain,
};
