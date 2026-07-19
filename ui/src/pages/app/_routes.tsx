import { group, lazy, route } from "@askrjs/askr/router";
import { requireUser } from "@askrjs/auth";

import Layout from "./_layout";

const QueryPage = lazy(() => import("./query"));

export function registerAppRoutes() {
  group({ layout: Layout, auth: requireUser() }, () => {
    route("/", QueryPage);
  });
}
