import { state } from "@askrjs/askr";
import { watch } from "@askrjs/askr/resources";
import { currentRoute, navigate } from "@askrjs/askr/router";
import {
  Alert,
  Block,
  Button,
  Field,
  Input,
  Label,
  PageHeader,
  Spinner,
} from "@askrjs/themes/components";

import { AuthBrand } from "@/components/auth/auth-brand";
import { AuthPage } from "@/components/auth/auth-page";
import { createLoginMutation, discoverAuthMethods } from "@/features/auth-actions";
import { setSession, signOut } from "@/shared/auth";
import { apiErrorMessage, AppApiError } from "@/shared/errors/api";

function resolveNextTarget(next: string | null) {
  if (
    !next?.startsWith("/") ||
    next.startsWith("//") ||
    next.includes("\\") ||
    [...next].some((character) => character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127)
  ) {
    return "/";
  }

  const target = new URL(next, window.location.origin);
  if (
    target.pathname.startsWith("//") ||
    target.pathname === "/login" ||
    target.pathname === "/logout"
  ) {
    return "/";
  }
  return `${target.pathname}${target.search}${target.hash}`;
}

function oidcStartUrl(next: string) {
  return `/api/v1/auth/oidc/start?${new URLSearchParams({ next })}`;
}

function loginErrorMessage(cause: unknown) {
  if (cause instanceof AppApiError) {
    if (cause.status === 401 || cause.status === 403)
      return "The username or password is incorrect.";
    if (cause.status >= 500) return "Cassie is unavailable. Try again in a moment.";
  }
  return apiErrorMessage(cause);
}

function PasswordLogin({ nextTarget }: { nextTarget: string }) {
  const loginMutation = createLoginMutation();
  const [username, setUsername] = state("root");
  const [password, setPassword] = state("");
  const [error, setError] = state("");

  async function handleSignIn(event?: { preventDefault?: () => void }) {
    event?.preventDefault?.();
    if (loginMutation.pending) {
      return;
    }

    setError("");
    try {
      const session = await loginMutation.execute({
        username: username().trim(),
        password: password(),
      });
      setSession(session);
      setPassword("");
      navigate(nextTarget, {
        history: "replace",
      });
    } catch (caught) {
      signOut();
      setError(loginErrorMessage(caught));
    }
  }

  return (
    <Block direction="column" gap="xl">
      <Block direction="column" gap="md">
        <AuthBrand />
        <PageHeader
          title="Sign in to Cassie Admin"
          description="Sign in as root with the configured root password."
        />
      </Block>

      <Block as="form" direction="column" gap="xl" onSubmit={handleSignIn}>
        <Field>
          <Label for="login-username">Username</Label>
          <Input
            id="login-username"
            name="username"
            autocomplete="username"
            placeholder="root"
            required
            disabled={loginMutation.pending}
            value={username()}
            onInput={(event: Event) => {
              setUsername((event.target as HTMLInputElement).value);
            }}
          />
        </Field>
        <Field>
          <Label for="login-password">Password</Label>
          <Input
            id="login-password"
            name="password"
            type="password"
            autocomplete="current-password"
            placeholder="Enter your password"
            required
            disabled={loginMutation.pending}
            value={password()}
            onInput={(event: Event) => {
              setPassword((event.target as HTMLInputElement).value);
            }}
          />
        </Field>
        <Button
          type="submit"
          variant="primary"
          width="full"
          aria-busy={loginMutation.pending}
          disabled={loginMutation.pending}
        >
          {loginMutation.pending ? "Signing in..." : "Sign in"}
        </Button>

        <div class="cassie-auth-status-shell" aria-live="assertive" aria-atomic="true">
          {error() ? <Alert variant="danger" title="Sign in failed" description={error()} /> : null}
        </div>
      </Block>
    </Block>
  );
}

type LoginMode = "loading" | "password" | "oidc" | "redirecting" | "unavailable";

export default function LoginPage() {
  const { query } = currentRoute();
  const nextTarget = resolveNextTarget(query.get("next"));
  const callbackFailed = query.get("oidc_error") === "1";
  const signedOut = query.get("signed_out") === "1";
  const [attempt, setAttempt] = state(0);
  const [mode, setMode] = state<LoginMode>("loading");

  watch(
    [attempt, () => nextTarget, () => callbackFailed || signedOut],
    async ([, next, stopped], { signal }) => {
      setMode("loading");
      try {
        const methods = await discoverAuthMethods(signal);
        if (signal.aborted) return;
        if (!methods.oidc) {
          setMode("password");
          return;
        }

        if (stopped) {
          setMode("oidc");
          return;
        }

        setMode("redirecting");
        window.location.replace(oidcStartUrl(next));
      } catch {
        if (!signal.aborted) setMode("unavailable");
      }
    },
  );

  const loginMode = mode();
  const redirecting = loginMode === "redirecting";
  const title =
    loginMode === "unavailable"
      ? "Could not determine how to sign in"
      : loginMode === "oidc" && callbackFailed
        ? "Sign in failed"
        : loginMode === "oidc" && signedOut
          ? "Signed out"
          : "Sign in to Cassie Admin";

  return (
    <AuthPage>
      {loginMode === "password" ? (
        <PasswordLogin nextTarget={nextTarget} />
      ) : (
        <Block direction="column" gap="xl">
          <Block direction="column" gap="md">
            <AuthBrand />
            <PageHeader title={title} />
          </Block>
          {loginMode === "loading" || redirecting ? (
            <Spinner
              label={redirecting ? "Opening the sign-in provider" : "Checking sign-in options"}
            />
          ) : null}
          {loginMode === "unavailable" ? (
            <Button variant="primary" width="full" onPress={() => setAttempt((value) => value + 1)}>
              Retry
            </Button>
          ) : null}
          {loginMode === "oidc" ? (
            <Button
              variant="primary"
              width="full"
              onPress={() => window.location.replace(oidcStartUrl(nextTarget))}
            >
              {callbackFailed ? "Try again" : "Sign in"}
            </Button>
          ) : null}
        </Block>
      )}
    </AuthPage>
  );
}
