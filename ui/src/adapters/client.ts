import { FetchClient } from "@fgrzl/fetch";

const API_BASE_URL =
  (typeof import.meta !== "undefined" && import.meta.env?.VITE_CASSIE_API_BASE_URL) || "/";

function createRequestId() {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return `cassie-ui-${crypto.randomUUID()}`;
  }

  return `cassie-ui-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export const client = new FetchClient({
  baseUrl: API_BASE_URL,
  credentials: "same-origin",
  timeout: 30_000,
});

client.use((request, next) => {
  const headers = new Headers(request.headers);

  if (!headers.has("Accept")) {
    headers.set("Accept", "application/json");
  }

  if (!headers.has("x-request-id")) {
    headers.set("x-request-id", createRequestId());
  }

  return next({
    ...request,
    headers,
  });
});
