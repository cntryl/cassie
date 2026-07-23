import { state } from "@askrjs/askr";
import { Link, navigate } from "@askrjs/askr/router";
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
  FieldError,
} from "@askrjs/themes/components";

import { apiv1 } from "@/adapters";
import cassieLogo from "@/assets/cassie-logo.png";
import { getSession, signOut } from "@/shared/auth";
import { apiErrorMessage, ensureResponseOk } from "@/shared/errors/api";

export default function LogoutPage() {
  const session = getSession();
  const [error, setError] = state<string | null>(null);
  const [isSigningOut, setIsSigningOut] = state(false);

  async function handleSignOut() {
    if (isSigningOut()) {
      return;
    }

    setError(null);
    setIsSigningOut(true);
    try {
      const response = await apiv1.logoutRestSession();
      if (!response.ok && response.status !== 401) {
        ensureResponseOk(response, "Unable to sign out");
      }
      signOut();
      navigate("/login");
    } catch (caught) {
      setError(apiErrorMessage(caught));
    } finally {
      setIsSigningOut(false);
    }
  }

  return (
    <Block as="main" class="cassie-login-page" background="canvas">
      <Block class="cassie-login-panel" width="full" gap="lg">
        <Card class="cassie-login-card" variant="raised">
          <CardHeader>
            <Brand>
              <BrandMark aria-hidden="true">
                <img class="cassie-brand-logo" src={cassieLogo} alt="" />
              </BrandMark>
              <BrandLabel>Cassie Admin</BrandLabel>
            </Brand>
            <CardTitle titleAs="h1">Sign out of Cassie Admin?</CardTitle>
            <CardDescription>
              {session?.user ? `You’re signed in as ${session.user}.` : "End your current session?"}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Block direction="column" gap="xl">
              {error() ? <FieldError>Sign out failed. {error()}</FieldError> : null}
              <Block direction="column" gap="md">
                <Button
                  type="button"
                  variant="destructive"
                  width="full"
                  onPress={handleSignOut}
                  disabled={isSigningOut()}
                >
                  {isSigningOut() ? "Signing out…" : "Sign out"}
                </Button>
                <Button asChild variant="outline" width="full" disabled={isSigningOut()}>
                  <Link href="/">Stay signed in</Link>
                </Button>
              </Block>
            </Block>
          </CardContent>
        </Card>
      </Block>
    </Block>
  );
}
