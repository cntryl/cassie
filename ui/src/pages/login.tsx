import { state } from "@askrjs/askr";
import { navigate } from "@askrjs/askr/router";
import { DatabaseIcon, LockIcon, UserIcon } from "@askrjs/lucide";
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
  FieldError,
  Input,
  InputGroup,
  InputGroupText,
  Label,
  Page,
} from "@askrjs/themes/components";

import { apiv1 } from "@/adapters";
import { setSession, signOut } from "@/shared/auth";
import { apiErrorMessage, AppApiError, unwrapResponse } from "@/shared/errors/api";

export function safeLoginContinuation(search: string) {
  const next = new URLSearchParams(search).get("next");
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
  const [username, setUsername] = state("");
  const [password, setPassword] = state("");
  const [error, setError] = state<string | null>(null);
  const [isVerifying, setIsVerifying] = state(false);

  async function handleSignIn(event?: { preventDefault?: () => void }) {
    event?.preventDefault?.();
    if (isVerifying()) {
      return;
    }

    setError(null);
    setIsVerifying(true);
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
      navigate(safeLoginContinuation(window.location.search));
    } catch (caught) {
      signOut();
      setError(loginErrorMessage(caught));
    } finally {
      setIsVerifying(false);
    }
  }

  return (
    <Page background="muted" center>
      <Block as="section" align="center" justify="center" grow>
        <Block width="full" maxWidth="sm" gap="lg">
          <Card variant="raised">
            <CardHeader>
              <Brand>
                <BrandMark aria-hidden="true">
                  <DatabaseIcon size={16} />
                </BrandMark>
                <BrandLabel>Cassie Admin</BrandLabel>
              </Brand>
              <CardTitle titleAs="h1">Sign in to Cassie Admin</CardTitle>
              <CardDescription>Use your operator name and password.</CardDescription>
            </CardHeader>
            <CardContent>
              <Block as="form" direction="column" gap="md" onSubmit={handleSignIn}>
                <Field>
                  <Label for="login-username">Username</Label>
                  <InputGroup>
                    <InputGroupText>
                      <UserIcon size={16} aria-hidden="true" />
                    </InputGroupText>
                    <Input
                      id="login-username"
                      name="username"
                      autocomplete="username"
                      placeholder="admin"
                      required
                      disabled={isVerifying()}
                      value={username()}
                      onInput={(event: Event) => {
                        setUsername((event.target as HTMLInputElement).value);
                      }}
                    />
                  </InputGroup>
                </Field>
                <Field>
                  <Label for="login-password">Password</Label>
                  <InputGroup>
                    <InputGroupText>
                      <LockIcon size={16} aria-hidden="true" />
                    </InputGroupText>
                    <Input
                      id="login-password"
                      name="password"
                      type="password"
                      autocomplete="current-password"
                      required
                      disabled={isVerifying()}
                      value={password()}
                      onInput={(event: Event) => {
                        setPassword((event.target as HTMLInputElement).value);
                      }}
                    />
                  </InputGroup>
                </Field>
                {error() ? <FieldError>{error()}</FieldError> : null}
                <Button type="submit" variant="primary" width="full" disabled={isVerifying()}>
                  {isVerifying() ? "Signing in…" : "Sign in"}
                </Button>
              </Block>
            </CardContent>
          </Card>
        </Block>
      </Block>
    </Page>
  );
}
