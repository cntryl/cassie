import {
  createRouteRegistry,
  route,
  type RouteComponent,
  type RouteRegistry,
} from "@askrjs/askr/router";

interface TestRoute {
  path: string;
  handler: RouteComponent;
}

export function createTestRouteRegistry(routes: readonly TestRoute[]): RouteRegistry {
  return createRouteRegistry(() => {
    for (const testRoute of routes) {
      route(testRoute.path, testRoute.handler);
    }
  });
}
