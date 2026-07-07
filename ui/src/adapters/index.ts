import { client } from "./client";
import { createAdapter } from "./generated/api";

// Adapter boundary: keep transport concerns and generation output together.
export const api = createAdapter(client);

export { client, createAdapter };
export type * from "./generated/api";
