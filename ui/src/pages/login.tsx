import { state } from "@askrjs/askr";
import { navigate } from "@askrjs/askr/router";
import { DatabaseIcon, LockIcon, TriangleAlertIcon, UserIcon } from "@askrjs/lucide";
import {
  Alert,
  Block,
  Brand,
  BrandLabel,
  BrandMark,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
  Field,
  Input,
  InputGroup,
  InputGroupText,
  Label,
  Page,
  Text,
} from "@askrjs/themes/components";

import { apiv1 } from "@/adapters";
import { setSession, signOut } from "@/shared/auth";
import { apiErrorMessage, unwrapResponse } from "@/shared/errors/api";

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
          username: username().trim(),
          password: password(),
        }),
        "Unable to sign in",
      );
      setSession(session);
      setPassword("");
      navigate("/");
    } catch (caught) {
      signOut();
      setError(apiErrorMessage(caught));
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
              <CardTitle>Sign in</CardTitle>
              <CardDescription>Enter your admin credentials to continue.</CardDescription>
            </CardHeader>
            <CardContent>
              <Block as="form" direction="column" gap="md" onSubmit={handleSignIn}>
                {error() ? (
                  <Alert
                    variant="danger"
                    title="Sign in failed"
                    description={error()}
                    icon={<TriangleAlertIcon size={16} />}
                  />
                ) : null}
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
                      value={password()}
                      onInput={(event: Event) => {
                        setPassword((event.target as HTMLInputElement).value);
                      }}
                    />
                  </InputGroup>
                </Field>
                <Button type="submit" variant="primary" width="full" disabled={isVerifying()}>
                  {isVerifying() ? "Signing in…" : "Sign in"}
                </Button>
              </Block>
            </CardContent>
            <CardFooter>
              <Text tone="muted" size="sm">
                The server issues an opaque HttpOnly session cookie; your password is not stored in
                this browser.
              </Text>
            </CardFooter>
          </Card>
        </Block>
      </Block>
    </Page>
  );
}
