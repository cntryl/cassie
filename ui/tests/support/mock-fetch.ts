import { vi } from "vite-plus/test";

interface MockJsonResponse {
  readonly body: unknown;
  readonly status: number;
}

const responses = new Map<string, MockJsonResponse>();

function responseKey(method: string, url: string) {
  return `${method.toUpperCase()} ${new URL(url, window.location.origin).pathname}`;
}

export const fetchMock = vi.fn(async (request: Request) => {
  const key = responseKey(request.method, request.url);
  const response = responses.get(key);

  if (!response) {
    throw new Error(`Missing fetch mock for ${key}`);
  }

  return new Response(JSON.stringify(response.body), {
    status: response.status,
    headers: { "content-type": "application/json" },
  });
});

export function mockJsonResponse(
  path: string,
  body: unknown,
  { method = "GET", status = 200 }: { method?: string; status?: number } = {},
) {
  responses.set(responseKey(method, path), { body, status });
  vi.stubGlobal("fetch", fetchMock);
}

export function resetFetchMock() {
  responses.clear();
  fetchMock.mockClear();
  vi.unstubAllGlobals();
}
