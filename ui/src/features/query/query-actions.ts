import { createMutation } from "@askrjs/askr/data";
import { apiv1, type DatabaseSummary, type QueryOperationCancellation } from "@/adapters";
import { unwrapResponse } from "@/shared/errors/api";
import { adminQueries } from "./admin-query";
import { queryService } from "./query-service";
import type { QueryExecutionResult, QueryValidationResult } from "./query-models";

interface QueryPayload {
  database: string;
  sql: string;
  operationId: string;
}

export function createExecuteQueryMutation() {
  return createMutation<QueryPayload, QueryExecutionResult>({
    key: adminQueries.key("mutations", "execute"),
    action: ({ database, sql, operationId }, { signal }) =>
      queryService.execute(database, sql, operationId, { signal }),
  });
}

export function createValidateQueryMutation() {
  return createMutation<QueryPayload, QueryValidationResult>({
    key: adminQueries.key("mutations", "validate"),
    action: ({ database, sql, operationId }, { signal }) =>
      queryService.validate(database, sql, operationId, { signal }),
  });
}

export function createExplainQueryMutation() {
  return createMutation<QueryPayload, QueryExecutionResult>({
    key: adminQueries.key("mutations", "explain"),
    action: ({ database, sql, operationId }, { signal }) =>
      queryService.explain(database, sql, operationId, { signal }),
  });
}

export function createCancelQueryMutation() {
  return createMutation<string, QueryOperationCancellation>({
    key: adminQueries.key("mutations", "cancel"),
    action: async (operationId, { signal }) =>
      unwrapResponse(
        await apiv1.cancelAdminQueryOperation({
          params: { operation_id: operationId },
          signal,
        }),
        "Unable to stop query operation",
      ),
  });
}

export function createDatabaseMutation() {
  return createMutation<string, DatabaseSummary>({
    key: adminQueries.key("mutations", "create-database"),
    action: async (name, { signal }) =>
      unwrapResponse(
        await apiv1.createAdminDatabase({ body: { name }, signal }),
        "Unable to create database",
      ),
    affects: () => [adminQueries.prefix("databases")],
    afterSuccess: "invalidate",
  });
}
