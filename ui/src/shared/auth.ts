import type { AuthContext } from "@askrjs/auth";

import { apiv1 } from "@/adapters";
import { AppApiError, unwrapResponse } from "@/shared/errors/api";

export interface AuthSession {
  user: string;
  role?: string;
}

type SessionState =
  | { status: "unknown" | "anonymous" | "unavailable" }
  | { status: "authenticated"; session: AuthSession };

let memorySession: SessionState = { status: "unknown" };

export function getSession(): AuthSession | null {
  return memorySession.status === "authenticated" ? memorySession.session : null;
}

export function getSessionStatus(): SessionState["status"] {
  return memorySession.status;
}

// Compatibility-shaped alias; credentials and bearer tokens are never stored.
export function getCredential(): AuthSession | null {
  return getSession();
}

export function isSignedIn(): boolean {
  return memorySession.status === "authenticated";
}

export function setSession(session: AuthSession): void {
  memorySession = { status: "authenticated", session };
}

// Test/router compatibility helper. Production login uses the REST endpoint.
export function signIn(username: string, _password: string): void {
  setSession({ user: username, role: "admin" });
}

export function signOut(): void {
  memorySession = { status: "anonymous" };
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
    tenant: null,
  };
}

export async function resolveRouteAuth(): Promise<AuthContext> {
  const currentSession = getSession();
  if (currentSession !== null) {
    return routeAuthContext(currentSession);
  }

  try {
    const session = unwrapResponse(await apiv1.getRestSession(), "Unable to restore session");
    setSession(session);
    return routeAuthContext(session);
  } catch (cause) {
    memorySession = {
      status: cause instanceof AppApiError && cause.status === 401 ? "anonymous" : "unavailable",
    };
    return routeAuthContext(null);
  }
}
