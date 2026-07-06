import { describe, expect, it } from "vite-plus/test";
import { getRoutes } from "@askrjs/askr/router";

import { adminRoutes } from "@/shared/admin-routes";
import "@/pages/_routes";

describe("admin route registration", () => {
  it("should_register_the_scaffold_admin_routes", () => {
    const paths = getRoutes().map((route) => route.path);

    expect(paths).toEqual(expect.arrayContaining(adminRoutes.map((route) => route.path)));
    expect(paths.filter((path) => adminRoutes.some((route) => route.path === path))).toHaveLength(
      adminRoutes.length,
    );
  });
});
