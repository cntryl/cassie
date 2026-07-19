import { client } from "./client";

// Adapter boundary: keep transport concerns and generation output together.
export const apiv1 = client;
export const api = apiv1;

export { client };
export { createApiClient } from "./generated";
export type * from "./generated";
