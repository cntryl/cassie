import { createMutation } from "@askrjs/askr/data";
import { queryService } from "./query-service";
import type { QueryExecutionResult, QueryValidationResult } from "./query-models";

interface QueryPayload {
  sql: string;
}

export function createExecuteQueryMutation() {
  return createMutation<QueryPayload, QueryExecutionResult>({
    action: ({ sql }, { signal }) => queryService.execute(sql, { signal }),
  });
}

export function createValidateQueryMutation() {
  return createMutation<QueryPayload, QueryValidationResult>({
    action: ({ sql }, { signal }) => queryService.validate(sql, { signal }),
  });
}

export function createExplainQueryMutation() {
  return createMutation<QueryPayload, QueryExecutionResult>({
    action: ({ sql }, { signal }) => queryService.explain(sql, { signal }),
  });
}
