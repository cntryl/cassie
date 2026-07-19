import type { AuthContext } from "@askrjs/askr/router";

import { apiv1 } from "@/adapters";
import { unwrapResponse } from "@/shared/errors/api";

export interface AuthSession {
  user: string;
  database: string;
  role?: string;
}

let memorySession: AuthSession | null = null;

export function getSession(): AuthSession | null {
  return memorySession;
}

// Compatibility-shaped alias; credentials and bearer tokens are never stored.
export function getCredential(): AuthSession | null {
  return getSession();
}

export function isSignedIn(): boolean {
  return memorySession !== null;
}

export function setSession(session: AuthSession): void {
  memorySession = session;
}

// Test/router compatibility helper. Production login uses the REST endpoint.
export function signIn(username: string, _password: string): void {
  setSession({ user: username, database: "postgres", role: "admin" });
}

export function signOut(): void {
  memorySession = null;
}

function routeAuthContext(session: AuthSession | null): AuthContext {
  if (session === null) {
    return {
      authenticated: false,
      principal: null,
      session: null,
      tenant: null,
    };
  }

  return {
    authenticated: true,
    principal: {
      id: session.user,
      subject: session.user,
      roles: session.role ? [session.role] : [],
    },
    session: {
      id: `cassie:${session.user}`,
      subject: session.user,
    },
    tenant: session.database,
  };
}

export async function resolveRouteAuth(): Promise<AuthContext> {
  if (memorySession !== null) {
    return routeAuthContext(memorySession);
  }

  try {
    const session = unwrapResponse(await apiv1.getRestSession(), "Unable to restore session");
    memorySession = session;
    return routeAuthContext(session);
  } catch {
    memorySession = null;
    return routeAuthContext(null);
  }
}
