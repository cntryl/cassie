import type { QuerySchemaResponse } from "@/adapters";

export const querySchemaResponse: QuerySchemaResponse = {
  sections: [
    {
      id: "tables",
      label: "Tables",
      items: [
        {
          id: "table:postgres.public.documents",
          kind: "table",
          label: "postgres.public.documents",
          database: "postgres",
          schema: "public",
          name: "documents",
          columns: [],
          metadata: "2 columns",
        },
        {
          id: "table:postgres.public.accounts",
          kind: "table",
          label: "postgres.public.accounts",
          database: "postgres",
          schema: "public",
          name: "accounts",
          columns: [],
          metadata: "6 columns",
        },
      ],
    },
    { id: "views", label: "Views", items: [] },
    { id: "indexes", label: "Indexes", items: [] },
    { id: "udfs", label: "UDFs", items: [] },
    { id: "procedures", label: "Procedures", items: [] },
  ],
};
