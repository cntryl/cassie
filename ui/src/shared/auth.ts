import type { RouteAuthState } from "@askrjs/askr/router";

const AUTH_STORAGE_KEY = "cassie-admin-credential";
let memoryCredential: AuthCredential | null = null;

export interface AuthCredential {
  username: string;
  password: string;
}

function browserStorage(): Storage | null {
  if (typeof window === "undefined") {
    return null;
  }

  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function getCredential(): AuthCredential | null {
  const storage = browserStorage();
  if (!storage) {
    return memoryCredential;
  }

  const raw = storage.getItem(AUTH_STORAGE_KEY);
  if (raw === null) {
    return memoryCredential;
  }

  try {
    const parsed = JSON.parse(raw) as Partial<AuthCredential>;
    if (typeof parsed.username === "string" && typeof parsed.password === "string") {
      return { username: parsed.username, password: parsed.password };
    }
  } catch {
    // Corrupt or foreign value under this key — treat as signed out.
  }

  return null;
}

export function isSignedIn(): boolean {
  return getCredential() !== null;
}

export function signIn(username: string, password: string): void {
  const credential = { username, password };
  const storage = browserStorage();
  if (!storage) {
    memoryCredential = credential;
    return;
  }

  memoryCredential = null;
  storage.setItem(AUTH_STORAGE_KEY, JSON.stringify(credential));
}

export function signOut(): void {
  memoryCredential = null;
  const storage = browserStorage();
  if (!storage) {
    return;
  }

  storage.removeItem(AUTH_STORAGE_KEY);
}

// Cassie's REST API expects `Authorization: Bearer <user>:<password>` (see
// src/rest/router.rs's parse_rest_credentials) — not a real bearer token. If
// no username was given, the backend accepts `Bearer <password>` alone and
// falls back to its own configured default user, so omit the empty "user:"
// prefix rather than sending a malformed credential.
export function getAuthorizationHeader(): string | null {
  const credential = getCredential();
  if (!credential) {
    return null;
  }

  const { username, password } = credential;
  return username.length > 0 ? `Bearer ${username}:${password}` : `Bearer ${password}`;
}

// Wired into registerRoutes(..., { auth: { resolve: resolveRouteAuth, ... } })
// in pages/_routes.tsx — askr's router calls this to decide whether a route
// guarded with `auth: true` should render or redirect to loginPath, and
// whether a guest-only route (`auth: "guest"`, e.g. /login) should redirect
// an already-signed-in user to guestRedirectTo instead.
export function resolveRouteAuth(): RouteAuthState<AuthCredential, AuthCredential> {
  const credential = getCredential();
  return { session: credential, user: credential };
}
