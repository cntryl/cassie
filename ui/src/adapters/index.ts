import { client } from "./client";
import { createAdapter } from "./generated/api";

// Adapter boundary: keep transport concerns and generation output together.
export const apiv1 = createAdapter(client);
export const api = apiv1;

export { client, createAdapter };
export type * from "./generated/api";
