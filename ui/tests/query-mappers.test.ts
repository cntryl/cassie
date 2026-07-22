import { describe, expect, it } from "vite-plus/test";

import { mapSchemaResponse } from "@/features/query/query-mappers";

describe("query DTO mapping", () => {
  it("should_anchor_an_empty_catalog_to_the_authenticated_database", () => {
    // Arrange
    const response = { sections: [] };

    // Act
    const schema = mapSchemaResponse(response, "analytics");

    // Assert
    expect(schema.databases).toHaveLength(1);
    expect(schema.databases[0]?.id).toBe("analytics");
    expect(schema.databases[0]?.namespaces[0]?.id).toBe("public");
  });
});
