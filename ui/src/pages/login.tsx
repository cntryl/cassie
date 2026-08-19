import { state } from "@askrjs/askr";
import { currentRoute, navigate } from "@askrjs/askr/router";
import {
  Block,
  Brand,
  BrandLabel,
  BrandMark,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  Field,
  Input,
  Label,
  Text,
} from "@askrjs/themes/components";

import { createLoginMutation } from "@/features/auth-actions";
import { cassieLogoImageProps, cassieLogoPath } from "@/shared/cassie-brand-assets";
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
    <Block as="main" class="cassie-login-page" background="canvas">
      <Block class="cassie-login-panel" width="full" gap="lg">
        <Card class="cassie-login-card" variant="raised">
          <CardHeader>
            <Brand>
              <BrandMark class="cassie-brand-mark" aria-hidden="true">
                <img src={cassieLogoPath} {...cassieLogoImageProps} width="32" height="32" alt="" />
              </BrandMark>
              <BrandLabel>Cassie Admin</BrandLabel>
            </Brand>
            <CardTitle titleAs="h1">Sign in to Cassie Admin</CardTitle>
            <CardDescription>Sign in as root with the configured root password.</CardDescription>
          </CardHeader>
          <CardContent>
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
                  required
                  disabled={loginMutation.pending}
                  value={password()}
                  onInput={(event: Event) => {
                    setPassword((event.target as HTMLInputElement).value);
                  }}
                />
              </Field>
              <div aria-live="assertive" aria-atomic="true">
                {error() ? (
                  <Text tone="danger" size="sm">
                    {error()}
                  </Text>
                ) : null}
              </div>
              <Button
                type="submit"
                variant="primary"
                width="full"
                aria-busy={loginMutation.pending}
                disabled={loginMutation.pending}
              >
                {loginMutation.pending ? "Signing in..." : "Sign in"}
              </Button>
            </Block>
          </CardContent>
        </Card>
      </Block>
    </Block>
  );
}
