import type { RouteAuthState } from "@askrjs/askr/router";

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

export async function resolveRouteAuth(): Promise<RouteAuthState<AuthSession, AuthSession>> {
  if (memorySession !== null) {
    return { session: memorySession, user: memorySession };
  }

  try {
    const session = unwrapResponse(await apiv1.getRestSession(), "Unable to restore session");
    memorySession = session;
    return { session, user: session };
  } catch {
    memorySession = null;
    return { session: null, user: null };
  }
}
