import { state } from "@askrjs/askr";
import { currentRoute, navigate } from "@askrjs/askr/router";
import { Alert, Block, Button, Field, Input, Label, PageHeader } from "@askrjs/themes/components";

import { AuthBrand } from "@/components/auth/auth-brand";
import { AuthPage } from "@/components/auth/auth-page";
import { createLoginMutation } from "@/features/auth-actions";
import { setSession, signOut } from "@/shared/auth";
import { apiErrorMessage, AppApiError } from "@/shared/errors/api";

function resolveNextTarget() {
  const next = currentRoute().query.get("next");
  return next && next.startsWith("/") && !next.startsWith("//") ? next : "/";
}

function loginErrorMessage(cause: unknown) {
  if (cause instanceof AppApiError) {
    if (cause.status === 401 || cause.status === 403)
      return "The username or password is incorrect.";
    if (cause.status >= 500) return "Cassie is unavailable. Try again in a moment.";
  }
  return apiErrorMessage(cause);
}

export default function LoginPage() {
  const loginMutation = createLoginMutation();
  const [username, setUsername] = state("root");
  const [password, setPassword] = state("");
  const [error, setError] = state("");
  const nextTarget = resolveNextTarget();

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
    <AuthPage>
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
            {error() ? (
              <Alert variant="danger" title="Sign in failed" description={error()} />
            ) : null}
          </div>
        </Block>
      </Block>
    </AuthPage>
  );
}
