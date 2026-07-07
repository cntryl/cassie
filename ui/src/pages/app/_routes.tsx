import { group, lazy, route } from "@askrjs/askr/router";

import { adminRoutes } from "@/shared/admin-routes";
import Layout from "./_layout";

const AdminPlaceholderPage = lazy(() => import("./placeholder"));
const QueryPage = lazy(() => import("./query"));

export function registerAppRoutes() {
  group({ layout: Layout }, () => {
    for (const adminRoute of adminRoutes) {
      route(adminRoute.path, adminRoute.path === "/admin/query" ? QueryPage : AdminPlaceholderPage);
    }
  });
}
