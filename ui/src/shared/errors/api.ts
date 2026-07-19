import type { FailureResult, HttpResult, SuccessResult } from "@askrjs/fetch";

export type AppApiErrorCode =
  | "badRequest"
  | "unauthenticated"
  | "forbidden"
  | "notFound"
  | "conflict"
  | "serviceUnavailable"
  | "emptyResponse"
  | "unknown";

export interface ServiceRequestOptions {
  signal?: AbortSignal;
  timeout?: number;
}

export class AppApiError extends Error {
  readonly code: AppApiErrorCode;
  readonly status: number;

  constructor(message: string, status: number, code: AppApiErrorCode) {
    super(message);
    this.name = "AppApiError";
    this.status = status;
    this.code = code;
  }
}

function errorCodeForStatus(status: number): AppApiErrorCode {
  if (status === 400) return "badRequest";
  if (status === 401) return "unauthenticated";
  if (status === 403) return "forbidden";
  if (status === 404) return "notFound";
  if (status === 409) return "conflict";
  if (status === 503) return "serviceUnavailable";
  return "unknown";
}

type ApiResult<T> = SuccessResult<T> | HttpResult<unknown> | FailureResult;

function errorMessage(error: unknown, fallbackMessage: string) {
  if (error instanceof Error) {
    return error.message;
  }

  if (typeof error === "string") {
    return error;
  }

  if (error !== null && typeof error === "object") {
    const payload = error as Record<string, unknown>;
    if (typeof payload.message === "string") {
      return payload.message;
    }
    if (typeof payload.error === "string") {
      return payload.error;
    }
  }

  return fallbackMessage;
}

// Service boundary helper: unwrap Askr fetch results before data leaves services.
export function unwrapResponse<T>(response: ApiResult<T>, fallbackMessage: string): T {
  if (response.ok && response.data !== null && response.data !== undefined) {
    return response.data;
  }

  if (response.ok) {
    throw new AppApiError(fallbackMessage, response.status, "emptyResponse");
  }

  throw new AppApiError(
    errorMessage(response.error, fallbackMessage),
    response.status,
    errorCodeForStatus(response.status),
  );
}

export function ensureResponseOk<T>(response: ApiResult<T>, fallbackMessage: string) {
  if (response.ok) {
    return;
  }

  throw new AppApiError(
    errorMessage(response.error, fallbackMessage),
    response.status,
    errorCodeForStatus(response.status),
  );
}

export function apiErrorMessage(error: unknown): string {
  if (error instanceof AppApiError) {
    return error.message;
  }

  if (error instanceof Error) {
    return error.message;
  }

  if (typeof error === "string") {
    return error;
  }

  return "Request failed";
}
