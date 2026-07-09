import { group, lazy, route } from "@askrjs/askr/router";

import Layout from "./_layout";

const QueryPage = lazy(() => import("./query"));

export function registerAppRoutes() {
  group({ layout: Layout }, () => {
    route("/", QueryPage);
  });
}
