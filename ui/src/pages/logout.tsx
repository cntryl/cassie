import { state } from "@askrjs/askr";
import { Link, navigate } from "@askrjs/askr/router";
import { DatabaseIcon, LogOutIcon, TriangleAlertIcon } from "@askrjs/lucide";
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
  Page,
  Separator,
  Text,
} from "@askrjs/themes/components";

import { apiv1 } from "@/adapters";
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
              <CardTitle>Sign out{session?.user ? ` of ${session.user}` : ""}?</CardTitle>
              <CardDescription>
                This revokes the server-backed session and clears its cookie.
              </CardDescription>
            </CardHeader>
            <CardContent>
              <Block direction="column" gap="md">
                {error() ? (
                  <Alert
                    variant="danger"
                    title="Sign out failed"
                    description={error()}
                    icon={<TriangleAlertIcon size={16} />}
                  />
                ) : null}
                <Separator decorative />
                <Button
                  type="button"
                  variant="destructive"
                  width="full"
                  onPress={handleSignOut}
                  disabled={isSigningOut()}
                >
                  <LogOutIcon size={16} aria-hidden="true" />
                  {isSigningOut() ? "Signing out…" : "Sign out"}
                </Button>
                <Button asChild variant="outline" width="full" disabled={isSigningOut()}>
                  <Link href="/">Stay signed in</Link>
                </Button>
              </Block>
            </CardContent>
            <CardFooter>
              <Text tone="muted" size="sm">
                Signing out returns you to the login screen.
              </Text>
            </CardFooter>
          </Card>
        </Block>
      </Block>
    </Page>
  );
}
