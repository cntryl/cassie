import { defineQuery, queryScope } from "@askrjs/askr/data";

import { apiv1, type DatabaseSummary } from "@/adapters";
import { unwrapResponse } from "@/shared/errors/api";

export const adminQueries = queryScope("admin");

export const databaseListQuery = defineQuery<Record<string, never>, DatabaseSummary[]>({
  key: () => adminQueries.key("databases"),
  fetch: async ({ signal }) =>
    unwrapResponse(await apiv1.listAdminDatabases({ signal }), "Unable to load databases"),
});
