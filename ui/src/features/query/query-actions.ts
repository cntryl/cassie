import { createMutation } from "@askrjs/askr/data";
import { queryService } from "./query-service";
import type { QueryExecutionResult, QueryValidationResult } from "./query-models";

interface QueryPayload {
  database: string;
  sql: string;
  operationId: string;
}

export function createExecuteQueryMutation() {
  return createMutation<QueryPayload, QueryExecutionResult>({
    action: ({ database, sql, operationId }, { signal }) =>
      queryService.execute(database, sql, operationId, { signal }),
  });
}

export function createValidateQueryMutation() {
  return createMutation<QueryPayload, QueryValidationResult>({
    action: ({ database, sql, operationId }, { signal }) =>
      queryService.validate(database, sql, operationId, { signal }),
  });
}

export function createExplainQueryMutation() {
  return createMutation<QueryPayload, QueryExecutionResult>({
    action: ({ database, sql, operationId }, { signal }) =>
      queryService.explain(database, sql, operationId, { signal }),
  });
}
