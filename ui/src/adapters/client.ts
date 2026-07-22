import type { Middleware } from "@askrjs/fetch";

import { createApiClient } from "./generated";

const configuredBaseUrl =
  (typeof import.meta !== "undefined" && import.meta.env?.VITE_CASSIE_API_BASE_URL) || "/";
const API_BASE_URL =
  typeof window === "undefined"
    ? configuredBaseUrl
    : new URL(configuredBaseUrl, window.location.origin).toString();

function createRequestId() {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return `cassie-ui-${crypto.randomUUID()}`;
  }

  return `cassie-ui-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function shouldRedirectToLogin(
  status: number,
  requestPath: string,
  currentPath: string,
): boolean {
  return status === 401 && !requestPath.startsWith("/api/v1/auth/") && currentPath !== "/login";
}

const cassieMiddleware: Middleware = async (context, next) => {
  const headers = new Headers(context.request.headers);

  if (!headers.has("Accept")) {
    headers.set("Accept", "application/json");
  }

  if (!headers.has("x-request-id")) {
    headers.set("x-request-id", createRequestId());
  }

  const result = await next({
    ...context,
    request: new Request(context.request, { headers }),
  });

  const path = new URL(context.request.url).pathname;
  if (!result.ok && shouldRedirectToLogin(result.status, path, window.location.pathname)) {
    window.location.assign("/login");
  }

  return result;
};

export const client = createApiClient({
  baseUrl: API_BASE_URL,
  credentials: "same-origin",
  timeout: 30_000,
  middleware: [cassieMiddleware],
});
