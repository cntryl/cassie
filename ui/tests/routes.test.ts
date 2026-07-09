import { describe, expect, it } from "vite-plus/test";
import { getRoutes } from "@askrjs/askr/router";

import "@/pages/_routes";

describe("admin route registration", () => {
  it("should_register_the_query_page_as_the_root_route", () => {
    const paths = getRoutes().map((route) => route.path);

    expect(paths.filter((path) => path === "/")).toHaveLength(1);
  });
});
