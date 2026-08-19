import { createMutation, queryScope } from "@askrjs/askr/data";

import { apiv1, type Session } from "@/adapters";
import { ensureResponseOk, unwrapResponse } from "@/shared/errors/api";

const authMutations = queryScope("auth-mutations");

export function createLoginMutation() {
  return createMutation<{ username: string; password: string }, Session>({
    key: authMutations.key("login"),
    action: async ({ username, password }, { signal }) =>
      unwrapResponse(
        await apiv1.loginRestSession({ body: { username, password }, signal }),
        "Unable to sign in",
      ),
  });
}

export function createLogoutMutation() {
  return createMutation<void, void>({
    key: authMutations.key("logout"),
    action: async (_input, { signal }) => {
      const response = await apiv1.logoutRestSession({ signal });
      if (!response.ok && response.status !== 401) {
        ensureResponseOk(response, "Unable to sign out");
      }
    },
  });
}
