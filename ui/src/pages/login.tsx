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

import { apiv1 } from "@/adapters";
import { cassieLogoImageProps, cassieLogoPath } from "@/shared/cassie-brand-assets";
import { setSession, signOut } from "@/shared/auth";
import { apiErrorMessage, AppApiError, unwrapResponse } from "@/shared/errors/api";

function resolveNextTarget() {
  const next = currentRoute().query.get("next");
  return next && next.startsWith("/") && !next.startsWith("//") ? next : "/";
}

function loginErrorMessage(error: unknown) {
  if (error instanceof AppApiError) {
    if (error.status === 401 || error.status === 403)
      return "The username or password is incorrect.";
    if (error.status >= 500) return "Cassie is unavailable. Try again in a moment.";
  }
  return apiErrorMessage(error);
}

export default function LoginPage() {
  const [username, setUsername] = state("root");
  const [password, setPassword] = state("");
  const [error, setError] = state("");
  const [isSigningIn, setIsSigningIn] = state(false);
  const nextTarget = resolveNextTarget();

  async function handleSignIn(event?: { preventDefault?: () => void }) {
    event?.preventDefault?.();
    if (isSigningIn()) {
      return;
    }

    setError("");
    setIsSigningIn(true);
    try {
      const session = unwrapResponse(
        await apiv1.loginRestSession({
          body: {
            username: username().trim(),
            password: password(),
          },
        }),
        "Unable to sign in",
      );
      setSession(session);
      setPassword("");
      navigate(nextTarget, {
        history: "replace",
      });
    } catch (caught) {
      signOut();
      setError(loginErrorMessage(caught));
    } finally {
      setIsSigningIn(false);
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
                  disabled={isSigningIn()}
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
                  disabled={isSigningIn()}
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
                aria-busy={isSigningIn()}
                disabled={isSigningIn()}
              >
                {isSigningIn() ? "Signing in..." : "Sign in"}
              </Button>
            </Block>
          </CardContent>
        </Card>
      </Block>
    </Block>
  );
}
