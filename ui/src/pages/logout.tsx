import { Link, navigate } from "@askrjs/askr/router";
import { DatabaseIcon, LogOutIcon } from "@askrjs/lucide";
import {
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

export default function LogoutPage() {
  const session = getSession();

  async function handleSignOut() {
    await apiv1.logoutRestSession();
    signOut();
    navigate("/login");
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
                <Separator decorative />
                <Button type="button" variant="destructive" width="full" onPress={handleSignOut}>
                  <LogOutIcon size={16} aria-hidden="true" />
                  Sign out
                </Button>
                <Button asChild variant="outline" width="full">
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
