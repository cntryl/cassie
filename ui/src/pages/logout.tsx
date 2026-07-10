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

import { getCredential, signOut } from "@/shared/auth";

export default function LogoutPage() {
  const credential = getCredential();

  function handleSignOut() {
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
              <CardTitle>
                Sign out{credential && credential.username.length > 0 ? ` of ${credential.username}` : ""}?
              </CardTitle>
              <CardDescription>
                This clears the stored credential for this browser only.
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
